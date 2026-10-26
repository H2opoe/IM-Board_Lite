#[path = "dingtalk_identity_lookup.rs"]
mod identity_lookup;
#[path = "dingtalk_read_contract.rs"]
mod read_contract;
use crate::bridge_registry::BridgeRegistry;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Instant;

use crate::security::sanitize_log;

use super::errors::{bridge_error_for, classify_dingtalk_cli_error, not_authenticated_message};
use super::normalizers::{normalize_dingtalk_chats, normalize_dingtalk_messages};
use super::paths::{expand_home, prepare_official_cli_for_runtime};
use super::{
    apply_official_cli_env, official_cli_command, wait_for_official_cli_output, BridgeEnvelope,
    BridgeError, BridgeRequest, OFFICIAL_CLI_COMMAND_TIMEOUT,
};

pub(super) async fn run_official_dingtalk_cli(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
    active_bridges: Option<&BridgeRegistry>,
    started_at: Instant,
) -> anyhow::Result<BridgeEnvelope> {
    let Some(profile) = &request.profile else {
        return Ok(bridge_error_for(
            "dingtalk",
            "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
            "DINGTALK_PROFILE_NOT_FOUND",
            "缺少钉钉账号配置。",
            true,
            started_at,
        ));
    };
    let cli_path = match prepare_official_cli_for_runtime(
        &resource_dir,
        Some(profile),
        "dingtalk",
        "dingtalk-workspace-cli",
        "dws",
    )
    .await
    {
        Ok(path) => path,
        Err(detail) => {
            return Ok(bridge_error_for(
                "dingtalk",
                "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                "DINGTALK_CLI_MISSING",
                &format!("官方CLI准备失败：{detail}"),
                true,
                started_at,
            ));
        }
    };

    // Windows keeps the independently verified, profile-isolated native runtime.
    let cli_path = match crate::runtime::isolated_dws::resolve(&resource_dir, &cli_path) {
        Ok(path) => path,
        Err(detail) => {
            return Ok(bridge_error_for(
                "dingtalk",
                "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                "DINGTALK_CLI_MISSING",
                &detail,
                true,
                started_at,
            ))
        }
    };
    let mut command = official_cli_command(&cli_path);
    apply_official_cli_env(&mut command);
    if let Some(auth_identity) = profile
        .config_json
        .get("authIdentity")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        command.env("DWS_AUTH_IDENTITY", auth_identity);
        command.env("DINGTALK_DWS_AGENTCODE", auth_identity);
    }
    if let Some(tenant) = profile
        .config_json
        .get("tenant")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        command.env("DWS_TENANT", tenant);
    }
    match request.command.as_str() {
        "auth-status" => {
            command
                .arg("auth")
                .arg("status")
                .arg("--format")
                .arg("json");
        }
        "get-self" => {
            command
                .arg("contact")
                .arg("user")
                .arg("get-self")
                .arg("--format")
                .arg("json");
        }
        "list-chats" | "fetch-messages" | "search-messages" => {
            let args = match read_contract::read_args(&request.command, &request.args) {
                Ok(args) => args,
                Err(message) => {
                    return Ok(bridge_error_for(
                        "dingtalk",
                        "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                        "DINGTALK_INVALID_READ_RANGE",
                        &message,
                        true,
                        started_at,
                    ))
                }
            };
            command.args(args);
        }
        "search-groups" => {
            command
                .arg("chat")
                .arg("search")
                .arg("--format")
                .arg("json")
                .arg("--query")
                .arg(request.args.get("query").cloned().unwrap_or_default());
        }
        other => {
            return Ok(bridge_error_for(
                "dingtalk",
                "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                "DINGTALK_UNSUPPORTED_COMMAND",
                &format!("钉钉官方CLI暂不支持应用命令：{other}"),
                true,
                started_at,
            ));
        }
    }

    let profile_cache = cache_dir.join(&profile.id);
    let tmp_dir = profile_cache.join("tmp");
    let dws_cache_dir = profile
        .config_json
        .get("dwsCacheDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .unwrap_or_else(|| profile_cache.join("dws-cache"));
    let dws_keychain_dir = profile
        .config_json
        .get("dwsKeychainDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .unwrap_or_else(|| profile_cache.join("dws-keychain"));
    std::fs::create_dir_all(&tmp_dir)?;
    std::fs::create_dir_all(&dws_cache_dir)?;
    std::fs::create_dir_all(&dws_keychain_dir)?;
    command
        .env("TMPDIR", &tmp_dir)
        .env("DWS_CACHE_DIR", dws_cache_dir)
        .env("DWS_KEYCHAIN_DIR", dws_keychain_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(config_dir) = profile
        .config_json
        .get("configDir")
        .and_then(|value| value.as_str())
    {
        command.env("DWS_CONFIG_DIR", expand_home(config_dir));
    }
    let identity_command = (request.command == "get-self").then(|| {
        let mut next = official_cli_command(&cli_path);
        for (key, value) in command.as_std().get_envs() {
            if let Some(value) = value {
                next.env(key, value);
            } else {
                next.env_remove(key);
            }
        }
        next.stdout(Stdio::piped()).stderr(Stdio::piped());
        next
    });
    #[cfg(windows)]
    let command = {
        let Some(config_dir) = profile
            .config_json
            .get("configDir")
            .and_then(serde_json::Value::as_str)
            .filter(|path| !path.trim().is_empty())
        else {
            return Ok(bridge_error_for(
                "dingtalk",
                "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                "DINGTALK_CLI_FAILED",
                "钉钉账号缺少隔离配置目录，请重新打开账号配置后保存。",
                true,
                started_at,
            ));
        };
        crate::runtime::dws_auth::wrap_command(command, &expand_home(config_dir))
    };
    let child = match super::command::spawn_bridge(command) {
        Ok(child) => child,
        Err(err) => {
            return Ok(bridge_error_for(
                "dingtalk",
                "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                "DINGTALK_CLI_MISSING",
                &format!("无法启动钉钉官方CLI：{err}"),
                true,
                started_at,
            ));
        }
    };
    let output =
        wait_for_official_cli_output(child, active_bridges, OFFICIAL_CLI_COMMAND_TIMEOUT).await?;

    let stderr = sanitize_log(&String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        // Partial read responses may contain real message bodies. Report only
        // error fields; never copy the result ledger into a banner or log.
        let (sanitized_stdout, sanitized_stderr) = if matches!(
            request.command.as_str(),
            "search-messages" | "fetch-messages" | "list-chats"
        ) {
            (
                read_contract::failure_summary_from_output(&stdout, &stderr),
                String::new(),
            )
        } else {
            (sanitize_log(&stdout), stderr.clone())
        };
        let detail = [sanitized_stdout.trim(), sanitized_stderr.trim()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(error) = classify_dingtalk_cli_error(&sanitized_stdout, &sanitized_stderr) {
            return Ok(BridgeEnvelope {
                ok: false,
                data: serde_json::Value::Null,
                warnings: Vec::new(),
                error: Some(error),
                meta: serde_json::json!({
                    "platform": "dingtalk",
                    "source": "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                    "duration_ms": started_at.elapsed().as_millis()
                }),
            });
        }
        let message = if detail.is_empty() {
            "钉钉官方CLI执行失败。".to_owned()
        } else {
            format!("钉钉官方CLI执行失败：{detail}")
        };
        return Ok(bridge_error_for(
            "dingtalk",
            "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
            "DINGTALK_CLI_FAILED",
            &message,
            true,
            started_at,
        ));
    }

    let raw = match serde_json::from_str::<serde_json::Value>(&stdout) {
        Ok(raw) => raw,
        Err(_) => {
            return Ok(bridge_error_for(
                "dingtalk",
                "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                "DINGTALK_INVALID_RESPONSE",
                "钉钉CLI未返回有效JSON，无法确认读取结果。",
                true,
                started_at,
            ))
        }
    };
    if raw.get("error").is_some_and(|error| !error.is_null()) {
        let sanitized_stdout = read_contract::failure_summary(&stdout);
        let error = classify_dingtalk_cli_error(&sanitized_stdout, &stderr).unwrap_or_else(|| {
            let detail = [sanitized_stdout.trim(), stderr.trim()]
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            BridgeError {
                code: "DINGTALK_CLI_FAILED".to_owned(),
                message: if detail.is_empty() {
                    "钉钉官方CLI执行失败。".to_owned()
                } else {
                    format!("钉钉官方CLI执行失败：{detail}")
                },
                recoverable: true,
            }
        });
        return Ok(BridgeEnvelope {
            ok: false,
            data: serde_json::Value::Null,
            warnings: Vec::new(),
            error: Some(error),
            meta: serde_json::json!({
                "platform": "dingtalk",
                "source": "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }
    if matches!(
        request.command.as_str(),
        "search-messages" | "list-chats" | "fetch-messages"
    ) {
        if let Err(problem) = read_contract::complete_payload(&raw, &request.command) {
            return Ok(bridge_error_for(
                "dingtalk",
                "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                problem.code(),
                problem.message(),
                true,
                started_at,
            ));
        }
    }
    if request.command == "auth-status" {
        let authenticated = raw
            .get("authenticated")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        return Ok(BridgeEnvelope {
            ok: authenticated,
            data: raw.clone(),
            warnings: vec![stderr]
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect(),
            error: (!authenticated).then(|| BridgeError {
                code: "DINGTALK_NOT_AUTHENTICATED".to_owned(),
                message: not_authenticated_message("钉钉"),
                recoverable: true,
            }),
            meta: serde_json::json!({
                "platform": "dingtalk",
                "source": "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }
    let (self_ids, identity_warning) = if let Some(command) = identity_command {
        identity_lookup::resolve(command, &raw, &profile.config_json, active_bridges).await?
    } else {
        (Vec::new(), None)
    };
    let data = match request.command.as_str() {
        "get-self" => raw.clone(),
        "list-chats" | "search-groups" => normalize_dingtalk_chats(&raw),
        "fetch-messages" => normalize_dingtalk_messages(&raw, &request.args),
        "search-messages" => normalize_dingtalk_messages(&raw, &request.args),
        _ => serde_json::Value::Array(Vec::new()),
    };
    if matches!(
        request.command.as_str(),
        "search-messages" | "fetch-messages"
    ) {
        let expected = read_contract::complete_payload(&raw, &request.command)
            .ok()
            .and_then(|payload| payload.get("messages"))
            .and_then(|value| value.as_array())
            .map_or(0, Vec::len);
        if data.as_array().map_or(0, Vec::len) != expected {
            return Ok(bridge_error_for(
                "dingtalk",
                "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                "DINGTALK_INVALID_RESPONSE",
                "钉钉部分消息缺少有效时间或正文，未将其误计为零条或完整读取。",
                true,
                started_at,
            ));
        }
    }
    Ok(BridgeEnvelope {
        ok: true,
        data,
        warnings: vec![stderr, identity_warning.unwrap_or_default()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect(),
        error: None,
        meta: serde_json::json!({
            "platform": "dingtalk",
            "source": "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
            "duration_ms": started_at.elapsed().as_millis(),
            "selfIds": self_ids
        }),
    })
}
