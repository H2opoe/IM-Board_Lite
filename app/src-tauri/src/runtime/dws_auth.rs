//! Every command uses the account's own DPAPI registry namespace; no global token swap.
use sha2::{Digest, Sha256};
use std::path::Path;

pub(crate) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(crate) fn namespace(config_dir: &Path) -> String {
    let normalized = config_dir
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase();
    format!(
        "{:x}",
        Sha256::digest(format!("im-board-dws-v1\0{normalized}"))
    )
}

pub(crate) fn session(
    config_dir: &Path,
    cli_path: &Path,
    body: &str,
    migrate_legacy: bool,
) -> String {
    let token_path = config_dir.join("keychain/windows-auth-token.regvalue");
    include_str!("dws_auth.ps1")
        .replace("__NATIVE_INVOKER__", include_str!("dws_native.ps1"))
        .replace(
            "__BOUNDED_INVOKER__",
            if body.contains("Invoke-IMBoardDwsBounded") {
                include_str!("dws_bounded.ps1")
            } else {
                ""
            },
        )
        .replace("__NAMESPACE__", &quote(&namespace(config_dir)))
        .replace("__CLI_PATH__", &quote(&cli_path.to_string_lossy()))
        .replace("__SHA256__", &quote(super::isolated_dws::SHA256))
        .replace("__TOKEN_PATH__", &quote(&token_path.to_string_lossy()))
        .replace("__CONFIG_DIR__", &quote(&config_dir.to_string_lossy()))
        .replace(
            "__MIGRATE_LEGACY__",
            if migrate_legacy { "$true" } else { "$false" },
        )
        .replace("__BODY__", body)
}

// Only this application's hashed account namespace is touched; global DWS data is untouched.
pub(crate) fn remove_namespace(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("账号凭据清理标识无效。".into());
    }
    #[cfg(windows)]
    {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let script = include_str!("dws_cleanup.ps1").replace("__NAMESPACE__", &quote(value));
        let mut child = Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "无法启动账号凭据清理；稍后会重试。".to_owned())?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(_)) => return Err("账号正在使用或凭据清理失败；稍后会重试。".into()),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("账号凭据清理未完成；稍后会重试。".into());
                }
            }
        }
    }
    #[cfg(not(windows))]
    Err("Windows 账号凭据需要在 Windows 上清理。".into())
}

