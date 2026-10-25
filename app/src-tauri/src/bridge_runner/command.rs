use crate::bridge_registry::BridgeRegistry;
use std::path::Path;
use tokio::process::Command;

pub(super) const OFFICIAL_CLI_AUTH_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(45);
pub(super) const OFFICIAL_CLI_COMMAND_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(180);

#[cfg(windows)]
pub(super) const CREATE_NO_WINDOW: u32 = 0x08000000;

/// All Unix cleanup paths share the same held Child, including before I/O starts.
#[cfg(unix)]
pub(super) struct ProcessGroupCleanup {
    owner: Option<std::sync::Arc<crate::process_group::ProcessOwner>>,
    activity: Option<crate::profile_manager::activity::BridgeActivityContext>,
    ticket: Option<crate::profile_manager::activity::PendingExitTicket>,
    registration: Option<crate::bridge_registry::BridgeRegistration>,
}
#[cfg(unix)]
impl ProcessGroupCleanup {
    pub(super) fn new(owner: std::sync::Arc<crate::process_group::ProcessOwner>) -> Self {
        Self {
            owner: Some(owner),
            activity: crate::profile_manager::activity::current_bridge_activity(),
            ticket: None,
            registration: None,
        }
    }
    fn track(mut self, registry: Option<&BridgeRegistry>) -> Self {
        self.activity = self
            .activity
            .take()
            .or_else(crate::profile_manager::activity::current_bridge_activity);
        if let (Some(registry), Some(owner)) = (registry, self.owner.as_ref()) {
            self.registration = Some(registry.register(owner.clone()));
        }
        self
    }
    fn cancellation_requested(&self) -> bool {
        self.registration
            .as_ref()
            .is_some_and(|entry| entry.cancellation_requested())
    }
    fn retire_registration(&mut self) {
        if self
            .registration
            .as_ref()
            .is_some_and(|registration| registration.retire())
        {
            self.owner = None;
            if let Some(ticket) = self.ticket.take() {
                ticket.confirm_exit();
            }
        }
    }
    fn prepare_finish(mut self) -> Self {
        self.retire_registration();
        if let (Some(activity), Some(owner)) = (self.activity.take(), self.owner.as_ref()) {
            self.ticket = Some(activity.retain_until_exit(owner.clone()));
        }
        self
    }
    fn finish(mut self) -> std::io::Result<()> {
        if let Some(owner) = self.owner.as_ref() {
            owner.terminate()?;
        }
        self.owner = None;
        if let Some(ticket) = self.ticket.take() {
            ticket.confirm_exit();
        }
        Ok(())
    }
}
#[cfg(unix)]
impl Drop for ProcessGroupCleanup {
    fn drop(&mut self) {
        self.retire_registration();
        if let Some(owner) = self.owner.take() {
            let ticket = self.ticket.take().or_else(|| {
                self.activity
                    .take()
                    .map(|activity| activity.retain_until_exit(owner.clone()))
            });
            // Retain the anchor before scheduling. Even thread creation failure
            // keeps an owned instance; no orphan reaper may recycle its PID.
            let _ = owner.request_kill();
            let retained = owner.clone();
            if std::thread::Builder::new()
                .name("bridge-cleanup".into())
                .spawn(move || {
                    if owner.terminate().is_ok() {
                        if let Some(ticket) = ticket {
                            ticket.confirm_exit();
                        }
                    } else {
                        crate::process_group::ProcessOwner::cleanup_in_background(owner);
                    }
                })
                .is_err()
            {
                crate::process_group::ProcessOwner::cleanup_in_background(retained);
            }
        }
    }
}

#[cfg(windows)]
#[path = "windows_child.rs"]
mod windows_child;
#[cfg(windows)]
pub(super) use windows_child::BridgeChild;
#[cfg(unix)]
#[path = "unix_child.rs"]
mod unix_child;
#[cfg(unix)]
pub(super) use unix_child::BridgeChild;

/// Windows owns the assigned Job handle for the full bridge lifecycle.
pub(super) fn spawn_bridge(command: Command) -> std::io::Result<BridgeChild> {
    #[cfg(windows)]
    {
        BridgeChild::spawn(command)
    }
    #[cfg(not(windows))]
    {
        crate::process_group::check_supported()?;
        let mut command = command;
        command.kill_on_drop(false).process_group(0);
        command.spawn().map(BridgeChild::from)
    }
}

