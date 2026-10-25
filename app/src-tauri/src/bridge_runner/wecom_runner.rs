#[path = "wecom_read_compat/mod.rs"]
mod read_compat;
use crate::bridge_registry::BridgeRegistry;
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
                "未能读取企业微信机器人身份，请确认绑定命令已完成并生成有效配置。",
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
        let child = match super::command::spawn_bridge(command) {
            Ok(child) => child,
            Err(err) => {
                return Ok(bridge_error(
                    "WECOM_CLI_MISSING",
                    &format!("无法启动企业微信官方CLI：{err}"),
                    true,
                    started_at,
                ));
            }
        };
        let output =
            wait_for_official_cli_output(child, active_bridges, OFFICIAL_CLI_AUTH_TIMEOUT).await?;
        if !output.status.success() {
            if let Some(error) = classify_wecom_cli_error(&String::from_utf8_lossy(&output.stderr))
            {
                return Ok(BridgeEnvelope {
                    ok: false,
                    data: serde_json::Value::Null,
                    warnings: vec![],
                    error: Some(error),
                    meta: serde_json::json!({"platform":"wecom"}),
                });
            }
            return Ok(bridge_error(
                "WECOM_IDENTITY_UNAVAILABLE",
                "未能读取企业微信机器人身份，请确认绑定命令已完成并生成有效配置。",
                true,
                started_at,
            ));
        }
        let output = String::from_utf8_lossy(&output.stdout);
        let authorized = output
            .lines()
            .any(|line| line.trim() == "Status: authorized");
        if request.command == "auth-status" {
            return Ok(BridgeEnvelope {
                ok: authorized,
                data: serde_json::json!({"authenticated": authorized}),
                warnings: Vec::new(),
                error: (!authorized).then(|| BridgeError {
                    code: "WECOM_NOT_AUTHENTICATED".into(),
                    message: not_authenticated_message("企业微信"),
                    recoverable: true,
                }),
                meta: serde_json::json!({"platform":"wecom"}),
            });
        }
        let bot_id = output
            .lines()
            .find_map(|line| line.trim().strip_prefix("Bot ID:"))
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let Some(bot_id) = bot_id else {
            return Ok(bridge_error(
                "WECOM_IDENTITY_UNAVAILABLE",
                "未能读取企业微信机器人身份，请确认绑定命令已完成并生成有效配置。",
                true,
                started_at,
            ));
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
            meta: serde_json::json!({
                "platform": "wecom",
                "source": "https://github.com/WecomTeam/wecom-cli",
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }
    Ok(bridge_error(
        "WECOM_UNSUPPORTED_COMMAND",
        "企业微信不支持该读取命令。",
        true,
        started_at,
    ))
}
