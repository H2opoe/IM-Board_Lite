#[path = "wecom_read_compat/mod.rs"]
mod read_compat;
use crate::bridge_registry::BridgeRegistry;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Instant;

use super::errors::{bridge_error, classify_wecom_cli_error, not_authenticated_message};

use super::paths::{expand_home, prepare_official_cli_for_runtime};
use super::{
    apply_official_cli_env, official_cli_command, wait_for_official_cli_output, BridgeEnvelope,
    BridgeError, BridgeRequest, OFFICIAL_CLI_AUTH_TIMEOUT,
};

pub(super) async fn run_official_wecom_cli(
    request: BridgeRequest,
    resource_dir: PathBuf,
    _cache_dir: PathBuf,
    active_bridges: Option<&BridgeRegistry>,
    started_at: Instant,
) -> anyhow::Result<BridgeEnvelope> {
    let Some(profile) = &request.profile else {
        return Ok(bridge_error(
            "WECOM_PROFILE_NOT_FOUND",
            "缺少企业微信账号配置。",
            true,
            started_at,
        ));
    };
    if matches!(
        request.command.as_str(),
        "list-contacts" | "list-chats" | "fetch-messages"
    ) {
        return Ok(read_compat::bridge(profile, &request.command, &request.args, started_at).await);
    }
    let cli_path = match prepare_official_cli_for_runtime(
        &resource_dir,
        Some(profile),
        "wecom",
        "wecom-cli",
        "wecom-cli",
    )
    .await
    {
        Ok(path) => path,
        Err(detail) => {
            return Ok(bridge_error(
                "WECOM_CLI_MISSING",
                &format!("官方CLI准备失败：{detail}"),
                true,
                started_at,
            ));
        }
    };
    let mut command = official_cli_command(&cli_path);
    apply_official_cli_env(&mut command);
    if matches!(request.command.as_str(), "account-identity" | "auth-status") {
        let Some(config_dir) = profile
            .config_json
            .get("configDir")
            .and_then(|value| value.as_str())
        else {
            return Ok(bridge_error(
                "WECOM_IDENTITY_UNAVAILABLE",
                &not_authenticated_message("企业微信"),
                true,
                started_at,
            ));
        };
        command.env("WECOM_CLI_CONFIG_DIR", expand_home(config_dir));
        command
            .arg("auth")
            .arg("show")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut diagnostic = serde_json::json!({
            "configDirNonAscii": !config_dir.is_ascii(),
            "configPathFingerprint": format!("{:x}", Sha256::digest(config_dir.as_bytes())),
            "cliPathFingerprint": format!("{:x}", Sha256::digest(cli_path.to_string_lossy().as_bytes())),
            "accessTokenEnvPresent": std::env::var_os("WECOM_CLI_ACCESS_TOKEN").is_some(),
            "diagnosticSchema": 2,
            "cliPackageVersion": crate::runtime::official_cli::cli_spec("wecom").and_then(|spec| crate::runtime::official_cli::official_cli_version(&cli_path, spec))
        });
        let dir = expand_home(config_dir);
        for (name, field) in [
            ("credentials.enc", "credentialsFile"),
            ("bot.enc", "legacyCredentialsFile"),
            (".encryption_key", "keyFile"),
        ] {
            let status = match std::fs::symlink_metadata(dir.join(name)) {
                Ok(m) if m.file_type().is_symlink() => "symlink",
                Ok(m) if !m.is_file() => "not_file",
                Ok(_) => match std::fs::File::open(dir.join(name)) {
                    Ok(_) => "readable",
                    Err(_) => "unreadable",
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => "absent",
                Err(_) => "unreadable",
            };
            diagnostic[field] = serde_json::json!(status);
            if let Ok(m) = std::fs::symlink_metadata(dir.join(name)) {
                diagnostic[format!("{field}Bytes")] = serde_json::json!(m.len());
                diagnostic[format!("{field}ModifiedEpoch")] = serde_json::json!(m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()));
            }
        }
        let child = match super::command::spawn_bridge(command) {
            Ok(child) => child,
            Err(err) => {
                let mut failure = bridge_error(
                    "WECOM_CLI_MISSING",
                    &format!("无法启动企业微信官方CLI：{err}"),
                    true,
                    started_at,
                );
                diagnostic["authResult"] = serde_json::json!("process_incomplete");
                failure.meta = serde_json::json!({"diagnostic":diagnostic});
                return Ok(failure);
            }
        };
        let output =
            match wait_for_official_cli_output(child, active_bridges, OFFICIAL_CLI_AUTH_TIMEOUT)
                .await
            {
                Ok(output) => output,
                Err(_) => {
                    let mut failure = bridge_error(
                        "WECOM_AUTH_CHECK_FAILED",
                        &not_authenticated_message("企业微信"),
                        true,
                        started_at,
                    );
                    diagnostic["authResult"] = serde_json::json!("process_incomplete");
                    failure.meta = serde_json::json!({"diagnostic":diagnostic});
                    return Ok(failure);
                }
            };
        diagnostic["exitCode"] = serde_json::json!(output.status.code());
        diagnostic["stdoutBytes"] = serde_json::json!(output.stdout.len());
        diagnostic["stderrBytes"] = serde_json::json!(output.stderr.len());
        if !output.status.success() {
            if let Some(mut error) =
                classify_wecom_cli_error(&String::from_utf8_lossy(&output.stderr))
            {
                error.message = not_authenticated_message("企业微信");
                return Ok(BridgeEnvelope {
                    ok: false,
                    data: serde_json::Value::Null,
                    warnings: vec![],
                    error: Some(error),
                    meta: serde_json::json!({"platform":"wecom", "diagnostic":diagnostic}),
                });
            }
            let mut failure = bridge_error(
                "WECOM_IDENTITY_UNAVAILABLE",
                &not_authenticated_message("企业微信"),
                true,
                started_at,
            );
            failure.meta = serde_json::json!({"diagnostic":diagnostic});
            return Ok(failure);
        }
        let output = String::from_utf8_lossy(&output.stdout);
        let (authorized, unauthorized) = parse_auth_status(&output);
        diagnostic["authResult"] = serde_json::json!(if authorized && !unauthorized {
            "authorized"
        } else if unauthorized && !authorized {
            "unauthorized"
        } else {
            "unrecognized"
        });
        let authorized = authorized && !unauthorized;
        let meta = serde_json::json!({"platform":"wecom", "diagnostic": diagnostic});
        if !authorized || request.command == "auth-status" {
            return Ok(BridgeEnvelope {
                ok: authorized,
                data: serde_json::json!({"authenticated": authorized}),
                warnings: Vec::new(),
                error: (!authorized).then(|| BridgeError {
                    code: if diagnostic["authResult"] == "unrecognized" {
                        "WECOM_AUTH_OUTPUT_UNRECOGNIZED"
                    } else {
                        "WECOM_NOT_AUTHENTICATED"
                    }
                    .into(),
                    message: not_authenticated_message("企业微信"),
                    recoverable: true,
                }),
                meta,
            });
        }
        let bot_id = output
            .lines()
            .find_map(|line| line.trim().strip_prefix("Bot ID:"))
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let Some(bot_id) = bot_id else {
            let mut failure = bridge_error(
                "WECOM_IDENTITY_UNAVAILABLE",
                &not_authenticated_message("企业微信"),
                true,
                started_at,
            );
            failure.meta = meta;
            return Ok(failure);
        };
        return Ok(BridgeEnvelope {
            ok: true,
            data: serde_json::json!({
                "platform": "wecom",
                "botId": bot_id,
                "createTime": serde_json::Value::Null
            }),
            warnings: Vec::new(),
            error: None,
            meta,
        });
    }
    Ok(bridge_error(
        "WECOM_UNSUPPORTED_COMMAND",
        "企业微信不支持该读取命令。",
        true,
        started_at,
    ))
}

// Only exact status lines count; "unauthorized" must never match "authorized".
fn parse_auth_status(output: &str) -> (bool, bool) {
    (
        output.lines().any(|l| l.trim() == "Status: authorized"),
        output.lines().any(|l| l.trim() == "Status: unauthorized"),
    )
}

#[cfg(test)]
mod auth_status_tests {
    use super::parse_auth_status;
    #[test]
    fn status_contract_handles_windows_lines_and_rejects_unknown_output() {
        assert_eq!(
            parse_auth_status("Status: authorized\r\nBot ID: fixture\r\n"),
            (true, false)
        );
        assert_eq!(parse_auth_status("Status: unauthorized\r\n"), (false, true));
        assert_eq!(parse_auth_status("初始化完成"), (false, false));
        assert_eq!(
            parse_auth_status("Status: authorized\nStatus: unauthorized"),
            (true, true)
        );
    }
}