pub(super) fn official_cli_command(path: &Path) -> Command {
    // The resolver accepts dependency-free native entries only.
    let mut command = Command::new(path);
    configure_process_supervision(&mut command);
    hide_windows_console(&mut command);
    command
}

pub(super) fn configure_process_supervision(command: &mut Command) {
    #[cfg(windows)]
    command.kill_on_drop(true);
    #[cfg(unix)]
    command.kill_on_drop(false).process_group(0);
}

// A bridge returns one bounded JSON result. stderr is diagnostic text, not a
// second payload channel. Exceeding either cap fails instead of parsing a prefix.
const MAX_BRIDGE_STDOUT: usize = 64 * 1024 * 1024;
const MAX_BRIDGE_STDERR: usize = 8 * 1024 * 1024;

async fn read_bounded_pipe<R: tokio::io::AsyncRead + Unpin>(
    reader: Option<R>,
    limit: usize,
    channel: &str,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let Some(mut reader) = reader else {
        return Ok(Vec::new());
    };
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let remaining = limit - output.len();
        let requested = buffer.len().min(remaining.saturating_add(1));
        let count = match reader.read(&mut buffer[..requested]).await {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if count == 0 {
            return Ok(output);
        }
        if count > remaining {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("桥接 {channel} 输出超过容量上限（{limit} 字节），请缩小读取范围后重试。"),
            ));
        }
        if output.capacity() - output.len() < count {
            let capacity = output
                .capacity()
                .saturating_mul(2)
                .max(8192)
                .max(output.len() + count)
                .min(limit);
            output
                .try_reserve_exact(capacity - output.len())
                .map_err(std::io::Error::other)?;
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

pub(super) async fn wait_for_bridge_output(
    child: impl Into<BridgeChild>,
    active_bridges: Option<&BridgeRegistry>,
    timeout: std::time::Duration,
    input: Option<&[u8]>,
) -> Result<std::io::Result<std::process::Output>, tokio::time::error::Elapsed> {
    wait_for_output_with_limits(
        child,
        active_bridges,
        timeout,
        input,
        MAX_BRIDGE_STDOUT,
        MAX_BRIDGE_STDERR,
    )
    .await
}

async fn wait_for_output_with_limits(
    child: impl Into<BridgeChild>,
    active_bridges: Option<&BridgeRegistry>,
    timeout: std::time::Duration,
    input: Option<&[u8]>,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<std::io::Result<std::process::Output>, tokio::time::error::Elapsed> {
    use tokio::io::AsyncWriteExt;
    let child = child.into();
    #[cfg(windows)]
    let (mut child, group_cleanup) = child.into_parts();
    #[cfg(unix)]
    let (owner, group_cleanup, stdin, stdout, stderr) = child.into_parts();
    let group_cleanup = group_cleanup.track(active_bridges);
    let cancelled_before_io = group_cleanup.cancellation_requested();
    #[cfg(windows)]
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let write_input = async move {
        if cancelled_before_io {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "同步已终止。",
            ));
        }
        if let Some(input) = input {
            let mut stdin = stdin.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "桥接输入管道未打开。")
            })?;
            stdin.write_all(input).await?;
        }
        Ok::<(), std::io::Error>(())
    };
    // Drain both output pipes while writing input, all under the same deadline.
    #[cfg(unix)]
    let wait_leader = owner.wait_async();
    #[cfg(windows)]
    let wait_leader = {
        let job = group_cleanup.owned_job();
        async move {
            let status = child.wait().await?;
            // A descendant may inherit stdout/stderr. Stop the owned tree when
            // its leader exits, before waiting for those pipes to reach EOF.
            tokio::task::spawn_blocking(move || job.terminate())
                .await
                .map_err(std::io::Error::other)??;
            Ok::<_, std::io::Error>(status)
        }
    };
    let result = tokio::time::timeout(timeout, async {
        let (_, stdout, stderr, status) = tokio::try_join!(
            write_input,
            read_bounded_pipe(stdout, stdout_limit, "stdout"),
            read_bounded_pipe(stderr, stderr_limit, "stderr"),
            wait_leader,
        )?;
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    })
    .await;
    {
        // Neither completed output nor an I/O error/deadline proves tree exit.
        // Keep registration while bounded cleanup runs. The blocking worker
        // owns the guard so dropping this await does not abandon cleanup.
        let group_cleanup = group_cleanup.prepare_finish();
        match tokio::task::spawn_blocking(move || group_cleanup.finish()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Ok(Err(bridge_cleanup_error(&result, error))),
            Err(error) => {
                return Ok(Err(bridge_cleanup_error(
                    &result,
                    std::io::Error::other(error),
                )))
            }
        }
    }
    result
}