#[cfg(windows)]
pub(crate) fn wrap_command(
    command: tokio::process::Command,
    config_dir: &Path,
) -> tokio::process::Command {
    let source = command.as_std();
    let invocation = format!(
        "Invoke-IMBoardDws {}",
        source
            .get_args()
            .map(|arg| quote(&arg.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let script = format!(
        "{}\nexit $dwsExitCode",
        session(
            config_dir,
            Path::new(source.get_program()),
            &invocation,
            true
        )
    );
    let mut wrapped = tokio::process::Command::new("powershell.exe");
    wrapped.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &script,
    ]);
    for (key, value) in source.get_envs() {
        if let Some(value) = value {
            wrapped.env(key, value);
        } else {
            wrapped.env_remove(key);
        }
    }
    if let Some(directory) = source.get_current_dir() {
        wrapped.current_dir(directory);
    }
    wrapped
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespaces_follow_config_identity_and_separate_editions_and_accounts() {
        assert_eq!(
            namespace(Path::new("C:\\IMBoard\\Profiles\\one\\dingtalk")),
            namespace(Path::new("c:/imboard/profiles/one/dingtalk/"))
        );
        let a = namespace(Path::new("C:/IMBoard/Profiles/one/dingtalk"));
        assert_eq!(a.len(), 64);
        assert_ne!(
            a,
            namespace(Path::new("C:/IMBoardLite/Profiles/one/dingtalk"))
        );
        assert_ne!(a, namespace(Path::new("C:/IMBoard/Profiles/two/dingtalk")));
    }
    #[test]
    fn session_has_no_global_registry_swap_and_checks_payload_before_auth() {
        let s = session(
            Path::new("C:/fixture's/config"),
            Path::new("C:/fixture/dws.exe"),
            "& 'fixture.exe'",
            true,
        );
        assert!(s.contains("fixture''s/config"));
        assert!(s.contains("IM_BOARD_DWS_NAMESPACE"));
        assert!(!s.contains(r"HKCU:\Software\DwsCli\keychain"));
        assert!(!s.contains("dwsPreviousToken"));
        assert!(!s.contains("Write-DwsSessionToken"));
        assert!(s.find("Get-FileHash").unwrap() < s.find("ProtectedData]::Unprotect").unwrap());
        assert!(s.contains("corpId -cne $dwsToken.corp_id"));
        assert!(!s.contains("__BODY__"));
        assert!(!s.contains("__NAMESPACE__"));
        assert!(!s.contains("__NATIVE_INVOKER__"));
        assert!(!s.contains("__BOUNDED_INVOKER__"));
        assert!(s.contains("$dwsMutex.WaitOne(0)"));
        assert!(!s.contains("WaitOne(300000)"));
        assert!(s.contains("$dwsCapability = Invoke-IMBoardDws"));
        assert!(s.contains("$ErrorActionPreference = $dwsSavedErrorPreference"));
        assert!(s.contains("$global:LASTEXITCODE = [int]$dwsNativeExitCode"));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn powershell(script: &str) -> Command {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        command
    }
    fn legacy_token(config: &Path) {
        let path = config.join("keychain/windows-auth-token.regvalue");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let script = format!(
            r#"$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Security; $data=[Text.Encoding]::UTF8.GetBytes('{{"access_token":"fixture","corp_id":"fixture-corp","user_id":"fixture-user"}}'); $entropy=[Text.Encoding]::UTF8.GetBytes('dws-cli'+[char]0+'auth-token'); $cipher=[Security.Cryptography.ProtectedData]::Protect($data,$entropy,[Security.Cryptography.DataProtectionScope]::CurrentUser); [IO.File]::WriteAllText({},[Convert]::ToBase64String($cipher))"#,
            quote(&path.to_string_lossy())
        );
        assert!(powershell(&script).status().unwrap().success());
    }
    struct Cleanup(Vec<std::path::PathBuf>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for config in &self.0 {
                let key = format!(
                    r"HKCU:\Software\IMBoard\DwsCli\Profiles\{}",
                    namespace(config)
                );
                let _ = powershell(&format!(
                    "Remove-Item -LiteralPath {} -Recurse -Force -ErrorAction SilentlyContinue",
                    quote(&key)
                ))
                .status();
            }
        }
    }
    #[test]
    fn killed_session_leaves_only_its_own_migrated_namespace_and_other_account_still_works() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a/dingtalk");
        let b = dir.path().join("b/dingtalk");
        let _cleanup = Cleanup(vec![a.clone(), b.clone()]);
        legacy_token(&a);
        legacy_token(&b);
        let resources = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let cli = crate::runtime::isolated_dws::bundled(&resources).unwrap();
        let ready = dir.path().join("ready");
        let body = format!(
            "[IO.File]::WriteAllText({},'ready'); Start-Sleep -Seconds 60",
            quote(&ready.to_string_lossy())
        );
        let mut child = powershell(&session(&a, &cli, &body, true)).spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !ready.exists() && Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let reached = ready.exists();
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            reached,
            "fixture did not reach post-migration interruption point"
        );
        assert!(!a.join("keychain/windows-auth-token.regvalue").exists());
        let verify="if (-not (Test-Path -LiteralPath $dwsRegistryKey)) { throw 'missing own namespace' }; $global:LASTEXITCODE=0";
        assert!(powershell(&session(&b, &cli, verify, true))
            .status()
            .unwrap()
            .success());
        assert!(!b.join("keychain/windows-auth-token.regvalue").exists());
        assert!(powershell(&session(&a, &cli, verify, true))
            .status()
            .unwrap()
            .success());
    }
    #[test]
    fn legacy_identity_mismatch_preserves_file_and_does_not_import() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("identity/dingtalk");
        let _cleanup = Cleanup(vec![config.clone()]);
        legacy_token(&config);
        std::fs::write(config.join("profiles.json"),r#"{"currentProfile":"fixture","profiles":[{"name":"fixture","corpId":"other-corp","userId":"other-user"}]}"#).unwrap();
        let resources = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let cli = crate::runtime::isolated_dws::bundled(&resources).unwrap();
        assert!(
            !powershell(&session(&config, &cli, "$global:LASTEXITCODE=0", true))
                .status()
                .unwrap()
                .success()
        );
        assert!(config.join("keychain/windows-auth-token.regvalue").exists());
    }
    #[test]
    fn cleanup_removes_only_own_credentials_and_rejects_delayed_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a/dingtalk");
        let b = dir.path().join("b/dingtalk");
        let _cleanup = Cleanup(vec![a.clone(), b.clone()]);
        legacy_token(&a);
        legacy_token(&b);
        let resources = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let cli = crate::runtime::isolated_dws::bundled(&resources).unwrap();
        for config in [&a, &b] {
            assert!(
                powershell(&session(config, &cli, "$global:LASTEXITCODE=0", true))
                    .status()
                    .unwrap()
                    .success()
            );
        }
        remove_namespace(&namespace(&a)).unwrap();
        remove_namespace(&namespace(&a)).unwrap(); // idempotent after partial cleanup/restart
        assert!(
            !powershell(&session(&a, &cli, "$global:LASTEXITCODE=0", true))
                .status()
                .unwrap()
                .success()
        );
        assert!(powershell(&session(&b,&cli,"if (-not (Test-Path -LiteralPath $dwsRegistryKey)) { throw 'missing account' }; $global:LASTEXITCODE=0",true)).status().unwrap().success());
        let key = format!(r"HKCU:\Software\IMBoard\DwsCli\Profiles\{}", namespace(&a));
        assert!(powershell(&format!(
            "if (@(Get-ChildItem -LiteralPath {}).Count -ne 0) {{ throw 'credentials remain' }}",
            quote(&key)
        ))
        .status()
        .unwrap()
        .success());
    }
    #[test]
    fn active_session_defers_cleanup_until_mutex_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("busy/dingtalk");
        let _cleanup = Cleanup(vec![config.clone()]);
        legacy_token(&config);
        let resources = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let cli = crate::runtime::isolated_dws::bundled(&resources).unwrap();
        let ready = dir.path().join("ready");
        let body = format!(
            "[IO.File]::WriteAllText({},'ready'); Start-Sleep -Seconds 60",
            quote(&ready.to_string_lossy())
        );
        let mut child = powershell(&session(&config, &cli, &body, true))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !ready.exists() && Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let reached = ready.exists();
        let busy = if reached {
            remove_namespace(&namespace(&config))
        } else {
            Ok(())
        };
        let _ = child.kill();
        let _ = child.wait();
        assert!(reached, "fixture did not acquire session mutex");
        assert!(busy.is_err());
        remove_namespace(&namespace(&config)).unwrap();
    }
}
