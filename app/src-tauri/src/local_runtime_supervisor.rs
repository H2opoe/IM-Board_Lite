use std::net::TcpListener;
#[cfg(windows)]
use std::process::Child;
use std::process::Command;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex as AsyncMutex, MutexGuard};

pub struct LocalRuntimeSupervisor {
    port: AtomicU16,
    process: Mutex<Option<OwnedRuntimeProcess>>,
    generation: AtomicU64,
    startup_lock: AsyncMutex<()>,
}

struct OwnedRuntimeProcess {
    pid: u32,
    generation: u64,
    #[cfg(unix)]
    owner: std::sync::Arc<crate::process_group::ProcessOwner>,
    #[cfg(windows)]
    job: Option<crate::process_job::ProcessJob>,
}

#[cfg(unix)]
impl Drop for LocalRuntimeSupervisor {
    fn drop(&mut self) {
        if let Some(process) = self
            .process
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = process.owner.request_kill();
            crate::process_group::ProcessOwner::cleanup_in_background(process.owner);
        }
    }
}

#[cfg(windows)]
pub type RuntimeChild = Child;
#[cfg(unix)]
#[derive(Debug)]
pub struct RuntimeChild {
    owner: std::sync::Arc<crate::process_group::ProcessOwner>,
    #[cfg(test)]
    pub stdout: Option<std::process::ChildStdout>,
}
#[cfg(unix)]
impl RuntimeChild {
    pub fn id(&self) -> u32 {
        self.owner.pid()
    }
    pub fn observe_exit(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.owner.wait_blocking()
    }
}