fn bridge_cleanup_error(
    original: &Result<std::io::Result<std::process::Output>, tokio::time::error::Elapsed>,
    cleanup: std::io::Error,
) -> std::io::Error {
    let (kind, context) = match original {
        Err(_) => (std::io::ErrorKind::TimedOut, "桥接命令已超时".to_owned()),
        Ok(Err(error)) => (error.kind(), format!("桥接命令失败：{error}")),
        Ok(Ok(_)) => (cleanup.kind(), "桥接命令已结束".to_owned()),
    };
    std::io::Error::new(kind, format!("{context}；无法确认进程组已退出：{cleanup}"))
}

pub(super) async fn wait_for_official_cli_output(
    child: impl Into<BridgeChild>,
    active_bridges: Option<&BridgeRegistry>,
    timeout: std::time::Duration,
) -> anyhow::Result<std::process::Output> {
    match wait_for_bridge_output(child, active_bridges, timeout, None).await {
        Ok(output) => Ok(output?),
        Err(_) => anyhow::bail!(
            "官方CLI执行超过{}秒，已请求终止无响应进程，请重试。",
            timeout.as_secs()
        ),
    }
}

pub(super) fn apply_official_cli_env(command: &mut Command) {
    command.env("PYTHONUTF8", "1");
    command.env("PYTHONIOENCODING", "utf-8");
    hide_windows_console(command);
}

