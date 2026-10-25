use super::archives::{download_and_extract_tgz, download_client};
use super::native_artifacts;
use super::resolver::package_dir;
use std::fs;
use std::path::{Path, PathBuf};

// Installs share a lock. The shared executable cache belongs to the application, not a profile.
pub(super) static INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) struct StagingGuard(pub PathBuf);
impl Drop for StagingGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn recover_interrupted_install(install_root: &Path) -> Result<(), String> {
    let parent = install_root.parent().ok_or("官方CLI安装目录无效。")?;
    let name = install_root
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("官方CLI目录名称无效。")?;
    let backup = parent.join(format!(".{name}.previous"));
    if !install_root.exists() && backup.exists() {
        if fs::symlink_metadata(&backup).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("官方CLI恢复备份不能是符号链接。".into());
        }
        rename_install_path(&backup, install_root)
            .map_err(|e| format!("恢复旧CLI失败（备份保留于{}）：{e}", backup.display()))?;
    }
    Ok(())
}

pub(super) fn replace_install_root_atomically(
    install_root: &Path,
    staging_root: &Path,
) -> Result<(), String> {
    replace_install_root_with(install_root, staging_root, rename_install_path)
}

fn replace_install_root_with(
    install_root: &Path,
    staging_root: &Path,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let parent = install_root.parent().ok_or("官方CLI安装目录无效。")?;
    if !staging_root.is_dir() || staging_root.parent() != Some(parent) {
        return Err("官方CLI暂存目录缺失或不在安装目录旁。".into());
    }
    for path in [install_root, staging_root] {
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("官方CLI安装目录不能是符号链接。".into());
        }
    }
    let backup_root = parent.join(format!(
        ".{}.previous",
        install_root
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or("官方CLI目录名称无效。")?
    ));
    recover_interrupted_install(install_root)?;
    if backup_root.exists() {
        fs::remove_dir_all(&backup_root).map_err(|e| format!("无法清理上一份CLI备份：{e}"))?;
    }
    if install_root.exists() {
        rename(install_root, &backup_root).map_err(|e| format!("无法备份旧CLI：{e}"))?;
    }
    if let Err(error) = rename(staging_root, install_root) {
        if backup_root.exists() {
            rename(&backup_root, install_root).map_err(|restore| {
                format!(
                    "启用新CLI失败：{error}；恢复失败：{restore}。旧版本保留于{}",
                    backup_root.display()
                )
            })?;
        }
        let preserved = if install_root.exists() {
            "旧版本已恢复"
        } else {
            "原安装状态未改变"
        };
        return Err(format!(
            "启用新CLI失败（{} → {}），{preserved}：{error}",
            staging_root.display(),
            install_root.display()
        ));
    }
    Ok(())
}

fn rename_install_path(from: &Path, to: &Path) -> std::io::Result<()> {
    rename_with_retry(
        from,
        to,
        cfg!(windows),
        |from, to| fs::rename(from, to),
        std::thread::sleep,
    )
}

fn rename_with_retry(
    from: &Path,
    to: &Path,
    windows: bool,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    mut wait: impl FnMut(std::time::Duration),
) -> std::io::Result<()> {
    // Windows may briefly retain an image/scanner handle after process exit.
    // Retry only access/sharing/lock errors and never fall back to partial copies.
    let delays = [40, 80, 160, 320, 640];
    let mut attempt = 0;
    loop {
        match rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) => {
                if !windows
                    || !matches!(error.raw_os_error(), Some(5 | 32 | 33))
                    || attempt == delays.len()
                {
                    return Err(error);
                }
                wait(std::time::Duration::from_millis(delays[attempt]));
                attempt += 1;
            }
        }
    }
}

pub(super) async fn install_native_packages(
    install_root: &Path,
    package: &str,
    version: &str,
    target: &str,
) -> Result<(), String> {
    let plan = native_artifacts::packages(package, version, target)?;
    let client = download_client()?;
    for artifact in plan {
        let destination = install_root
            .join("node_modules")
            .join(package_dir(&artifact.name));
        download_and_extract_tgz(&client, &artifact.url, &artifact.integrity, &destination).await?;
        validate_package_identity(&destination, &artifact.name, &artifact.version)?;
    }
    Ok(())
}

