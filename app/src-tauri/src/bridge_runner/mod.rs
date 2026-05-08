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
use feishu_runner::run_official_feishu_cli;
use paths::resolve_bridge_executable;
use process::{bridge_process_spec, bridge_spawn_error, BridgeProcessSpec};
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
    if request.platform == "wechat" && request.command == "account-identity" {
        return Ok(read_wechat_account_identity(&request, started_at));
    }
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

    if let Some(error) = validate_wechat_profile_files(&request, started_at) {
        return Ok(error);
    }

    let executable = resolve_bridge_executable(&resource_dir, &request);
    let process = wechat_profile_process_spec(&request)
        .unwrap_or_else(|| bridge_process_spec(&request.platform, executable, &resource_dir));
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
                "应用内置Python运行时缺失或不可用，macOS微信bridge无法运行。请重新安装或使用重新打包后的应用。"
                    .to_owned(),
            ],
            error: Some(BridgeError {
                code: "BRIDGE_PYTHON_MISSING".to_owned(),
                message: "微信Bridge缺少可运行的内置Python运行时。".to_owned(),
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
    hide_windows_console(&mut command);
    command.args(&process.prefix_args);
    if request.platform == "wechat" {
        command.env("PYTHONDONTWRITEBYTECODE", "1");
        command.env("PYTHONUTF8", "1");
        command.env("PYTHONIOENCODING", "utf-8");
        if let Some(profile) = &request.profile {
            if let Some(source_dir) = profile
                .config_json
                .get("wechatCliSourceDir")
                .and_then(|value| value.as_str())
            {
                command.env("PYTHONPATH", source_dir);
            }
        }
    }
    let cli_command = profile_cli_command(&request).unwrap_or_else(|| request.command.clone());
    if request.platform == "wechat" {
        if let Some(profile) = &request.profile {
            if let Some(config_path) = profile
                .config_json
                .get("configPath")
                .and_then(|value| value.as_str())
            {
                command.arg("--config").arg(config_path);
            }
        }
    }
    command.arg(cli_command).arg("--format").arg("json");

    if let Some(profile) = &request.profile {
        if request.platform != "wechat" {
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
        }
        command.env("IMD_PROFILE_CONFIG_JSON", profile.config_json.to_string());
        let profile_cache = cache_dir.join(&profile.id);
        let tmp_dir = profile_cache.join("tmp");
        std::fs::create_dir_all(&tmp_dir)?;
        command.env("TMPDIR", tmp_dir);
    }

    for (key, value) in profile_cli_args(&request) {
        if key == "__positional" {
            command.arg(value);
        } else {
            command.arg(format!("--{key}")).arg(value);
        }
    }
    if let Some(secret) = &request.stdin_secret {
        command.stdin(Stdio::piped());
        command.env("IMD_STDIN_SECRET", "1");
        let mut child = match command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                return Ok(bridge_spawn_error(
                    &request.platform,
                    &process,
                    err,
                    started_at.elapsed().as_millis(),
                ))
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(secret.as_bytes()).await?;
        }
        let output = child.wait_with_output().await?;
        return bridge_output(
            request.platform.as_str(),
            &process.executable,
            output,
            started_at,
        );
    }

    let output = match command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(output) => output,
        Err(err) => {
            return Ok(bridge_spawn_error(
                &request.platform,
                &process,
                err,
                started_at.elapsed().as_millis(),
            ))
        }
    };
    bridge_output(
        request.platform.as_str(),
        &process.executable,
        output,
        started_at,
    )
}