impl LocalRuntimeSupervisor {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            port: AtomicU16::new(available_loopback_port()?),
            process: Mutex::new(None),
            generation: AtomicU64::new(0),
            startup_lock: AsyncMutex::new(()),
        })
    }

    pub async fn startup_guard(&self) -> MutexGuard<'_, ()> {
        self.startup_lock.lock().await
    }

    pub fn renew_endpoint(&self) -> Result<String, String> {
        let port = available_loopback_port().map_err(|error| error.to_string())?;
        self.port.store(port, Ordering::SeqCst);
        Ok(self.base_url())
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port.load(Ordering::SeqCst))
    }

    pub fn port(&self) -> u16 {
        self.port.load(Ordering::SeqCst)
    }

    pub fn spawn(&self, command: Command) -> Result<(RuntimeChild, u64), String> {
        // Hold ownership while spawning so stop cannot miss an unregistered child.
        let mut process = self.process.lock().map_err(|error| error.to_string())?;
        if process.is_some() {
            return Err("本地推理服务仍有未回收的进程。".to_owned());
        }
        #[cfg(unix)]
        let child = {
            crate::process_group::check_supported().map_err(|error| error.to_string())?;
            use std::os::unix::process::CommandExt;
            let mut command = command;
            command.process_group(0);
            let mut child = command.spawn().map_err(|error| error.to_string())?;
            #[cfg(test)]
            let stdout = child.stdout.take();
            #[cfg(not(test))]
            let _ = child.stdout.take();
            RuntimeChild {
                owner: crate::process_group::ProcessOwner::from_blocking(child),
                #[cfg(test)]
                stdout,
            }
        };
        #[cfg(windows)]
        let (child, job) =
            crate::process_job::ProcessJob::spawn(command).map_err(|error| error.to_string())?;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *process = Some(OwnedRuntimeProcess {
            pid: child.id(),
            generation,
            #[cfg(unix)]
            owner: child.owner.clone(),
            #[cfg(windows)]
            job: Some(job),
        });
        Ok((child, generation))
    }

    #[cfg(test)]
    pub fn register(&self, pid: u32) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut process) = self.process.lock() {
            *process = Some(OwnedRuntimeProcess {
                pid,
                generation,
                #[cfg(unix)]
                owner: crate::process_group::ProcessOwner::unconfirmed_fixture(),
                #[cfg(windows)]
                job: None,
            });
        }
        generation
    }

    #[cfg(test)]
    pub fn clear_if_owned(&self, pid: u32, generation: u64) -> bool {
        self.process
            .lock()
            .map(|mut process| {
                if process
                    .as_ref()
                    .is_some_and(|owned| owned.pid == pid && owned.generation == generation)
                {
                    *process = None;
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false)
    }

    // A successful leader wait does not prove its owned process tree is gone.
    // Retain registration if waiting or group cleanup cannot establish exit.
    pub fn observe_wait(
        &self,
        pid: u32,
        generation: u64,
        result: &std::io::Result<std::process::ExitStatus>,
    ) -> Result<bool, String> {
        self.observe_wait_with(pid, generation, result, terminate_owned_process)
    }

    fn observe_wait_with(
        &self,
        pid: u32,
        generation: u64,
        result: &std::io::Result<std::process::ExitStatus>,
        finish_tree: impl FnOnce(&OwnedRuntimeProcess) -> Result<(), String>,
    ) -> Result<bool, String> {
        let mut process = self.process.lock().map_err(|error| error.to_string())?;
        if !process
            .as_ref()
            .is_some_and(|owned| owned.pid == pid && owned.generation == generation)
        {
            return Ok(false);
        }
        if result.is_ok() {
            // Keep the registration lock during bounded cleanup so a new
            // generation cannot be registered before this group is finished.
            finish_tree(process.as_ref().expect("ownership checked"))?;
            *process = None;
        }
        Ok(true)
    }

    pub fn has_owned_process(&self) -> bool {
        self.process
            .lock()
            .map(|process| process.is_some())
            .unwrap_or(false)
    }

    pub fn stop(&self) -> Result<bool, String> {
        self.stop_with(terminate_owned_process)
    }

    fn stop_with(
        &self,
        terminate: impl FnOnce(&OwnedRuntimeProcess) -> Result<(), String>,
    ) -> Result<bool, String> {
        // Serialize termination with registration and exit observation. Releasing
        // this lock before signaling lets another observer retire the old group
        // while this stop still holds its numeric identity. Keep ownership on error.
        let mut process = self.process.lock().map_err(|error| error.to_string())?;
        let Some(owned) = process.as_ref() else {
            return Ok(false);
        };
        terminate(owned)?;
        *process = None;
        Ok(true)
    }

    pub async fn wait_until_ready(&self, model: &str, timeout: Duration) -> bool {
        let started_at = Instant::now();
        while started_at.elapsed() < timeout {
            if !self.has_owned_process() {
                return false;
            }
            if self.is_owned_runtime_ready(model).await && self.probe_inference(model).await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
        false
    }

    pub async fn is_owned_runtime_ready(&self, model: &str) -> bool {
        if !self.has_owned_process() {
            return false;
        }
        let Ok(client) = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
        else {
            return false;
        };
        let Ok(response) = client
            .get(format!("{}/models", self.base_url()))
            .send()
            .await
        else {
            return false;
        };
        if !response.status().is_success() {
            return false;
        }
        response
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|value| value.get("data").and_then(|data| data.as_array()).cloned())
            .is_some_and(|models| {
                models.iter().any(|item| {
                    item.get("id")
                        .and_then(|id| id.as_str())
                        .is_some_and(|id| id == model)
                })
            })
    }

    async fn probe_inference(&self, model: &str) -> bool {
        if !self.has_owned_process() {
            return false;
        }
        let Ok(client) = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
        else {
            return false;
        };
        client
            .post(format!("{}/chat/completions", self.base_url()))
            .json(&serde_json::json!({
                "model": model,
                "messages": [{ "role": "user", "content": "ping" }],
                "max_tokens": 1,
                "temperature": 0
            }))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
    }
}

#[cfg(windows)]
impl Drop for LocalRuntimeSupervisor {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn available_loopback_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn terminate_owned_process(process: &OwnedRuntimeProcess) -> Result<(), String> {
    #[cfg(unix)]
    {
        process
            .owner
            .terminate()
            .map_err(|error| format!("终止本地推理进程组失败：{error}"))
    }
    #[cfg(windows)]
    {
        process
            .job
            .as_ref()
            .ok_or_else(|| "本地推理进程缺少 Job 所有权，拒绝按 PID 终止。".to_owned())?
            .terminate()
            .map_err(|error| format!("终止本地推理进程 Job 失败：{error}"))
    }
}

#[cfg(all(test, unix))]
fn process_group_exists(pid: u32) -> bool {
    crate::process_group::exists(pid).expect("fixture process-group query failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_a_dynamic_loopback_endpoint() {
        let supervisor = LocalRuntimeSupervisor::new().expect("supervisor");
        assert!(supervisor.port() > 0);
        assert!(supervisor.base_url().starts_with("http://127.0.0.1:"));
        assert!(supervisor.base_url().ends_with("/v1"));
    }

    #[test]
    fn spawn_failure_does_not_claim_ownership() {
        let supervisor = LocalRuntimeSupervisor::new().unwrap();
        let command = Command::new("imboard-nonexistent-runtime-fixture-708937");
        assert!(supervisor.spawn(command).is_err());
        assert!(!supervisor.has_owned_process());
    }

    #[test]
    fn spawn_refuses_to_replace_unreaped_ownership() {
        let supervisor = LocalRuntimeSupervisor::new().unwrap();
        let generation = supervisor.register(10_001);
        let command = Command::new("imboard-nonexistent-runtime-fixture-708937");
        let result = supervisor.spawn(command);
        let retained = supervisor.clear_if_owned(10_001, generation);
        assert!(result.unwrap_err().contains("未回收"));
        assert!(retained);
    }

    #[test]
    fn failed_stop_keeps_ownership_for_a_successful_retry() {
        let supervisor = LocalRuntimeSupervisor::new().unwrap();
        supervisor.register(10_001);
        let failure = supervisor.stop_with(|pid| {
            assert_eq!(pid.pid, 10_001);
            Err("fixture termination failure".into())
        });
        assert!(failure.is_err());
        assert!(supervisor.has_owned_process());
        assert!(supervisor
            .stop_with(|pid| {
                assert_eq!(pid.pid, 10_001);
                Ok(())
            })
            .unwrap());
        assert!(!supervisor.has_owned_process());
        assert!(!supervisor
            .stop_with(|_| panic!("no registered process"))
            .unwrap());
    }

    #[test]
    fn registration_waits_for_inflight_termination() {
        use std::sync::{mpsc, Arc};
        let supervisor = Arc::new(LocalRuntimeSupervisor::new().unwrap());
        supervisor.register(10_001);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let stopping = Arc::clone(&supervisor);
        let stop = std::thread::spawn(move || {
            stopping.stop_with(|pid| {
                assert_eq!(pid.pid, 10_001);
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
        });
        entered_rx.recv().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (registered_tx, registered_rx) = mpsc::channel();
        let registering = Arc::clone(&supervisor);
        let registration = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let generation = registering.register(10_001);
            registered_tx.send(generation).unwrap();
            generation
        });
        started_rx.recv().unwrap();
        let early = registered_rx.recv_timeout(Duration::from_millis(100));
        // Always release and join before asserting, including the failing baseline.
        release_tx.send(()).unwrap();
        let stopped = stop.join().unwrap();
        let generation = registration.join().unwrap();
        let retained = supervisor.clear_if_owned(10_001, generation);
        assert!(
            early.is_err(),
            "new generation registered during termination"
        );
        assert!(stopped.unwrap());
        assert!(
            retained,
            "completed stop cleared the replacement generation"
        );
    }

    #[test]
    fn wait_failure_keeps_ownership_for_retry() {
        let supervisor = LocalRuntimeSupervisor::new().unwrap();
        let generation = supervisor.register(10_001);
        let failed = Err(std::io::Error::other("fixture wait failure"));
        assert!(supervisor
            .observe_wait(10_001, generation, &failed)
            .unwrap());
        assert!(supervisor.has_owned_process());
        assert!(supervisor
            .stop_with(|pid| {
                assert_eq!(pid.pid, 10_001);
                Ok(())
            })
            .unwrap());
        assert!(!supervisor.has_owned_process());
    }

    #[test]
    fn stale_wait_cannot_claim_a_newer_registration() {
        let supervisor = LocalRuntimeSupervisor::new().unwrap();
        let first = supervisor.register(10_001);
        let next = supervisor.register(10_001);
        let failed = Err(std::io::Error::other("fixture wait failure"));
        assert!(!supervisor.observe_wait(10_001, first, &failed).unwrap());
        assert!(supervisor.has_owned_process());
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        let exited = Ok(std::process::ExitStatus::from_raw(0));
        assert!(!supervisor
            .observe_wait_with(10_001, first, &exited, |_| panic!("stale generation"))
            .unwrap());
        assert!(supervisor.has_owned_process());
        assert!(supervisor
            .observe_wait_with(10_001, next, &exited, |_| Ok(()))
            .unwrap());
        assert!(!supervisor.has_owned_process());
    }

    #[test]
    fn failed_descendant_cleanup_keeps_registration_for_retry() {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        let supervisor = LocalRuntimeSupervisor::new().unwrap();
        let generation = supervisor.register(10_001);
        let exited = Ok(std::process::ExitStatus::from_raw(0));
        assert!(supervisor
            .observe_wait_with(10_001, generation, &exited, |pid| {
                assert_eq!(pid.pid, 10_001);
                Err("fixture group cleanup failure".into())
            })
            .is_err());
        assert!(supervisor.has_owned_process());
        assert!(supervisor
            .observe_wait_with(10_001, generation, &exited, |_| Ok(()))
            .unwrap());
        assert!(!supervisor.has_owned_process());
    }

    #[test]
    fn clears_only_the_process_generation_it_owns() {
        let supervisor = LocalRuntimeSupervisor::new().expect("supervisor");
        let first_generation = supervisor.register(10_001);
        let second_generation = supervisor.register(10_002);
        assert!(!supervisor.clear_if_owned(10_001, first_generation));
        assert!(supervisor.has_owned_process());
        assert!(supervisor.clear_if_owned(10_002, second_generation));
        assert!(!supervisor.has_owned_process());
    }
}

#[cfg(all(test, unix))]
mod process_tree_tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    #[test]
    fn observed_parent_exit_also_cleans_up_its_descendants() {
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "sh -c 'trap \"\" TERM; echo ready; exec sleep 30' & exit 7",
            ])
            .stdout(Stdio::piped());
        let supervisor = LocalRuntimeSupervisor::new().unwrap();
        let (mut child, generation) = supervisor.spawn(command).unwrap();
        let pid = child.id();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim(), "ready");
        let result = child.observe_exit();
        assert_eq!(result.as_ref().unwrap().code(), Some(7));
        // WNOWAIT observed exit; the held leader still reserves its identity.
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, 0);
        assert!(supervisor.observe_wait(pid, generation, &result).unwrap());
        assert!(!supervisor.has_owned_process());
        assert!(!process_group_exists(pid));
    }
    #[test]
    fn stop_can_interrupt_an_observer_without_waiting_for_leader_exit() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec sleep 30"]);
        let supervisor = std::sync::Arc::new(LocalRuntimeSupervisor::new().unwrap());
        let (mut child, generation) = supervisor.spawn(command).unwrap();
        let pid = child.id();
        let observing = supervisor.clone();
        let waiter = std::thread::spawn(move || {
            let result = child.observe_exit();
            observing.observe_wait(pid, generation, &result).unwrap();
            result.unwrap()
        });
        let start = Instant::now();
        assert!(supervisor.stop().unwrap());
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(!waiter.join().unwrap().success());
        assert!(!supervisor.has_owned_process());
    }
}
