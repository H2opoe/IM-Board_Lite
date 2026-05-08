use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Instant;

use super::errors::{
    bridge_error_for, classify_feishu_cli_error, parse_feishu_cli_json, sanitize_feishu_cli_output,
    validate_feishu_auth_status,
};
use super::normalizers::{
    feishu_time_arg, normalize_feishu_chats, normalize_feishu_message_sessions,
    normalize_feishu_messages,
};
use super::paths::{expand_home, resolve_official_cli_for_runtime};
use super::{
    apply_official_cli_env, official_cli_command, register_pid, unregister_pid, BridgeEnvelope,
    BridgeRequest,
};

pub(super) async fn run_official_feishu_cli(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
    active_pids: Option<&Mutex<Vec<u32>>>,
    started_at: Instant,
) -> anyhow::Result<BridgeEnvelope> {
    let Some(profile) = &request.profile else {
        return Ok(bridge_error_for(
            "feishu",
            "https://github.com/larksuite/cli",
            "FEISHU_PROFILE_NOT_FOUND",
            "缺少飞书账号配置。",
            true,
            started_at,
        ));
    };
    let Some(cli_path) = resolve_official_cli_for_runtime(
        &resource_dir,
        Some(profile),
        "feishu",
        "@larksuite/cli",
        "lark-cli",
    ) else {
        return Ok(bridge_error_for(
            "feishu",
            "https://github.com/larksuite/cli",
            "FEISHU_CLI_MISSING",
            "官方CLI尚未准备完成，请重新打开绑定窗口等待准备完成，或重新安装IM-Board。",
            true,
            started_at,
        ));
    };
    let profile_name = profile
        .config_json
        .get("profileName")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&profile.id);

    let mut command = official_cli_command(&cli_path);
    apply_official_cli_env(&mut command);
    if let Some(lark_config_dir) = feishu_lark_config_dir(&profile.config_json) {
        std::fs::create_dir_all(&lark_config_dir)?;
        command.env("LARKSUITE_CLI_CONFIG_DIR", lark_config_dir);
    }
    command.arg("--profile").arg(profile_name);
    match request.command.as_str() {
        "list-chats" => {
            command
                .arg("im")
                .arg("chats")
                .arg("list")
                .arg("--as")
                .arg("user")
                .arg("--page-all")
                .arg("--format")
                .arg("json");
            let page_size = request
                .args
                .get("limit")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(200)
                .clamp(1, 100);
            command
                .arg("--params")
                .arg(format!(r#"{{"page_size":{page_size}}}"#));
        }
        "fetch-messages" => {
            let chat_id = request.args.get("chat").cloned().unwrap_or_default();
            if chat_id.trim().is_empty() {
                return Ok(bridge_error_for(
                    "feishu",
                    "https://github.com/larksuite/cli",
                    "MISSING_CHAT",
                    "fetch-messages缺少chat参数。",
                    true,
                    started_at,
                ));
            }
            command
                .arg("im")
                .arg("+chat-messages-list")
                .arg("--as")
                .arg("user")
                .arg("--chat-id")
                .arg(chat_id)
                .arg("--page-size")
                .arg("50")
                .arg("--format")
                .arg("json");
            if let Some(start) = request
                .args
                .get("start_time")
                .and_then(|value| feishu_time_arg(value))
            {
                command.arg("--start").arg(start);
            }
            if let Some(end) = request
                .args
                .get("end_time")
                .and_then(|value| feishu_time_arg(value))
            {
                command.arg("--end").arg(end);
            }
        }
        "search-messages" => {
            command
                .arg("im")
                .arg("+messages-search")
                .arg("--as")
                .arg("user")
                .arg("--page-all")
                .arg("--page-size")
                .arg("50")
                .arg("--format")
                .arg("json");
            if let Some(start) = request
                .args
                .get("start_time")
                .and_then(|value| feishu_time_arg(value))
            {
                command.arg("--start").arg(start);
            }
            if let Some(end) = request
                .args
                .get("end_time")
                .and_then(|value| feishu_time_arg(value))
            {
                command.arg("--end").arg(end);
            }
        }
        "auth-status" => {
            command.arg("auth").arg("status").arg("--verify");
        }
        other => {
            return Ok(bridge_error_for(
                "feishu",
                "https://github.com/larksuite/cli",
                "FEISHU_UNSUPPORTED_COMMAND",
                &format!("飞书官方CLI暂不支持应用命令：{other}"),
                true,
                started_at,
            ));
        }
    }

    let profile_cache = cache_dir.join(&profile.id);
    let tmp_dir = profile_cache.join("tmp");
    std::fs::create_dir_all(&tmp_dir)?;
    command
        .env("TMPDIR", tmp_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Ok(bridge_error_for(
                "feishu",
                "https://github.com/larksuite/cli",
                "FEISHU_CLI_MISSING",
                &format!("无法启动飞书官方CLI：{err}"),
                true,
                started_at,
            ));
        }
    };
    let child_id = child.id();
    register_pid(active_pids, child_id);
    let output = child.wait_with_output().await?;
    unregister_pid(active_pids, child_id);

    let stderr = sanitize_feishu_cli_output(&String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let sanitized_stdout = sanitize_feishu_cli_output(&stdout);
        let detail = [sanitized_stdout.trim(), stderr.trim()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(error) = classify_feishu_cli_error(&sanitized_stdout, &stderr) {
            return Ok(BridgeEnvelope {
                ok: false,
                data: serde_json::Value::Null,
                warnings: Vec::new(),
                error: Some(error),
                meta: serde_json::json!({
                    "platform": "feishu",
                    "source": "https://github.com/larksuite/cli",
                    "duration_ms": started_at.elapsed().as_millis()
                }),
            });
        }
        let message = if detail.is_empty() {
            "飞书官方CLI执行失败。".to_owned()
        } else {
            format!("飞书官方CLI执行失败：{detail}")
        };
        return Ok(bridge_error_for(
            "feishu",
            "https://github.com/larksuite/cli",
            "FEISHU_CLI_FAILED",
            &message,
            true,
            started_at,
        ));
    }

    let sanitized_stdout = sanitize_feishu_cli_output(&stdout);
    let raw = parse_feishu_cli_json(&stdout)
        .unwrap_or_else(|| serde_json::json!({ "text": sanitized_stdout.clone() }));
    if request.command == "auth-status" {
        if let Some(error) = validate_feishu_auth_status(&raw) {
            return Ok(BridgeEnvelope {
                ok: false,
                data: raw.clone(),
                warnings: Vec::new(),
                error: Some(error),
                meta: serde_json::json!({
                    "platform": "feishu",
                    "source": "https://github.com/larksuite/cli",
                    "duration_ms": started_at.elapsed().as_millis()
                }),
            });
        }
    } else if let Some(error) = classify_feishu_cli_error(&sanitized_stdout, &stderr) {
        return Ok(BridgeEnvelope {
            ok: false,
            data: raw.clone(),
            warnings: Vec::new(),
            error: Some(error),
            meta: serde_json::json!({
                "platform": "feishu",
                "source": "https://github.com/larksuite/cli",
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }
    let data = match request.command.as_str() {
        "list-chats" => normalize_feishu_chats(&raw),
        "fetch-messages" => normalize_feishu_messages(&raw, &request.args),
        "search-messages" => normalize_feishu_message_sessions(&raw),
        "auth-status" => serde_json::json!({ "authenticated": true, "raw": raw }),
        _ => serde_json::Value::Array(Vec::new()),
    };
    Ok(BridgeEnvelope {
        ok: true,
        data,
        warnings: vec![stderr]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect(),
        error: None,
        meta: serde_json::json!({
            "platform": "feishu",
            "source": "https://github.com/larksuite/cli",
            "duration_ms": started_at.elapsed().as_millis()
        }),
    })
}

fn feishu_lark_config_dir(config: &serde_json::Value) -> Option<PathBuf> {
    if let Some(config_dir) = config
        .get("larkConfigDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        return Some(expand_home(config_dir));
    }
    config
        .get("configDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| expand_home(value).join(".lark-cli"))
}