fn bridge_output(
    platform: &str,
    executable: &Path,
    output: std::process::Output,
    started_at: Instant,
) -> anyhow::Result<BridgeEnvelope> {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        let detail = sanitize_log([stdout.as_str(), stderr.as_str()].join("\n").trim());
        if platform == "wechat" {
            if let Some(error) = classify_wechat_bridge_error(&detail) {
                return Ok(BridgeEnvelope {
                    ok: false,
                    data: serde_json::Value::Null,
                    warnings: if detail.is_empty() {
                        Vec::new()
                    } else {
                        vec![detail.clone()]
                    },
                    error: Some(error),
                    meta: serde_json::json!({
                        "platform": platform,
                        "executable": executable.to_string_lossy(),
                        "duration_ms": started_at.elapsed().as_millis()
                    }),
                });
            }
        }
        return Ok(BridgeEnvelope {
            ok: false,
            data: serde_json::Value::Null,
            warnings: if detail.is_empty() {
                Vec::new()
            } else {
                vec![detail.clone()]
            },
            error: Some(BridgeError {
                code: "BRIDGE_CRASHED".to_owned(),
                message: "Bridge进程执行失败".to_owned(),
                recoverable: true,
            }),
            meta: serde_json::json!({
                "platform": platform,
                "executable": executable.to_string_lossy(),
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }
    let data = serde_json::from_str::<serde_json::Value>(&stdout)
        .unwrap_or_else(|_| serde_json::json!({ "stdout": stdout }));
    Ok(BridgeEnvelope {
        ok: true,
        data,
        warnings: if stderr.trim().is_empty() {
            Vec::new()
        } else {
            vec![sanitize_log(&stderr)]
        },
        error: None,
        meta: serde_json::json!({
            "platform": platform,
            "executable": executable.to_string_lossy(),
            "duration_ms": started_at.elapsed().as_millis()
        }),
    })
}

fn wechat_profile_process_spec(request: &BridgeRequest) -> Option<BridgeProcessSpec> {
    if request.platform != "wechat" {
        return None;
    }
    let profile = request.profile.as_ref()?;
    let cli_path = profile
        .config_json
        .get("cliPath")
        .and_then(|value| value.as_str())?;
    let source_dir = profile
        .config_json
        .get("wechatCliSourceDir")
        .and_then(|value| value.as_str())?;
    if cli_path.trim().is_empty() || source_dir.trim().is_empty() {
        return None;
    }
    Some(BridgeProcessSpec {
        executable: PathBuf::from(cli_path),
        prefix_args: vec!["-m".to_owned(), "wechat_cli.main".to_owned()],
        script_path: None,
    })
}

fn validate_wechat_profile_files(
    request: &BridgeRequest,
    started_at: Instant,
) -> Option<BridgeEnvelope> {
    if request.platform != "wechat" || request.command == "init" {
        return None;
    }
    let profile = request.profile.as_ref()?;
    let config_path = profile
        .config_json
        .get("configPath")
        .and_then(|value| value.as_str())
        .map(PathBuf::from)?;
    let keys_path = profile
        .config_json
        .get("keysPath")
        .and_then(|value| value.as_str())
        .map(PathBuf::from)?;

    let config_ready = json_file_has_object_data(&config_path);
    let keys_ready = json_file_has_object_data(&keys_path);
    if config_ready && keys_ready {
        return None;
    }

    let code = if !is_windows_wechat_running() {
        "WECHAT_NOT_RUNNING"
    } else if config_path.exists() != keys_path.exists() {
        "WECHAT_BIND_INCOMPLETE"
    } else if keys_path.exists() {
        "WECHAT_KEYS_EMPTY"
    } else {
        "WECHAT_BIND_NOT_COMPLETED"
    };
    Some(BridgeEnvelope {
        ok: false,
        data: serde_json::Value::Null,
        warnings: vec![format!(
            "configPath={}, keysPath={}",
            config_path.display(),
            keys_path.display()
        )],
        error: Some(BridgeError {
            code: code.to_owned(),
            message: wechat_bind_error_message(code),
            recoverable: true,
        }),
        meta: serde_json::json!({
            "platform": request.platform,
            "command": request.command,
            "duration_ms": started_at.elapsed().as_millis()
        }),
    })
}

fn read_wechat_account_identity(request: &BridgeRequest, started_at: Instant) -> BridgeEnvelope {
    let Some(profile) = request.profile.as_ref() else {
        return wechat_account_identity_error(
            "WECHAT_IDENTITY_PROFILE_MISSING",
            "微信账号配置缺失，无法判断是否重复绑定。",
            started_at,
        );
    };
    let Some(config_path) = profile
        .config_json
        .get("configPath")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    else {
        return wechat_account_identity_error(
            "WECHAT_IDENTITY_CONFIG_MISSING",
            "微信绑定配置文件路径缺失，请重新完成绑定。",
            started_at,
        );
    };
    let config_path = PathBuf::from(config_path);
    let Ok(content) = std::fs::read_to_string(&config_path) else {
        return wechat_account_identity_error(
            "WECHAT_IDENTITY_CONFIG_MISSING",
            "微信绑定配置文件不存在，请重新完成绑定。",
            started_at,
        );
    };
    let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) else {
        return wechat_account_identity_error(
            "WECHAT_IDENTITY_CONFIG_INVALID",
            "微信绑定配置文件无法读取，请重新完成绑定。",
            started_at,
        );
    };
    let db_dir = config
        .get("db_dir")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim();
    let Some(account_dir) = wechat_account_dir_from_db_dir(db_dir) else {
        return wechat_account_identity_error(
            "WECHAT_IDENTITY_DB_DIR_MISSING",
            "微信绑定配置缺少账号数据目录，请重新完成绑定。",
            started_at,
        );
    };
    BridgeEnvelope {
        ok: true,
        data: serde_json::json!({
            "platform": "wechat",
            "tenantId": "",
            "tenantName": "微信",
            "userId": account_dir,
            "userName": profile_remark_or_label(profile),
            "dbDir": db_dir
        }),
        warnings: Vec::new(),
        error: None,
        meta: serde_json::json!({
            "platform": request.platform,
            "command": request.command,
            "configPath": config_path.to_string_lossy(),
            "duration_ms": started_at.elapsed().as_millis()
        }),
    }
}

fn wechat_account_identity_error(code: &str, message: &str, started_at: Instant) -> BridgeEnvelope {
    BridgeEnvelope {
        ok: false,
        data: serde_json::Value::Null,
        warnings: Vec::new(),
        error: Some(BridgeError {
            code: code.to_owned(),
            message: message.to_owned(),
            recoverable: true,
        }),
        meta: serde_json::json!({
            "platform": "wechat",
            "command": "account-identity",
            "duration_ms": started_at.elapsed().as_millis()
        }),
    }
}

fn wechat_account_dir_from_db_dir(db_dir: &str) -> Option<String> {
    let normalized = db_dir.trim().replace('\\', "/");
    let parts = normalized
        .split('/')
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    if let Some(file_name) = parts.last() {
        if file_name.eq_ignore_ascii_case("db_storage") {
            return parts
                .iter()
                .rev()
                .nth(1)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
        }
    }
    Some(db_dir.trim().to_owned()).filter(|value| !value.is_empty())
}

fn profile_remark_or_label(profile: &ImProfile) -> String {
    profile
        .config_json
        .get("remark")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&profile.label)
        .to_owned()
}

fn json_file_has_object_data(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    value
        .as_object()
        .is_some_and(|object| object.values().any(|value| !value.is_null()))
}

fn is_windows_wechat_running() -> bool {
    if !cfg!(windows) {
        return true;
    }
    let Ok(output) = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Weixin.exe", "/FO", "CSV", "/NH"])
        .output()
    else {
        return true;
    };
    String::from_utf8_lossy(&output.stdout)
        .to_ascii_lowercase()
        .contains("weixin.exe")
}

fn wechat_bind_error_message(code: &str) -> String {
    match code {
        "WECHAT_NOT_RUNNING" => "微信未运行，请先打开并登录微信，再重新执行绑定命令。",
        "WECHAT_NOT_LOGGED_IN" => "微信读取不到消息，请确认电脑微信是否已登录后重试。",
        "WECHAT_BIND_INCOMPLETE" => "微信绑定文件不完整，请完成终端里的账号选择和绑定命令。",
        "WECHAT_KEYS_EMPTY" => "微信密钥文件没有可用数据，请登录微信后重新绑定。",
        "WECHAT_DECRYPT_FAILED" => "微信数据库解密失败，请重新运行绑定命令刷新密钥后再同步。",
        "WECHAT_BRIDGE_ENTRY_MISSING" => "微信Bridge入口缺失，请重新准备内置Windows资源。",
        _ => "微信绑定尚未完成，请完成终端里的账号选择和绑定命令。",
    }
    .to_owned()
}

fn classify_wechat_bridge_error(detail: &str) -> Option<BridgeError> {
    let normalized = detail.to_ascii_lowercase();
    let code = if normalized.contains("weixin.exe") || normalized.contains("not running") {
        "WECHAT_NOT_RUNNING"
    } else if normalized.contains("not logged in")
        || detail.contains("未登录")
        || detail.contains("未登陆")
        || detail.contains("无法解密 session.db")
    {
        "WECHAT_NOT_LOGGED_IN"
    } else if detail.contains("密钥文件不存在")
        || detail.contains("请运行: wechat-cli init")
        || detail.contains("未找到微信数据目录")
        || detail.contains("未能自动检测到微信数据目录")
    {
        "WECHAT_BIND_NOT_COMPLETED"
    } else if normalized.contains("file not found")
        || normalized.contains("cannot find")
        || detail.contains("系统找不到指定的路径")
        || detail.contains("ϵͳ")
    {
        "WECHAT_BRIDGE_ENTRY_MISSING"
    } else if normalized.contains("wechat_decrypt_failed")
        || normalized.contains("file is not a database")
        || detail.contains("文件不是数据库")
    {
        "WECHAT_DECRYPT_FAILED"
    } else {
        return None;
    };
    Some(BridgeError {
        code: code.to_owned(),
        message: wechat_bind_error_message(code),
        recoverable: true,
    })
}

fn profile_cli_command(request: &BridgeRequest) -> Option<String> {
    let profile = request.profile.as_ref()?;
    let commands = profile.config_json.get("cliCommands")?.as_object()?;
    let key = command_config_key(&request.command);
    commands.get(&key)?.as_str().map(ToOwned::to_owned)
}

fn profile_cli_args(request: &BridgeRequest) -> Vec<(String, String)> {
    let Some(profile) = request.profile.as_ref() else {
        return request
            .args
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
    };
    let command_key = command_config_key(&request.command);
    let arg_map = profile
        .config_json
        .get("cliArgs")
        .and_then(|value| value.get(&command_key))
        .and_then(|value| value.as_object());
    let placement = profile
        .config_json
        .get("cliArgPlacement")
        .and_then(|value| value.get(&command_key))
        .and_then(|value| value.as_object());
    request
        .args
        .iter()
        .filter_map(|(key, value)| {
            if should_skip_profile_cli_arg(request, &command_key, key) {
                return None;
            }
            let mapped = arg_map
                .and_then(|items| find_config_value(items, key))
                .and_then(|value| value.as_str())
                .unwrap_or(key);
            let place = placement
                .and_then(|items| find_config_value(items, key))
                .and_then(|value| value.as_str());
            let item = if place == Some("positional") {
                ("__positional".to_owned(), value.clone())
            } else {
                (mapped.to_owned(), value.clone())
            };
            Some(item)
        })
        .collect()
}

fn should_skip_profile_cli_arg(request: &BridgeRequest, command_key: &str, key: &str) -> bool {
    if request.platform != "wechat" {
        return false;
    }
    let normalized_key = normalize_config_key(key);
    if matches!(normalized_key.as_str(), "chatname" | "chattype") {
        return true;
    }
    command_key == "listChats" && matches!(normalized_key.as_str(), "starttime" | "endtime")
}

fn find_config_value<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    requested_key: &str,
) -> Option<&'a serde_json::Value> {
    let normalized = normalize_config_key(requested_key);
    object
        .iter()
        .find(|(key, _)| normalize_config_key(key) == normalized)
        .map(|(_, value)| value)
}

