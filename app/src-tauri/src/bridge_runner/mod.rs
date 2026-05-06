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
#[cfg(windows)]
mod windows_wechat;
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
use windows_wechat::*;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
            ConnectorKind::WechatOfficial => {
                #[cfg(windows)]
                {
                    return run_windows_original_wechat_cli(
                        request,
                        resource_dir,
                        cache_dir,
                        active_pids,
                        started_at,
                    )
                    .await;
                }
            }
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
            ConnectorKind::WechatLocal => {}
        }
    }
    let executable = resolve_bridge_executable(&resource_dir, &request);
    let process = bridge_process_spec(&request.platform, executable, &resource_dir);
    if request.platform == "wechat"
        && process.script_path.is_some()
        && process.prefix_args.is_empty()
    {
        let script = process
            .script_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());
        return Ok(BridgeEnvelope {
            ok: false,
            data: serde_json::Value::Null,
            warnings: vec![
                "应用内置 Python 运行时缺失或不可用，macOS 微信 bridge 无法运行。请重新安装或使用重新打包后的应用。"
                    .to_owned(),
            ],
            error: Some(BridgeError {
                code: "BRIDGE_PYTHON_MISSING".to_owned(),
                message: "微信 Bridge 缺少可运行的内置 Python 运行时。".to_owned(),
                recoverable: true,
            }),
            meta: serde_json::json!({
                "platform": request.platform,
                "executable": process.executable.to_string_lossy(),
                "script": script,
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }
    let mut command = Command::new(&process.executable);
    command.args(&process.prefix_args);
    if request.platform == "wechat" && process.script_path.is_some() {
        command.env("PYTHONDONTWRITEBYTECODE", "1");
    }

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

// Windows 微信走热更新的原版 wechat-cli；这些实现只在 Windows 构建中参与编译，避免 macOS 检查时产生误导性的 dead_code 警告。
#[cfg(windows)]
const ORIGINAL_WECHAT_CLI_SOURCE: &str = "https://github.com/huohuoer/wechat-cli";

const APP_DATA_DIR_NAME: &str = "IMBoard";

#[cfg(windows)]
async fn run_windows_original_wechat_cli(
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

pub(super) fn register_pid(active_pids: Option<&Mutex<Vec<u32>>>, child_id: Option<u32>) {
    if let (Some(active_pids), Some(child_id)) = (active_pids, child_id) {
        if let Ok(mut pids) = active_pids.lock() {
            pids.push(child_id);
        }
    }
}

pub(super) fn unregister_pid(active_pids: Option<&Mutex<Vec<u32>>>, child_id: Option<u32>) {
    if let (Some(active_pids), Some(child_id)) = (active_pids, child_id) {
        if let Ok(mut pids) = active_pids.lock() {
            pids.retain(|pid| *pid != child_id);
        }
    }
}

fn is_node_cli_entry(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("js"))
}

#[cfg(windows)]
pub(super) fn official_cli_command(path: &Path) -> Command {
    if is_node_cli_entry(path) {
        let mut command = Command::new("node");
        command.arg(path);
        hide_windows_console(&mut command);
        return command;
    }
    windows_command_for_path(path)
}

#[cfg(not(windows))]
pub(super) fn official_cli_command(path: &Path) -> Command {
    if is_node_cli_entry(path) {
        let mut command = Command::new("node");
        command.arg(path);
        return command;
    }
    Command::new(path)
}

#[cfg(windows)]
pub(super) fn hide_windows_console(command: &mut Command) {
    // Windows GUI 版同步消息时会频繁启动官方 CLI；隐藏子进程控制台，避免每次拉取会话历史都弹出终端窗口。
    command.creation_flags(CREATE_NO_WINDOW);
}

pub(super) fn apply_official_cli_env(command: &mut Command) {
    if let Some(path) = official_cli_path() {
        command.env("PATH", path);
    }
}

fn official_cli_path() -> Option<std::ffi::OsString> {
    let mut entries = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&path));
    }
    entries.extend(node_path_candidates());
    dedupe_existing_paths(&mut entries);
    std::env::join_paths(entries).ok()
}

fn node_path_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    if cfg!(windows) {
        for key in [
            "ProgramFiles",
            "ProgramFiles(x86)",
            "LOCALAPPDATA",
            "APPDATA",
        ] {
            if let Some(root) = std::env::var_os(key) {
                let root = PathBuf::from(root);
                candidates.push(root.join("nodejs"));
                candidates.push(root.join("Programs").join("nodejs"));
                candidates.push(root.join("npm"));
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        collect_child_bin_dirs(
            &home.join(".nvm").join("versions").join("node"),
            &mut candidates,
        );
        collect_fnm_node_dirs(&home.join(".fnm").join("node-versions"), &mut candidates);
        collect_fnm_node_dirs(
            &home
                .join(".local")
                .join("share")
                .join("fnm")
                .join("node-versions"),
            &mut candidates,
        );
        candidates.push(home.join(".volta").join("bin"));
        candidates.push(home.join(".local").join("bin"));
    }
    candidates
}

fn collect_child_bin_dirs(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        output.push(entry.path().join("bin"));
    }
}

fn collect_fnm_node_dirs(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        output.push(entry.path().join("installation").join("bin"));
    }
}

fn dedupe_existing_paths(entries: &mut Vec<PathBuf>) {
    let mut seen = Vec::<PathBuf>::new();
    entries.retain(|entry| {
        if !entry.exists() || seen.iter().any(|item| item == entry) {
            return false;
        }
        seen.push(entry.clone());
        true
    });
}

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
            "friendly_hint": "该组织尚未开启 CLI 数据访问权限，请联系组织主管理员开启。",
            "message": "business error: success=false",
            "reason": "business_error",
            "server_error_code": "TOKEN_VERIFIED_FAILED",
            "server_key": "group-chat"
          }
        }"#;

        let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
        assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
        assert!(error.recoverable);
        assert!(error.message.contains("CLI 数据访问权限"));
    }

    #[test]
    fn classifies_sanitized_dingtalk_permission_error() {
        let stdout = r#"{
          "error": {
            "action_url": "https://open-dev.dingtalk.com/fe/old#/developerSettings",
            "category": "api",
            "code": 1,
            "friendly_hint": "该组织尚未开启 CLI 数据访问权限，请联系组织主管理员开启。",
            "message": "business error: success=false",
token=***
            "reason": "business_error",
            "server_key": "group-chat"
          }
        }"#;

        let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
        assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
        assert!(error.recoverable);
        assert!(error.message.contains("CLI 数据访问权限"));
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