pub(super) fn hide_windows_console(command: &mut Command) {
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

#[cfg(all(test, unix))]
mod registration_tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_before_registration_interrupts_and_cleans_the_late_child() {
        let registry = BridgeRegistry::default();
        assert!(!registry.cancel_all().unwrap());
        let mut command = official_cli_command(Path::new("/bin/sh"));
        let child = command.args(["-c", "exec sleep 60"]).spawn().unwrap();
        let pid = child.id().unwrap();
        let output = wait_for_bridge_output(
            child,
            Some(&registry),
            std::time::Duration::from_secs(60),
            None,
        )
        .await
        .unwrap();
        assert_eq!(output.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        assert!(registry.is_empty());
        assert!(!crate::process_group::exists(pid).unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn actual_child_success_and_timeout_release_registration() {
        let registry = BridgeRegistry::default();
        let mut command = official_cli_command(Path::new("/bin/sh"));
        let child = command.args(["-c", "exit 0"]).spawn().unwrap();
        assert!(wait_for_official_cli_output(
            child,
            Some(&registry),
            std::time::Duration::from_secs(2)
        )
        .await
        .unwrap()
        .status
        .success());
        assert!(registry.is_empty());
        let mut command = official_cli_command(Path::new("/bin/sh"));
        let child = command.args(["-c", "exec sleep 60"]).spawn().unwrap();
        assert!(wait_for_official_cli_output(
            child,
            Some(&registry),
            std::time::Duration::from_millis(20)
        )
        .await
        .is_err());
        assert!(registry.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aborting_an_actual_child_wait_releases_registration() {
        let registry = std::sync::Arc::new(BridgeRegistry::default());
        let worker_registry = registry.clone();
        let task = tokio::spawn(async move {
            let mut command = official_cli_command(Path::new("/bin/sh"));
            let child = command.args(["-c", "exec sleep 60"]).spawn().unwrap();
            wait_for_official_cli_output(
                child,
                Some(&worker_registry),
                std::time::Duration::from_secs(120),
            )
            .await
        });
        let registered = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if !registry.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        task.abort();
        let result = task.await;
        assert!(registered.is_ok(), "fixture child did not start");
        assert!(result.unwrap_err().is_cancelled());
        assert!(registry.is_empty());
    }
}

#[cfg(all(test, unix))]
#[path = "unix_lifecycle_tests.rs"]
mod descendant_tests;

#[cfg(all(test, unix))]
mod input_tests {
    use super::*;
    use std::process::Stdio;

    #[tokio::test]
    async fn timeout_includes_a_blocked_input_write() {
        let mut command = official_cli_command(Path::new("/bin/sh"));
        command
            .args(["-c", "exec sleep 60"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn().unwrap();
        let registry = BridgeRegistry::default();
        let input = vec![b'x'; 1024 * 1024];
        let result = wait_for_bridge_output(
            child,
            Some(&registry),
            std::time::Duration::from_millis(100),
            Some(&input),
        )
        .await;
        // A timeout remains a timeout if subsequent cleanup adds diagnostics.
        // Successful cleanup keeps Elapsed; uncertain cleanup returns TimedOut.
        let timed_out = match &result {
            Err(_) => true,
            Ok(Err(error)) => error.kind() == std::io::ErrorKind::TimedOut,
            Ok(Ok(_)) => false,
        };
        assert!(timed_out, "input write escaped the deadline: {result:?}");
        assert!(registry.is_empty());
    }

    #[tokio::test]
    async fn drains_output_while_writing_input_and_closes_stdin() {
        let mut command = official_cli_command(Path::new("/bin/sh"));
        command
            .args([
                "-c",
                "head -c 262144 /dev/zero; head -c 262144 /dev/zero >&2; cat",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn().unwrap();
        let input = vec![b'x'; 1024 * 1024];
        let output =
            wait_for_bridge_output(child, None, std::time::Duration::from_secs(5), Some(&input))
                .await
                .unwrap()
                .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 262144 + input.len());
        assert_eq!(&output.stdout[262144..], input.as_slice());
        assert_eq!(output.stderr.len(), 262144);
    }

    #[tokio::test]
    async fn missing_input_pipe_reports_error_and_releases_registration() {
        let mut command = official_cli_command(Path::new("/bin/sh"));
        command.args(["-c", "exec sleep 60"]).stdin(Stdio::null());
        let registry = BridgeRegistry::default();
        let output = wait_for_bridge_output(
            command.spawn().unwrap(),
            Some(&registry),
            std::time::Duration::from_secs(1),
            Some(b"fixture"),
        )
        .await
        .unwrap();
        assert_eq!(output.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        assert!(registry.is_empty());
    }
}

#[cfg(test)]
mod output_limit_tests {
    use super::*;

    #[tokio::test]
    async fn accepts_exact_limit_and_rejects_one_extra_byte_without_returning_prefix() {
        assert_eq!(
            read_bounded_pipe(Some(&b"abcd"[..]), 4, "stdout")
                .await
                .unwrap(),
            b"abcd"
        );
        let error = read_bounded_pipe(Some(&b"abcde"[..]), 4, "stdout")
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("stdout"));
        assert!(!error.to_string().contains("abcde"));
        assert!(read_bounded_pipe(Some(&b""[..]), 0, "stderr")
            .await
            .unwrap()
            .is_empty());
        assert!(read_bounded_pipe(Some(&b"x"[..]), 0, "stderr")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn growing_buffer_capacity_stays_within_limit() {
        let data = vec![b'x'; 20000];
        let result = read_bounded_pipe(Some(data.as_slice()), data.len(), "stdout")
            .await
            .unwrap();
        assert_eq!(result, data);
        assert!(result.capacity() <= 20000);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn excessive_stdout_or_stderr_fails_and_releases_registration() {
        use std::process::Stdio;
        for script in [
            "head -c 4096 /dev/zero; exec sleep 60",
            "head -c 4096 /dev/zero >&2; exec sleep 60",
        ] {
            let mut command = official_cli_command(Path::new("/bin/sh"));
            command
                .args(["-c", script])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let registry = BridgeRegistry::default();
            let result = wait_for_output_with_limits(
                command.spawn().unwrap(),
                Some(&registry),
                std::time::Duration::from_secs(2),
                None,
                128,
                256,
            )
            .await
            .unwrap();
            assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
            assert!(registry.is_empty());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preserves_nonzero_status_and_complete_bounded_outputs() {
        use std::process::Stdio;
        let mut command = official_cli_command(Path::new("/bin/sh"));
        command
            .args(["-c", "printf ok; printf error >&2; exit 7"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = wait_for_output_with_limits(
            command.spawn().unwrap(),
            None,
            std::time::Duration::from_secs(2),
            None,
            2,
            5,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"ok");
        assert_eq!(output.stderr, b"error");
    }
}

#[cfg(all(test, windows))]
#[path = "command_windows_tests.rs"]
mod windows_tests;
