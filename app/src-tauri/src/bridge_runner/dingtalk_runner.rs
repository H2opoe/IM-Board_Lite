use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Instant;

#[cfg(windows)]
use tokio::process::Command;

use crate::security::sanitize_log;
#[cfg(windows)]
use crate::storage::models::ImProfile;

use super::errors::{bridge_error_for, classify_dingtalk_cli_error, not_authenticated_message};
#[cfg(windows)]
use super::hide_windows_console;
use super::normalizers::{
    dingtalk_time_arg, normalize_dingtalk_chats, normalize_dingtalk_messages,
};
use super::paths::{expand_home, resolve_official_cli_for_runtime};
use super::{
    apply_official_cli_env, official_cli_command, wait_for_official_cli_output, BridgeEnvelope,
    BridgeError, BridgeRequest, OFFICIAL_CLI_COMMAND_TIMEOUT,
};

#[cfg(windows)]
const WINDOWS_DINGTALK_REGISTRY_KEY: &str = r"HKCU\Software\DwsCli\keychain\dws-cli";
#[cfg(windows)]
const WINDOWS_DINGTALK_AUTH_TOKEN_VALUE: &str = "YXV0aC10b2tlbg";
#[cfg(windows)]
const WINDOWS_DINGTALK_PROFILE_TOKEN_FILE: &str = "windows-auth-token.regvalue";

pub(super) async fn run_official_dingtalk_cli(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
    active_pids: Option<&Mutex<Vec<u32>>>,
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
    let Some(cli_path) = resolve_official_cli_for_runtime(
        &resource_dir,
        Some(profile),
        "dingtalk",
        "dingtalk-workspace-cli",
        "dws",
    ) else {
        return Ok(bridge_error_for(
            "dingtalk",
            "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
            "DINGTALK_CLI_MISSING",
            "官方CLI尚未准备完成，请重新打开绑定窗口等待准备完成，或重新安装IM-Board。",
            true,
            started_at,
        ));
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
        "list-chats" => {
            command
                .arg("chat")
                .arg("list-top-conversations")
                .arg("--format")
                .arg("json")
                .arg("--limit")
                .arg(
                    request
                        .args
                        .get("limit")
                        .cloned()
                        .unwrap_or_else(|| "200".to_owned()),
                );
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
        "fetch-messages" => {
            let chat_id = request.args.get("chat").cloned().unwrap_or_default();
            if chat_id.trim().is_empty() {
                return Ok(bridge_error_for(
                    "dingtalk",
                    "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
                    "MISSING_CHAT",
                    "fetch-messages缺少chat参数。",
                    true,
                    started_at,
                ));
            }
            command
                .arg("chat")
                .arg("message")
                .arg("list")
                .arg("--format")
                .arg("json")
                .arg("--group")
                .arg(chat_id)
                .arg("--forward")
                .arg(
                    request
                        .args
                        .get("forward")
                        .cloned()
                        .unwrap_or_else(|| "true".to_owned()),
                )
                .arg("--limit")
                .arg(
                    request
                        .args
                        .get("limit")
                        .cloned()
                        .unwrap_or_else(|| "100".to_owned()),
                );
            if let Some(start) = request
                .args
                .get("start_time")
                .and_then(|value| dingtalk_time_arg(value))
            {
                command.arg("--time").arg(start);
            }
        }
        "search-messages" => {
            command
                .arg("chat")
                .arg("message")
                .arg("list-all")
                .arg("--format")
                .arg("json")
                .arg("--cursor")
                .arg(request.args.get("cursor").cloned().unwrap_or_default())
                .arg("--limit")
                .arg(
                    request
                        .args
                        .get("limit")
                        .cloned()
                        .unwrap_or_else(|| "200".to_owned()),
                );
            if let Some(start) = request
                .args
                .get("start_time")
                .and_then(|value| dingtalk_time_arg(value))
            {
                command.arg("--start").arg(start);
            }
            if let Some(end) = request
                .args
                .get("end_time")
                .and_then(|value| dingtalk_time_arg(value))
            {
                command.arg("--end").arg(end);
            }
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
    #[cfg(windows)]
    prepare_windows_dingtalk_token(profile).await?;

    let child = match command.spawn() {
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
        wait_for_official_cli_output(child, active_pids, OFFICIAL_CLI_COMMAND_TIMEOUT).await?;

    let stderr = sanitize_log(&String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let sanitized_stdout = sanitize_log(&stdout);
        let detail = [sanitized_stdout.trim(), stderr.trim()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(error) = classify_dingtalk_cli_error(&sanitized_stdout, &stderr) {
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

    let raw = serde_json::from_str::<serde_json::Value>(&stdout)
        .unwrap_or_else(|_| serde_json::json!({ "text": sanitize_log(&stdout) }));
    if raw.get("error").is_some() {
        let sanitized_stdout = sanitize_log(&stdout);
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
    let data = match request.command.as_str() {
        "get-self" => raw,
        "list-chats" | "search-groups" => normalize_dingtalk_chats(&raw),
        "fetch-messages" => normalize_dingtalk_messages(&raw, &request.args),
        "search-messages" => normalize_dingtalk_messages(&raw, &request.args),
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
            "platform": "dingtalk",
            "source": "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
            "duration_ms": started_at.elapsed().as_millis()
        }),
    })
}

#[cfg(windows)]
fn windows_dingtalk_token_path(profile: &ImProfile) -> PathBuf {
    profile
        .config_json
        .get("dwsKeychainDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .or_else(|| {
            profile
                .config_json
                .get("configDir")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(|value| expand_home(value).join("keychain"))
        })
        .unwrap_or_else(|| PathBuf::from(".").join("dws-keychain"))
        .join(WINDOWS_DINGTALK_PROFILE_TOKEN_FILE)
}

#[cfg(windows)]
async fn prepare_windows_dingtalk_token(profile: &ImProfile) -> anyhow::Result<()> {
    // Windows 版 DWS 当前把 auth-token 固定写入 HKCU 注册表，无法被 DWS_CONFIG_DIR 隔离。
    // 每次运行前由 IM-Board 导入当前 profile 保存的 token；未授权的新 profile 则先清空全局 token，
    // 避免“第二个账号未完成授权”时误读到上一个账号。
    let token_path = windows_dingtalk_token_path(profile);
    if token_path.exists() {
        let token = std::fs::read_to_string(&token_path)?
            .trim_start_matches('\u{feff}')
            .trim()
            .to_owned();
        if !token.is_empty() {
            run_windows_registry_command(
                "add",
                &[
                    WINDOWS_DINGTALK_REGISTRY_KEY,
                    "/v",
                    WINDOWS_DINGTALK_AUTH_TOKEN_VALUE,
                    "/t",
                    "REG_SZ",
                    "/d",
                    &token,
                    "/f",
                ],
            )
            .await?;
            return Ok(());
        }
    }
    run_windows_registry_command(
        "delete",
        &[
            WINDOWS_DINGTALK_REGISTRY_KEY,
            "/v",
            WINDOWS_DINGTALK_AUTH_TOKEN_VALUE,
            "/f",
        ],
    )
    .await
    .or_else(|_| Ok(()))
}

#[cfg(windows)]
async fn run_windows_registry_command(action: &str, args: &[&str]) -> anyhow::Result<()> {
    let mut command = Command::new("reg");
    command.arg(action).args(args);
    hide_windows_console(&mut command);
    let output = command.output().await?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = sanitize_log(&String::from_utf8_lossy(&output.stderr));
    anyhow::bail!("Windows 注册表钉钉授权隔离失败：{stderr}");
}