fn command_config_key(command: &str) -> String {
    let mut output = String::new();
    let mut uppercase_next = false;
    for ch in command.chars() {
        if ch == '-' || ch == '_' {
            uppercase_next = true;
            continue;
        }
        if uppercase_next {
            output.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            output.push(ch);
        }
    }
    output
}

fn normalize_config_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| *ch != '-' && *ch != '_')
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn official_cli_command(cli_path: &Path) -> Command {
    let extension = cli_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "js" {
        let mut command = Command::new("node");
        command.arg(cli_path);
        command
    } else {
        Command::new(cli_path)
    }
}

fn apply_official_cli_env(command: &mut Command) {
    command.env("PYTHONUTF8", "1");
    command.env("PYTHONIOENCODING", "utf-8");
    hide_windows_console(command);
}

fn register_pid(active_pids: Option<&Mutex<Vec<u32>>>, pid: Option<u32>) {
    let (Some(active_pids), Some(pid)) = (active_pids, pid) else {
        return;
    };
    if let Ok(mut pids) = active_pids.lock() {
        pids.push(pid);
    }
}

fn unregister_pid(active_pids: Option<&Mutex<Vec<u32>>>, pid: Option<u32>) {
    let (Some(active_pids), Some(pid)) = (active_pids, pid) else {
        return;
    };
    if let Ok(mut pids) = active_pids.lock() {
        pids.retain(|item| *item != pid);
    }
}

