use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::connectors::{self, ConnectorKind};
use crate::security::sanitize_log;
use crate::storage::models::ImProfile;

mod dingtalk_runner;
mod errors;
mod feishu_runner;
mod normalizers;
mod paths;
mod process;
mod wecom_runner;
use dingtalk_runner::run_official_dingtalk_cli;
#[cfg(test)]
use errors::*;
use feishu_runner::run_official_feishu_cli;
#[cfg(test)]
use normalizers::*;
use paths::resolve_bridge_executable;
use process::{bridge_process_spec, bridge_spawn_error};
use wecom_runner::run_official_wecom_cli;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
pub(super) const APP_DATA_DIR_NAME: &str = "IMBoard";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRequest {
    pub platform: String,
    pub command: String,
    pub profile: Option<ImProfile>,
    pub args: HashMap<String, String>,
    #[serde(default)]
    pub stdin_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEnvelope {
    pub ok: bool,
    pub data: serde_json::Value,
    pub warnings: Vec<String>,
    pub error: Option<BridgeError>,
    pub meta: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

pub async fn run_bridge(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
) -> anyhow::Result<BridgeEnvelope> {
    run_bridge_tracked(request, resource_dir, cache_dir, None).await
}

pub async fn run_bridge_tracked(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
    active_pids: Option<&Mutex<Vec<u32>>>,
) -> anyhow::Result<BridgeEnvelope> {
    let started_at = Instant::now();
    if let Some(adapter) = connectors::find(&request.platform) {
        match adapter.kind {
            ConnectorKind::Wecom => {
                return run_official_wecom_cli(
                    request,
                    resource_dir,
                    cache_dir,
                    active_pids,
                    started_at,
                )
                .await;
            }
            ConnectorKind::Feishu => {
                return run_official_feishu_cli(
                    request,
                    resource_dir,
                    cache_dir,
                    active_pids,
                    started_at,
                )
                .await;
            }
            ConnectorKind::Dingtalk => {
                return run_official_dingtalk_cli(
                    request,
                    resource_dir,
                    cache_dir,
                    active_pids,
                    started_at,
                )
                .await;
            }
        }
    }
    let executable = resolve_bridge_executable(&resource_dir, &request);
    let process = bridge_process_spec(&request.platform, executable, &resource_dir);
    let mut command = Command::new(&process.executable);
    command.args(&process.prefix_args);

    command.arg(&request.command).arg("--format").arg("json");

    if let Some(profile) = &request.profile {
        command.arg("--profile-id").arg(&profile.id);
        if let Some(config_path) = profile
            .config_json
            .get("configPath")
            .and_then(|v| v.as_str())
        {
            command.arg("--config").arg(config_path);
        }
        if let Some(keys_path) = profile.config_json.get("keysPath").and_then(|v| v.as_str()) {
            command.arg("--keys-file").arg(keys_path);
        }
        command.env("IMD_PROFILE_CONFIG_JSON", profile.config_json.to_string());
        let profile_cache = cache_dir.join(&profile.id);
        let tmp_dir = profile_cache.join("tmp");
        std::fs::create_dir_all(&tmp_dir)?;
        command.env("TMPDIR", tmp_dir);
    }

    for (key, value) in request.args {
        if request.platform == "wechat" && matches!(key.as_str(), "chat_name" | "chat_type") {
            continue;
        }
        command
            .arg(format!("--{}", key.replace('_', "-")))
            .arg(value);
    }

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = if let Some(stdin_secret) = request.stdin_secret {
        command.stdin(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                return Ok(bridge_spawn_error(
                    &request.platform,
                    &process,
                    err,
                    started_at.elapsed().as_millis(),
                ));
            }
        };
        let child_id = child.id();
        register_pid(active_pids, child_id);
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(stdin_secret.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
        }
        let output = child.wait_with_output().await?;
        unregister_pid(active_pids, child_id);
        output
    } else {
        let child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                return Ok(bridge_spawn_error(
                    &request.platform,
                    &process,
                    err,
                    started_at.elapsed().as_millis(),
                ));
            }
        };
        let child_id = child.id();
        register_pid(active_pids, child_id);
        let output = child.wait_with_output().await?;
        unregister_pid(active_pids, child_id);
        output
    };
    let stderr = sanitize_log(&String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Ok(mut envelope) = serde_json::from_str::<BridgeEnvelope>(&stdout) {
        envelope.meta = merge_meta(
            envelope.meta,
            serde_json::json!({ "duration_ms": started_at.elapsed().as_millis() }),
        );
        if !stderr.is_empty() {
            envelope.warnings.push(stderr);
        }
        return Ok(envelope);
    }

    if !output.status.success() {
        let stdout_detail = sanitize_log(stdout.trim());
        let warnings = [stderr.as_str(), stdout_detail.as_str()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        return Ok(BridgeEnvelope {
            ok: false,
            data: serde_json::Value::Null,
            warnings,
            error: Some(BridgeError {
                code: "BRIDGE_CRASHED".to_owned(),
                message: "Bridge进程执行失败".to_owned(),
                recoverable: true,
            }),
            meta: serde_json::json!({
                "platform": request.platform,
                "executable": process.executable.to_string_lossy(),
                "script": process.script_path.map(|path| path.to_string_lossy().to_string()),
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }

    let mut envelope: BridgeEnvelope = serde_json::from_str(&stdout)?;
    envelope.meta = merge_meta(
        envelope.meta,
        serde_json::json!({ "duration_ms": started_at.elapsed().as_millis() }),
    );
    if !stderr.is_empty() {
        envelope.warnings.push(stderr);
    }
    Ok(envelope)
}

include!("command.rs");
fn merge_meta(mut left: serde_json::Value, right: serde_json::Value) -> serde_json::Value {
    if let (Some(left_map), Some(right_map)) = (left.as_object_mut(), right.as_object()) {
        for (key, value) in right_map {
            left_map.insert(key.clone(), value.clone());
        }
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_feishu_b2c_app_history_error() {
        let stdout = r#"{
          "ok": false,
          "identity": "user",
          "error": {
            "type": "api_error",
            "code": 231204,
            "message": "HTTP 400: The app type is not supported, ext=b2c app not support",
            "detail": null
          }
        }"#;

        let error = classify_feishu_cli_error(stdout, "").expect("classified");
        assert_eq!(error.code, "FEISHU_B2C_APP_UNSUPPORTED");
        assert!(error.recoverable);
        assert!(error.message.contains("已跳过"));
    }

    #[test]
    fn classifies_dingtalk_developer_settings_permission_error() {
        let stdout = r#"{
          "error": {
            "action_url": "https://open-dev.dingtalk.com/fe/old#/developerSettings",
            "category": "api",
            "code": 1,
            "friendly_hint": "该组织尚未开启 CLI数据访问权限，请联系组织主管理员开启。",
            "message": "business error: success=false",
            "reason": "business_error",
            "server_error_code": "TOKEN_VERIFIED_FAILED",
            "server_key": "group-chat"
          }
        }"#;

        let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
        assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
        assert!(error.recoverable);
        assert!(error.message.contains("CLI数据访问权限"));
    }

    #[test]
    fn classifies_sanitized_dingtalk_permission_error() {
        let stdout = r#"{
          "error": {
            "action_url": "https://open-dev.dingtalk.com/fe/old#/developerSettings",
            "category": "api",
            "code": 1,
            "friendly_hint": "该组织尚未开启 CLI数据访问权限，请联系组织主管理员开启。",
            "message": "business error: success=false",
token=***
            "reason": "business_error",
            "server_key": "group-chat"
          }
        }"#;

        let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
        assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
        assert!(error.recoverable);
        assert!(error.message.contains("CLI数据访问权限"));
    }

    #[test]
    fn classifies_dingtalk_pat_permission_error() {
        let stdout = r#"{
          "code": "PAT_MEDIUM_RISK_NO_PERMISSION",
          "data": {
            "requiredScopes": [
              {
                "scope": "chat.message:list"
              }
            ]
          },
          "success": false
        }"#;

        let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
        assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
        assert!(error.recoverable);
        assert!(error.message.contains("消息读取权限"));
    }

    #[test]
    fn classifies_dingtalk_group_chat_forbidden_error() {
        let stdout = r#"{
          "error": {
            "category": "api",
            "code": 1,
            "hint": "The API returned a business-level error. Check required parameters and values.",
            "message": "forbidden request",
            "operation": "tools/call",
            "reason": "business_error",
            "server_error_code": "1001",
            "server_key": "group-chat",
            "trace_id": "21030ead17780641308991861e0a34"
          }
        }"#;

        let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
        assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
        assert!(error.recoverable);
        assert!(error.message.contains("消息读取权限"));
    }

    #[test]
    fn filters_feishu_cli_page_progress() {
        let output = "[page 1] fetching...\n[page 1] fetched 50 items\n真实警告";

        assert_eq!(sanitize_feishu_cli_output(output), "真实警告");
    }

    #[test]
    fn parses_feishu_json_with_page_progress() {
        let stdout =
            "[page 1] fetching...\n{\"items\":[{\"chat_id\":\"oc_1\",\"name\":\"产品群\"}]}\n";

        let raw = parse_feishu_cli_json(stdout).expect("json parsed");
        let chats = normalize_feishu_chats(&raw);

        assert_eq!(
            chats
                .as_array()
                .and_then(|items| items.first())
                .and_then(|item| item.get("chatName"))
                .and_then(|value| value.as_str()),
            Some("产品群")
        );
    }
}
