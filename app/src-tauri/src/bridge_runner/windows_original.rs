use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Instant;

use super::errors::bridge_error_for;
use super::paths::expand_home;
use super::windows_wechat::{
    append_wechat_option, resolve_windows_wechat_cli, run_tracked_output, wechat_cli_arg_placement,
    wechat_cli_command, wechat_command_key, windows_wechat_profile_paths,
};
use super::{apply_official_cli_env, official_cli_command, BridgeEnvelope, BridgeError, BridgeRequest};
use crate::security::sanitize_log;

#[cfg(windows)]
const ORIGINAL_WECHAT_CLI_SOURCE: &str = "https://github.com/huohuoer/wechat-cli";

const APP_DATA_DIR_NAME: &str = "IMBoard";

#[cfg(windows)]
pub(super) async fn run_windows_original_wechat_cli(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
    active_pids: Option<&Mutex<Vec<u32>>>,
    started_at: Instant,
) -> anyhow::Result<BridgeEnvelope> {
    match request.command.as_str() {
        "discover" => discover_windows_original_wechat_cli(&resource_dir, started_at).await,
        "init-profile" => {
            init_windows_original_wechat_cli(
                request,
                &resource_dir,
                cache_dir,
                active_pids,
                started_at,
            )
            .await
        }
        "list-chats" | "fetch-messages" | "sessions" | "history" | "fts-history" | "search"
        | "stats" => {
            query_windows_original_wechat_cli(
                request,
                &resource_dir,
                cache_dir,
                active_pids,
                started_at,
            )
            .await
        }
        other => Ok(bridge_error_for(
            "wechat",
            ORIGINAL_WECHAT_CLI_SOURCE,
            "WECHAT_UNSUPPORTED_COMMAND",
            &format!("原版 wechat-cli 暂不支持应用命令：{other}"),
            true,
            started_at,
        )),
    }
}