fn hide_windows_console(command: &mut Command) {
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_profile(platform: &str, config_json: serde_json::Value) -> ImProfile {
        ImProfile {
            id: "test-profile".to_owned(),
            platform: platform.to_owned(),
            label: "test".to_owned(),
            enabled: true,
            config_json,
            status: "normal".to_owned(),
            sort_order: 0,
            created_at: "2026-05-08T00:00:00Z".to_owned(),
            updated_at: "2026-05-08T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn wechat_sessions_drop_unsupported_time_window_args() {
        let profile = test_profile(
            "wechat",
            json!({
                "cliArgs": {
                    "listChats": {
                        "limit": "limit"
                    }
                }
            }),
        );
        let request = BridgeRequest {
            platform: "wechat".to_owned(),
            command: "list-chats".to_owned(),
            profile: Some(profile),
            args: HashMap::from([
                ("limit".to_owned(), "200".to_owned()),
                ("start_time".to_owned(), "2026-05-08 00:00:00".to_owned()),
                ("end_time".to_owned(), "2026-05-08 23:59:59".to_owned()),
            ]),
            stdin_secret: None,
        };

        let args = profile_cli_args(&request);

        assert_eq!(args.len(), 1);
        assert!(args.contains(&("limit".to_owned(), "200".to_owned())));
    }

    #[test]
    fn wechat_sqlite_database_error_is_decrypt_failed() {
        let error = classify_wechat_bridge_error("sqlite3.DatabaseError: file is not a database")
            .expect("wechat decrypt error");

        assert_eq!(error.code, "WECHAT_DECRYPT_FAILED");
        assert_eq!(
            error.message,
            "微信数据库解密失败，请重新运行绑定命令刷新密钥后再同步。"
        );
    }

    #[test]
    fn wechat_session_decrypt_error_guides_login_check() {
        let error = classify_wechat_bridge_error("错误: 无法解密 session.db")
            .expect("wechat login state error");

        assert_eq!(error.code, "WECHAT_NOT_LOGGED_IN");
        assert_eq!(
            error.message,
            "微信读取不到消息，请确认电脑微信是否已登录后重试。"
        );
    }

    #[test]
    fn wechat_missing_config_guides_binding_completion() {
        let error = classify_wechat_bridge_error(
            "FileNotFoundError: 密钥文件不存在: C:\\IMBoard\\all_keys.json\n请运行: wechat-cli init",
        )
        .expect("wechat binding incomplete error");

        assert_eq!(error.code, "WECHAT_BIND_NOT_COMPLETED");
        assert_eq!(
            error.message,
            "微信绑定尚未完成，请完成终端里的账号选择和绑定命令。"
        );
    }

    #[test]
    fn wechat_account_identity_uses_account_directory_from_db_storage() {
        assert_eq!(
            wechat_account_dir_from_db_dir(
                r"C:\Users\chase\Documents\xwechat_files\wxid_example\db_storage"
            ),
            Some("wxid_example".to_owned())
        );
    }

    #[test]
    fn wechat_history_keeps_mapped_time_window_args() {
        let profile = test_profile(
            "wechat",
            json!({
                "cliArgs": {
                    "fetchMessages": {
                        "chat": "chat",
                        "startTime": "start-time",
                        "endTime": "end-time"
                    }
                },
                "cliArgPlacement": {
                    "fetchMessages": {
                        "chat": "positional"
                    }
                }
            }),
        );
        let request = BridgeRequest {
            platform: "wechat".to_owned(),
            command: "fetch-messages".to_owned(),
            profile: Some(profile),
            args: HashMap::from([
                ("chat".to_owned(), "chat-a".to_owned()),
                ("start_time".to_owned(), "2026-05-08 00:00:00".to_owned()),
                ("end_time".to_owned(), "2026-05-08 23:59:59".to_owned()),
            ]),
            stdin_secret: None,
        };

        let args = profile_cli_args(&request);

        assert!(args.contains(&("__positional".to_owned(), "chat-a".to_owned())));
        assert!(args.contains(&("start-time".to_owned(), "2026-05-08 00:00:00".to_owned())));
        assert!(args.contains(&("end-time".to_owned(), "2026-05-08 23:59:59".to_owned())));
    }

    #[test]
    fn wechat_history_drops_sync_metadata_args() {
        let profile = test_profile(
            "wechat",
            json!({
                "cliArgs": {
                    "fetchMessages": {
                        "chat": "chat",
                        "limit": "limit"
                    }
                },
                "cliArgPlacement": {
                    "fetchMessages": {
                        "chat": "positional"
                    }
                }
            }),
        );
        let request = BridgeRequest {
            platform: "wechat".to_owned(),
            command: "fetch-messages".to_owned(),
            profile: Some(profile),
            args: HashMap::from([
                ("chat".to_owned(), "wxid_a".to_owned()),
                ("chat_name".to_owned(), "张三".to_owned()),
                ("chat_type".to_owned(), "1".to_owned()),
                ("limit".to_owned(), "300".to_owned()),
            ]),
            stdin_secret: None,
        };

        let args = profile_cli_args(&request);

        assert!(args.contains(&("__positional".to_owned(), "wxid_a".to_owned())));
        assert!(args.contains(&("limit".to_owned(), "300".to_owned())));
        assert!(!args.iter().any(|(key, _)| key == "chat_name"));
        assert!(!args.iter().any(|(key, _)| key == "chat_type"));
    }
}
