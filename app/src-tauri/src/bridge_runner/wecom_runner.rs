use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Instant;

use crate::security::sanitize_log;

use super::errors::{bridge_error, classify_wecom_cli_error, not_authenticated_message};
use super::normalizers::{
    is_wecom_group_chat, normalize_wecom_chats, normalize_wecom_contacts, normalize_wecom_messages,
    now_text, remove_empty_json_fields, today_start_text, unwrap_wecom_cli_payload,
};
use super::paths::{expand_home, resolve_official_cli_for_runtime};
use super::{
    apply_official_cli_env, official_cli_command, register_pid, unregister_pid, BridgeEnvelope,
    BridgeError, BridgeRequest,
};

pub(super) async fn run_official_wecom_cli(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
    active_pids: Option<&Mutex<Vec<u32>>>,
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
    let Some(cli_path) = resolve_official_cli_for_runtime(
        &resource_dir,
        Some(profile),
        "wecom",
        "wecom-cli",
        "wecom-cli",
    ) else {
        return Ok(bridge_error(
            "WECOM_CLI_MISSING",
            "企业微信官方CLI尚未准备完成，请重新打开绑定窗口等待准备完成或重新安装 IM-Board。",
            true,
            started_at,
        ));
    };
    let mut command = official_cli_command(&cli_path);
    apply_official_cli_env(&mut command);
    if request.command == "auth-status" {
        let config_path = profile
            .config_json
            .get("configPath")
            .and_then(|value| value.as_str())
            .map(expand_home)
            .or_else(|| {
                profile
                    .config_json
                    .get("configDir")
                    .and_then(|value| value.as_str())
                    .map(|value| expand_home(value).join("bot.enc"))
            });
        let is_authenticated = config_path
            .as_ref()
            .and_then(|path| std::fs::metadata(path).ok())
            .is_some_and(|metadata| metadata.is_file() && metadata.len() > 0);
        if is_authenticated {
            return Ok(BridgeEnvelope {
                ok: true,
                data: serde_json::json!({ "authenticated": true }),
                warnings: Vec::new(),
                error: None,
                meta: serde_json::json!({
                    "platform": "wecom",
                    "source": "https://github.com/WecomTeam/wecom-cli",
                    "duration_ms": started_at.elapsed().as_millis()
                }),
            });
        }
        return Ok(bridge_error(
            "WECOM_NOT_AUTHENTICATED",
            &not_authenticated_message("企业微信"),
            true,
            started_at,
        ));
    }
    let payload = match request.command.as_str() {
        "list-contacts" => serde_json::json!({}),
        "list-chats" => serde_json::json!({
            "begin_time": request.args.get("start_time").cloned().unwrap_or_else(today_start_text),
            "end_time": request.args.get("end_time").cloned().unwrap_or_else(now_text),
            "cursor": request.args.get("cursor").cloned().unwrap_or_default(),
        }),
        "fetch-messages" => {
            let chat_id = request.args.get("chat").cloned().unwrap_or_default();
            if chat_id.trim().is_empty() {
                return Ok(bridge_error(
                    "MISSING_CHAT",
                    "fetch-messages 缺少 chat 参数。",
                    true,
                    started_at,
                ));
            }
            let chat_name = request
                .args
                .get("chat_name")
                .cloned()
                .unwrap_or_else(|| chat_id.clone());
            serde_json::json!({
                "chat_type": request
                    .args
                    .get("chat_type")
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or_else(|| if is_wecom_group_chat(&chat_id, &chat_name, &profile.config_json) { 2 } else { 1 }),
                "chatid": chat_id,
                "begin_time": request.args.get("start_time").cloned().unwrap_or_else(today_start_text),
                "end_time": request.args.get("end_time").cloned().unwrap_or_else(now_text),
                "cursor": request.args.get("cursor").cloned().unwrap_or_default(),
            })
        }
        other => {
            return Ok(bridge_error(
                "WECOM_UNSUPPORTED_COMMAND",
                &format!("企业微信官方CLI暂不支持应用命令：{other}"),
                true,
                started_at,
            ));
        }
    };
    let clean_payload = remove_empty_json_fields(payload);
    let (category, method) = match request.command.as_str() {
        "list-contacts" => ("contact", "get_userlist"),
        "list-chats" => ("msg", "get_msg_chat_list"),
        "fetch-messages" => ("msg", "get_message"),
        _ => unreachable!(),
    };
    command
        .arg(category)
        .arg(method)
        .arg(serde_json::to_string(&clean_payload)?)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(config_dir) = profile
        .config_json
        .get("configDir")
        .and_then(|value| value.as_str())
    {
        command.env("WECOM_CLI_CONFIG_DIR", expand_home(config_dir));
    }
    let profile_cache = cache_dir.join(&profile.id);
    let tmp_dir = profile_cache.join("tmp");
    std::fs::create_dir_all(&tmp_dir)?;
    command.env("TMPDIR", tmp_dir);

    let child = match command.spawn() {
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
    let child_id = child.id();
    register_pid(active_pids, child_id);
    let output = child.wait_with_output().await?;
    unregister_pid(active_pids, child_id);

    let stderr = sanitize_log(&String::from_utf8_lossy(&output.stderr));
    let stdout = sanitize_log(&String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        let cli_detail = [stdout.trim(), stderr.trim()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let error = classify_wecom_cli_error(&cli_detail).unwrap_or_else(|| BridgeError {
            code: "WECOM_CLI_FAILED".to_owned(),
            message: if cli_detail.is_empty() {
                "企业微信官方CLI执行失败。".to_owned()
            } else {
                format!("企业微信官方CLI执行失败：{cli_detail}")
            },
            recoverable: true,
        });
        return Ok(BridgeEnvelope {
            ok: false,
            data: serde_json::Value::Null,
            warnings: Vec::new(),
            error: Some(error),
            meta: serde_json::json!({
                "platform": "wecom",
                "source": "https://github.com/WecomTeam/wecom-cli",
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }
    let raw: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        serde_json::json!({
            "text": stdout.trim()
        })
    });
    let raw = unwrap_wecom_cli_payload(raw);
    if raw
        .get("errcode")
        .and_then(|value| value.as_i64())
        .is_some_and(|code| code != 0)
    {
        let errmsg = raw
            .get("errmsg")
            .and_then(|value| value.as_str())
            .unwrap_or("企业微信 API 返回错误。");
        let error = classify_wecom_cli_error(errmsg).unwrap_or_else(|| BridgeError {
            code: "WECOM_API_ERROR".to_owned(),
            message: errmsg.to_owned(),
            recoverable: true,
        });
        return Ok(BridgeEnvelope {
            ok: false,
            data: raw.clone(),
            warnings: vec![stderr]
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect(),
            error: Some(error),
            meta: serde_json::json!({
                "platform": "wecom",
                "source": "https://github.com/WecomTeam/wecom-cli",
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }

    let data = match request.command.as_str() {
        "list-contacts" => normalize_wecom_contacts(&raw),
        "list-chats" => normalize_wecom_chats(&raw, &profile.config_json),
        "fetch-messages" => normalize_wecom_messages(&raw, &request.args, &profile.config_json),
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
            "platform": "wecom",
            "source": "https://github.com/WecomTeam/wecom-cli",
            "duration_ms": started_at.elapsed().as_millis()
        }),
    })
}