fn validate_package_identity(directory: &Path, name: &str, version: &str) -> Result<(), String> {
    let text = fs::read(directory.join("package.json"))
        .map_err(|e| format!("CLI包缺少有效元数据：{e}"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&text).map_err(|e| format!("CLI包元数据无效：{e}"))?;
    if value["name"].as_str() != Some(name) || value["version"].as_str() != Some(version) {
        return Err(format!("CLI包标识不匹配：预期{name}@{version}"));
    }
    Ok(())
}

#[path = "healthcheck.rs"]
mod healthcheck;
pub(super) use healthcheck::check_binary;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_rename_retries_only_bounded_sharing_errors() {
        let mut attempts = 0;
        let mut waits = Vec::new();
        rename_with_retry(
            Path::new("stage"),
            Path::new("live"),
            true,
            |_, _| {
                attempts += 1;
                if attempts < 3 {
                    Err(std::io::Error::from_raw_os_error(5))
                } else {
                    Ok(())
                }
            },
            |duration| waits.push(duration.as_millis()),
        )
        .unwrap();
        assert_eq!(attempts, 3);
        assert_eq!(waits, vec![40, 80]);
        for (windows, code, expected) in [
            (true, 32, 6),
            (true, 33, 6),
            (true, 5, 6),
            (true, 2, 1),
            (false, 5, 1),
        ] {
            let mut attempts = 0;
            let result = rename_with_retry(
                Path::new("stage"),
                Path::new("live"),
                windows,
                |_, _| {
                    attempts += 1;
                    Err(std::io::Error::from_raw_os_error(code))
                },
                |_| {},
            );
            assert_eq!(attempts, expected);
            assert_eq!(result.unwrap_err().raw_os_error(), Some(code));
        }
    }
    #[test]
    fn failed_publication_restores_old_tree_and_first_install_reports_no_fake_recovery() {
        for existing in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let live = dir.path().join("cli");
            let stage = dir.path().join("stage");
            fs::create_dir(&stage).unwrap();
            fs::write(stage.join("version"), "new").unwrap();
            if existing {
                fs::create_dir(&live).unwrap();
                fs::write(live.join("version"), "old").unwrap();
            }
            let error = replace_install_root_with(&live, &stage, |from, to| {
                if from == stage {
                    Err(std::io::Error::from_raw_os_error(5))
                } else {
                    fs::rename(from, to)
                }
            })
            .unwrap_err();
            assert_eq!(error.contains("旧版本已恢复"), existing);
            assert!(stage.join("version").exists());
            if existing {
                assert_eq!(fs::read_to_string(live.join("version")).unwrap(), "old");
            } else {
                assert!(!live.exists());
            }
        }
    }
    #[test]
    fn failed_restore_preserves_backup_for_next_recovery() {
        let root = tempfile::tempdir().unwrap();
        let live = root.path().join("cli");
        let stage = root.path().join("stage");
        fs::create_dir(&live).unwrap();
        fs::write(live.join("version"), "old").unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("version"), "new").unwrap();
        let backup = root.path().join(".cli.previous");
        let error = replace_install_root_with(&live, &stage, |from, to| {
            if from == stage || from == backup {
                Err(std::io::Error::from_raw_os_error(5))
            } else {
                fs::rename(from, to)
            }
        })
        .unwrap_err();
        assert!(error.contains("恢复失败"));
        assert_eq!(fs::read_to_string(backup.join("version")).unwrap(), "old");
        assert!(!live.exists());
        recover_interrupted_install(&live).unwrap();
        assert_eq!(fs::read_to_string(live.join("version")).unwrap(), "old");
        assert_eq!(fs::read_to_string(stage.join("version")).unwrap(), "new");
    }
    #[test]
    fn package_identity_is_checked_instead_of_synthesized() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_package_identity(dir.path(), "@wecom/cli", "0.1.9").is_err());
        fs::write(
            dir.path().join("package.json"),
            r#"{"name":"@wecom/cli","version":"0.1.9"}"#,
        )
        .unwrap();
        assert!(validate_package_identity(dir.path(), "@wecom/cli", "0.1.9").is_ok());
        assert!(validate_package_identity(dir.path(), "@wecom/cli", "0.2.0").is_err());
        assert!(validate_package_identity(dir.path(), "another", "0.1.9").is_err());
    }
    #[test]
    fn activation_preserves_old_install_and_recovers_interrupted_backup() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("cli");
        let stage = dir.path().join("stage");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("version"), "old").unwrap();
        assert!(replace_install_root_atomically(&dest, &stage).is_err());
        assert_eq!(fs::read_to_string(dest.join("version")).unwrap(), "old");
        fs::rename(&dest, dir.path().join(".cli.previous")).unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("version"), "new").unwrap();
        replace_install_root_atomically(&dest, &stage).unwrap();
        assert_eq!(fs::read_to_string(dest.join("version")).unwrap(), "new");
        assert_eq!(
            fs::read_to_string(dir.path().join(".cli.previous/version")).unwrap(),
            "old"
        );
    }
    #[tokio::test]
    async fn canceled_install_removes_only_staging() {
        let dir = tempfile::tempdir().unwrap();
        let stage = dir.path().join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(dir.path().join("old"), "keep").unwrap();
        let future = async {
            let _guard = StagingGuard(stage.clone());
            std::future::pending::<()>().await;
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), future)
                .await
                .is_err()
        );
        assert!(!stage.exists());
        assert!(dir.path().join("old").exists());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn failed_native_probe_does_not_activate_or_leave_configuration() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_binary(Path::new("/usr/bin/false"), dir.path())
            .await
            .is_err());
        assert!(!dir.path().join(".healthcheck").exists());
        assert!(check_binary(Path::new("/usr/bin/true"), dir.path())
            .await
            .is_ok());
    }
}
