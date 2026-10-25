//! Probe a disposable copy on Windows so activation never moves an executed image.
use super::StagingGuard;
use std::fs;
use std::path::Path;
#[cfg(any(windows, test))]
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn probe_command(binary: &Path, health_dir: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .arg("--version")
        .env_clear()
        .current_dir(health_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for name in ["SystemRoot", "WINDIR", "PATH", "TMPDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    for name in [
        "WECOM_CLI_CONFIG_DIR",
        "LARKSUITE_CLI_CONFIG_DIR",
        "DWS_CONFIG_DIR",
        "DWS_CACHE_DIR",
        "DWS_KEYCHAIN_DIR",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
    ] {
        command.env(name, health_dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

#[cfg(any(windows, test))]
fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            fs::create_dir(&target).map_err(|e| e.to_string())?;
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
            #[cfg(windows)]
            {
                let mut permissions = fs::metadata(&target)
                    .map_err(|e| e.to_string())?
                    .permissions();
                if permissions.readonly() {
                    // Only the owned disposable copy needs to be removable.
                    permissions.set_readonly(false);
                    fs::set_permissions(&target, permissions).map_err(|e| e.to_string())?;
                }
            }
        } else {
            return Err("CLI自检副本禁止符号链接或特殊文件。".into());
        }
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn prepare_probe_copy(binary: &Path, staging: &Path) -> Result<(StagingGuard, PathBuf), String> {
    let relative = binary
        .strip_prefix(staging)
        .map_err(|_| "CLI自检入口不在暂存目录内。")?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("CLI自检入口路径无效。".into());
    }
    if fs::symlink_metadata(staging)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("CLI自检暂存目录不能是符号链接。".into());
    }
    let directory = staging
        .parent()
        .ok_or("CLI暂存目录无父目录。")?
        .join(format!(".cli-health-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&directory).map_err(|e| format!("无法创建CLI自检副本：{e}"))?;
    let guard = StagingGuard(directory);
    let payload = guard.0.join("payload");
    fs::create_dir(&payload).map_err(|e| e.to_string())?;
    copy_tree(staging, &payload)?;
    let copied = payload.join(relative);
    if !copied.is_file() {
        return Err("CLI自检副本缺少可执行入口。".into());
    }
    Ok((guard, copied))
}

#[cfg(windows)]
pub(crate) async fn check_binary(binary: &Path, staging: &Path) -> Result<(), String> {
    let (binary, staging) = (binary.to_owned(), staging.to_owned());
    // The worker retains the copy and Job even if the installation future is cancelled.
    tokio::task::spawn_blocking(move || {
        let (guard, copied) = prepare_probe_copy(&binary, &staging)?;
        let (mut child, job) =
            crate::process_job::ProcessJob::spawn(probe_command(&copied, &guard.0))
                .map_err(|e| format!("CLI无法启动，旧安装已保留：{e}"))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Err(error) => break Err(format!("CLI自检等待失败：{error}")),
                Ok(None) if std::time::Instant::now() >= deadline => {
                    break Err("CLI版本自检超时，旧安装已保留。".into())
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        };
        // Leader exit is insufficient: stop and confirm the entire owned tree.
        job.terminate()
            .map_err(|e| format!("CLI自检进程树退出未确认，已停止启用：{e}"))?;
        let _ = child.wait();
        let status = status?;
        if !status.success() {
            return Err(format!("CLI版本自检失败（{status}），旧安装已保留。"));
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("CLI自检任务失败：{e}"))?
}

#[cfg(not(windows))]
pub(crate) async fn check_binary(binary: &Path, staging: &Path) -> Result<(), String> {
    let health_dir = staging.join(".healthcheck");
    fs::create_dir_all(&health_dir).map_err(|e| e.to_string())?;
    let _guard = StagingGuard(health_dir.clone());
    let mut command = tokio::process::Command::from(probe_command(binary, &health_dir));
    command.kill_on_drop(true);
    let status = tokio::time::timeout(std::time::Duration::from_secs(15), command.status())
        .await
        .map_err(|_| "CLI版本自检超时，旧安装已保留。".to_owned())?
        .map_err(|e| format!("CLI无法启动，旧安装已保留：{e}"))?;
    if !status.success() {
        return Err(format!("CLI版本自检失败（{status}），旧安装已保留。"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_copy_preserves_relative_resources_without_touching_staging() {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        fs::create_dir_all(stage.join("bin")).unwrap();
        fs::write(stage.join("bin/cli.exe"), b"owned native fixture").unwrap();
        fs::write(stage.join("package.json"), b"metadata").unwrap();
        let (guard, copied) = prepare_probe_copy(&stage.join("bin/cli.exe"), &stage).unwrap();
        assert!(!copied.starts_with(&stage));
        assert_eq!(fs::read(&copied).unwrap(), b"owned native fixture");
        assert_eq!(
            fs::read(
                copied
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("package.json")
            )
            .unwrap(),
            b"metadata"
        );
        fs::rename(&stage, root.path().join("activated")).unwrap();
        assert!(copied.is_file());
        let copy_root = guard.0.clone();
        drop(guard);
        assert!(!copy_root.exists());
        assert!(root.path().join("activated/bin/cli.exe").is_file());
    }
    #[cfg(windows)]
    #[test]
    fn locked_probe_copy_does_not_block_original_staging_activation() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("cli.exe"), b"owned lock fixture").unwrap();
        let (guard, copied) = prepare_probe_copy(&stage.join("cli.exe"), &stage).unwrap();
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(copied)
            .unwrap();
        fs::rename(&stage, root.path().join("active")).unwrap();
        drop(handle);
        drop(guard);
        assert!(root.path().join("active/cli.exe").is_file());
    }
    #[test]
    fn outside_entry_and_missing_binary_are_rejected_and_copy_is_cleaned() {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        fs::create_dir(&stage).unwrap();
        assert!(prepare_probe_copy(&root.path().join("outside.exe"), &stage).is_err());
        assert!(prepare_probe_copy(&stage.join("missing.exe"), &stage).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn links_never_copy_external_files() {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("cli.exe"), b"fixture").unwrap();
        std::os::unix::fs::symlink("/etc", stage.join("external")).unwrap();
        assert!(prepare_probe_copy(&stage.join("cli.exe"), &stage).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