#[cfg(windows)]
async fn discover_windows_original_wechat_cli(
    resource_dir: &Path,
    started_at: Instant,
) -> anyhow::Result<BridgeEnvelope> {
    let Some(cli_path) = resolve_windows_wechat_cli(None, resource_dir) else {
        return Ok(bridge_error_for(
            "wechat",
            ORIGINAL_WECHAT_CLI_SOURCE,
            "WECHAT_ORIGINAL_CLI_MISSING",
            "原版 wechat-cli 尚未完成热更新，请重新打开绑定窗口等待准备完成。",
            true,
            started_at,
        ));
    };

    let mut command = official_cli_command(&cli_path);
    apply_official_cli_env(&mut command);
    command
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = match command.output().await {
        Ok(output) => output,
        Err(err) => {
            return Ok(bridge_error_for(
                "wechat",
                ORIGINAL_WECHAT_CLI_SOURCE,
                "WECHAT_ORIGINAL_CLI_MISSING",
                &format!("无法启动原版 wechat-cli：{err}"),
                true,
                started_at,
            ));
        }
    };
    if !output.status.success() {
        let detail = sanitize_log(&format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
        return Ok(bridge_error_for(
            "wechat",
            ORIGINAL_WECHAT_CLI_SOURCE,
            "WECHAT_ORIGINAL_CLI_FAILED",
            &format!("原版 wechat-cli 自检失败：{}", detail.trim()),
            true,
            started_at,
        ));
    }
    let cli_version = sanitize_log(&format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
    .lines()
    .map(str::trim)
    .find(|line| !line.is_empty())
    .unwrap_or("已配置版本")
    .to_owned();

    let profile_id = "wechat_windows_original";
    let app_dir = dirs::data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(APP_DATA_DIR_NAME);
    let profile_dir = app_dir.join("Profiles").join(profile_id);
    let profile_cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| app_dir.join("Caches"))
        .join(APP_DATA_DIR_NAME)
        .join(profile_id);
    let data_dir = dirs::document_dir()
        .map(|path| path.join("WeChat Files"))
        .unwrap_or_default();

    Ok(BridgeEnvelope {
        ok: true,
        data: serde_json::json!([{
            "id": profile_id,
            "platform": "wechat",
            "label": "原版微信",
            "pid": 0,
            "relatedPids": [],
            "bundleId": "wechat-cli-original",
            "containerId": "wechat-cli-original",
            "appPath": cli_path.to_string_lossy(),
            "cliPath": cli_path.to_string_lossy(),
            "cliVersion": cli_version,
            "runtime": "windows_original_cli",
            "setupMode": "windows_original_cli",
            "requiresPassword": false,
            "dataDir": data_dir.to_string_lossy(),
            "wechatFilesPath": data_dir.to_string_lossy(),
            "dbDir": "",
            "candidateDbDirs": [],
            "running": true,
            "confidence": "cli",
            "profileDir": profile_dir.to_string_lossy(),
            "configPath": profile_dir.join("config.json").to_string_lossy(),
            "keysPath": profile_dir.join("all_keys.json").to_string_lossy(),
            "cacheDir": profile_cache_dir.to_string_lossy()
        }]),
        warnings: Vec::new(),
        error: None,
        meta: serde_json::json!({
            "platform": "wechat",
            "runtime": "windows_original_cli",
            "source": ORIGINAL_WECHAT_CLI_SOURCE,
            "duration_ms": started_at.elapsed().as_millis()
        }),
    })
}

#[cfg(windows)]
async fn init_windows_original_wechat_cli(
    request: BridgeRequest,
    resource_dir: &Path,
    cache_dir: PathBuf,
    active_pids: Option<&Mutex<Vec<u32>>>,
    started_at: Instant,
) -> anyhow::Result<BridgeEnvelope> {
    let Some(profile) = &request.profile else {
        return Ok(bridge_error_for(
            "wechat",
            ORIGINAL_WECHAT_CLI_SOURCE,
            "WECHAT_PROFILE_NOT_FOUND",
            "缺少微信账号配置。",
            true,
            started_at,
        ));
    };
    let Some(cli_path) = resolve_windows_wechat_cli(Some(profile), resource_dir) else {
        return Ok(bridge_error_for(
            "wechat",
            ORIGINAL_WECHAT_CLI_SOURCE,
            "WECHAT_ORIGINAL_CLI_MISSING",
            "原版 wechat-cli 尚未完成热更新，请重新打开绑定窗口等待准备完成。",
            true,
            started_at,
        ));
    };
    let paths = windows_wechat_profile_paths(profile, &cache_dir);
    if let Some(parent) = paths.config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = paths.keys_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(paths.cache_dir.join("tmp"))?;

    let mut command = official_cli_command(&cli_path);
    apply_official_cli_env(&mut command);
    command
        .arg("init")
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--keys-file")
        .arg(&paths.keys_path);
    if let Some(db_dir) = request
        .args
        .get("db_dir")
        .map(String::as_str)
        .or_else(|| {
            profile
                .config_json
                .get("dbDir")
                .and_then(|value| value.as_str())
        })
        .filter(|value| !value.trim().is_empty())
    {
        command.arg("--db-dir").arg(expand_home(db_dir));
    }
    command
        .arg("--force")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.env("TMPDIR", paths.cache_dir.join("tmp"));

    let output = run_tracked_output(command, active_pids).await?;
    let stderr = sanitize_log(&String::from_utf8_lossy(&output.stderr));
    let stdout = sanitize_log(&String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        let detail = [stdout.trim(), stderr.trim()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(BridgeEnvelope {
            ok: false,
            data: serde_json::Value::Null,
            warnings: if detail.is_empty() {
                Vec::new()
            } else {
                vec![detail]
            },
            error: Some(BridgeError {
                code: "WECHAT_ORIGINAL_CLI_INIT_FAILED".to_owned(),
                message: "原版 wechat-cli 初始化失败。".to_owned(),
                recoverable: true,
            }),
            meta: serde_json::json!({
                "platform": "wechat",
                "runtime": "windows_original_cli",
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }

    Ok(BridgeEnvelope {
        ok: true,
        data: serde_json::json!({
            "success": true,
            "profileId": profile.id,
            "cliPath": cli_path.to_string_lossy(),
            "configPath": paths.config_path.to_string_lossy(),
            "keysPath": paths.keys_path.to_string_lossy(),
            "cacheDir": paths.cache_dir.to_string_lossy()
        }),
        warnings: [stderr.trim(), stdout.trim()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect(),
        error: None,
        meta: serde_json::json!({
            "platform": "wechat",
            "runtime": "windows_original_cli",
            "duration_ms": started_at.elapsed().as_millis()
        }),
    })
}

#[cfg(windows)]
async fn query_windows_original_wechat_cli(
    request: BridgeRequest,
    resource_dir: &Path,
    cache_dir: PathBuf,
    active_pids: Option<&Mutex<Vec<u32>>>,
    started_at: Instant,
) -> anyhow::Result<BridgeEnvelope> {
    let Some(profile) = &request.profile else {
        return Ok(bridge_error_for(
            "wechat",
            ORIGINAL_WECHAT_CLI_SOURCE,
            "WECHAT_PROFILE_NOT_FOUND",
            "缺少微信账号配置。",
            true,
            started_at,
        ));
    };
    let Some(cli_path) = resolve_windows_wechat_cli(Some(profile), resource_dir) else {
        return Ok(bridge_error_for(
            "wechat",
            ORIGINAL_WECHAT_CLI_SOURCE,
            "WECHAT_ORIGINAL_CLI_MISSING",
            "原版 wechat-cli 尚未完成热更新，请重新打开绑定窗口等待准备完成。",
            true,
            started_at,
        ));
    };
    let paths = windows_wechat_profile_paths(profile, &cache_dir);
    let command_key = wechat_command_key(&request.command);
    let actual_command = wechat_cli_command(&profile.config_json, &request.command);
    let mut command = official_cli_command(&cli_path);
    apply_official_cli_env(&mut command);
    command.arg("--config").arg(&paths.config_path);
    for part in actual_command.split_whitespace() {
        command.arg(part);
    }
    match command_key.as_str() {
        "listChats" => {
            if let Some(limit) = request.args.get("limit") {
                append_wechat_option(
                    &mut command,
                    &profile.config_json,
                    &request.command,
                    "limit",
                    limit,
                );
            }
            command.arg("--format").arg("json");
        }
        "fetchMessages" => {
            let chat = request.args.get("chat").cloned().unwrap_or_default();
            if chat.trim().is_empty() {
                return Ok(bridge_error_for(
                    "wechat",
                    ORIGINAL_WECHAT_CLI_SOURCE,
                    "MISSING_CHAT",
                    "fetch-messages 缺少 chat 参数。",
                    true,
                    started_at,
                ));
            }
            if wechat_cli_arg_placement(&profile.config_json, &request.command, "chat")
                == "positional"
            {
                command.arg(chat);
            } else {
                append_wechat_option(
                    &mut command,
                    &profile.config_json,
                    &request.command,
                    "chat",
                    &chat,
                );
            }
            for key in ["limit", "offset", "start_time", "end_time"] {
                if let Some(value) = request.args.get(key) {
                    append_wechat_option(
                        &mut command,
                        &profile.config_json,
                        &request.command,
                        key,
                        value,
                    );
                }
            }
            command.arg("--format").arg("json");
        }
        "search" => {
            let query = request.args.get("query").cloned().unwrap_or_default();
            if query.trim().is_empty() {
                return Ok(bridge_error_for(
                    "wechat",
                    ORIGINAL_WECHAT_CLI_SOURCE,
                    "MISSING_QUERY",
                    "search 缺少 query 参数。",
                    true,
                    started_at,
                ));
            }
            command.arg(query);
            if let Some(chat) = request.args.get("chat") {
                command.arg("--chat").arg(chat);
            }
            if let Some(limit) = request.args.get("limit") {
                command.arg("--limit").arg(limit);
            }
            command.arg("--format").arg("json");
        }
        "stats" => {
            if let Some(chat) = request.args.get("chat") {
                command.arg(chat);
            }
            command.arg("--format").arg("json");
        }
        _ => {
            return Ok(bridge_error_for(
                "wechat",
                ORIGINAL_WECHAT_CLI_SOURCE,
                "WECHAT_UNSUPPORTED_COMMAND",
                &format!("原版 wechat-cli 暂不支持应用命令：{}", request.command),
                true,
                started_at,
            ));
        }
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command.env("TMPDIR", paths.cache_dir.join("tmp"));

    let output = run_tracked_output(command, active_pids).await?;
    let stderr = sanitize_log(&String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Ok(BridgeEnvelope {
            ok: false,
            data: serde_json::Value::Null,
            warnings: vec![stderr],
            error: Some(BridgeError {
                code: "WECHAT_ORIGINAL_CLI_FAILED".to_owned(),
                message: "原版 wechat-cli 执行失败。".to_owned(),
                recoverable: true,
            }),
            meta: serde_json::json!({
                "platform": "wechat",
                "runtime": "windows_original_cli",
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }
    let data = serde_json::from_str(&stdout)
        .unwrap_or_else(|_| serde_json::json!({ "text": stdout.trim() }));
    Ok(BridgeEnvelope {
        ok: true,
        data,
        warnings: if stderr.is_empty() {
            Vec::new()
        } else {
            vec![stderr]
        },
        error: None,
        meta: serde_json::json!({
            "platform": "wechat",
            "runtime": "windows_original_cli",
            "command": request.command,
            "duration_ms": started_at.elapsed().as_millis()
        }),
    })
}
