use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Instant;

use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::connectors::{self, ConnectorKind};
use crate::security::sanitize_log;
use crate::storage::models::ImProfile;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(windows)]
const WINDOWS_DINGTALK_REGISTRY_KEY: &str = r"HKCU\Software\DwsCli\keychain\dws-cli";
#[cfg(windows)]
const WINDOWS_DINGTALK_AUTH_TOKEN_VALUE: &str = "YXV0aC10b2tlbg";
#[cfg(windows)]
const WINDOWS_DINGTALK_PROFILE_TOKEN_FILE: &str = "windows-auth-token.regvalue";

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

struct BridgeProcessSpec {
    executable: PathBuf,
    prefix_args: Vec<String>,
    script_path: Option<PathBuf>,
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

fn bridge_process_spec(
    platform: &str,
    executable: PathBuf,
    resource_dir: &Path,
) -> BridgeProcessSpec {
    if platform == "wechat" && is_python_bridge_script(&executable) {
        if let Some(python) = resolve_python3(resource_dir) {
            return BridgeProcessSpec {
                executable: python,
                prefix_args: vec![executable.to_string_lossy().to_string()],
                script_path: Some(executable),
            };
        }
        return BridgeProcessSpec {
            executable: executable.clone(),
            prefix_args: Vec::new(),
            script_path: Some(executable),
        };
    }
    BridgeProcessSpec {
        executable,
        prefix_args: Vec::new(),
        script_path: None,
    }
}

fn is_python_bridge_script(path: &Path) -> bool {
    if path.extension().is_some() {
        return false;
    }
    std::fs::read(path)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes.into_iter().take(80).collect()).ok())
        .is_some_and(|head| head.starts_with("#!") && head.contains("python"))
}

fn resolve_python3(resource_dir: &Path) -> Option<PathBuf> {
    python3_path_candidates(resource_dir)
        .into_iter()
        .find(|path| is_usable_python3(path))
}

fn python3_path_candidates(resource_dir: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.extend(bundled_python3_candidates(resource_dir));
    for key in ["PYTHON3", "PYTHON"] {
        if let Some(value) = std::env::var_os(key) {
            candidates.push(PathBuf::from(value));
        }
    }
    if let Some(home) = dirs::home_dir() {
        candidates.extend([
            home.join(".local").join("bin").join("python3"),
            home.join(".pyenv").join("shims").join("python3"),
            home.join(".asdf").join("shims").join("python3"),
        ]);
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/python3"),
        PathBuf::from("/usr/local/bin/python3"),
    ]);
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(
            std::env::split_paths(&path)
                .map(|dir| dir.join("python3"))
                .filter(|path| !is_macos_system_python_shim(path))
                .collect::<Vec<_>>(),
        );
    }
    if !cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/usr/bin/python3"));
    }
    dedupe_existing_paths(&mut candidates);
    candidates
}

fn is_macos_system_python_shim(path: &Path) -> bool {
    cfg!(target_os = "macos") && path == Path::new("/usr/bin/python3")
}

fn bundled_python3_candidates(resource_dir: &Path) -> Vec<PathBuf> {
    let (arch, executable) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("darwin-arm64", ["python", "bin", "python3.10"]),
        ("macos", "x86_64") => ("darwin-x64", ["python", "bin", "python3.10"]),
        ("windows", "x86_64") => ("win-x64", ["python", "python.exe", ""]),
        _ => ("", ["", "", ""]),
    };
    if arch.is_empty() {
        return Vec::new();
    }
    let cwd = std::env::current_dir().ok();
    let roots = [
        Some(resource_dir.join("runtime")),
        Some(resource_dir.join("_up_").join("runtime")),
        Some(resource_dir.join("..").join("runtime")),
        cwd.as_ref().map(|path| path.join("runtime")),
        cwd.as_ref().map(|path| path.join("..").join("runtime")),
    ];
    roots
        .into_iter()
        .flatten()
        .map(|root| {
            let mut path = root
                .join("python")
                .join(PYTHON_STANDALONE_VERSION)
                .join(arch)
                .join(executable[0])
                .join(executable[1]);
            if !executable[2].is_empty() {
                path = path.join(executable[2]);
            }
            path
        })
        .collect()
}

const PYTHON_STANDALONE_VERSION: &str = "20260414";

fn is_usable_python3(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(output) = std::process::Command::new(path)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let version = [
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    ]
    .join(" ");
    version.contains("Python 3.")
}

fn bridge_spawn_error(
    platform: &str,
    process: &BridgeProcessSpec,
    err: std::io::Error,
    duration_ms: u128,
) -> BridgeEnvelope {
    let detail = sanitize_log(&err.to_string());
    let script = process
        .script_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let mut hints = Vec::new();
    hints.push(format!(
        "无法启动 bridge 可执行入口：{}",
        process.executable.to_string_lossy()
    ));
    if platform == "wechat" && script.is_some() && process.prefix_args.is_empty() {
        hints.push("应用内置 Python 运行时缺失或不可用，macOS 微信 bridge 无法运行。请重新安装或使用重新打包后的应用。".to_owned());
    }
    hints.push(detail.clone());
    BridgeEnvelope {
        ok: false,
        data: serde_json::Value::Null,
        warnings: hints,
        error: Some(BridgeError {
            code: "BRIDGE_SPAWN_FAILED".to_owned(),
            message: format!("Bridge进程启动失败：{detail}"),
            recoverable: true,
        }),
        meta: serde_json::json!({
            "platform": platform,
            "executable": process.executable.to_string_lossy(),
            "script": script,
            "duration_ms": duration_ms
        }),
    }
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

async fn run_official_wecom_cli(
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
            "企业微信官方 CLI 尚未准备完成，请重新打开绑定窗口等待准备完成或重新安装 IM-Board。",
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
                &format!("企业微信官方 CLI 暂不支持应用命令：{other}"),
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
                &format!("无法启动企业微信官方 CLI：{err}"),
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
                "企业微信官方 CLI 执行失败。".to_owned()
            } else {
                format!("企业微信官方 CLI 执行失败：{cli_detail}")
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

async fn run_official_feishu_cli(
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
            "飞书官方 CLI 尚未准备完成，请重新打开绑定窗口等待准备完成或重新安装 IM-Board。",
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
                    "fetch-messages 缺少 chat 参数。",
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
                &format!("飞书官方 CLI 暂不支持应用命令：{other}"),
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
                &format!("无法启动飞书官方 CLI：{err}"),
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
            "飞书官方 CLI 执行失败。".to_owned()
        } else {
            format!("飞书官方 CLI 执行失败：{detail}")
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

async fn run_official_dingtalk_cli(
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
            "钉钉官方 CLI 尚未准备完成，请重新打开绑定窗口等待准备完成或重新安装 IM-Board。",
            true,
            started_at,
        ));
    };

    let mut command = official_cli_command(&cli_path);
    apply_official_cli_env(&mut command);
    if cfg!(windows) {
        let dingtalk_home_dir = profile
            .config_json
            .get("homeDir")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(expand_home)
            .or_else(|| {
                profile
                    .config_json
                    .get("configDir")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| expand_home(value).join("home"))
            });
        if let Some(home_dir) = dingtalk_home_dir {
            std::fs::create_dir_all(&home_dir)?;
            command.env("HOME", &home_dir).env("USERPROFILE", &home_dir);
            let appdata_dir = home_dir.join("AppData").join("Roaming");
            let local_appdata_dir = home_dir.join("AppData").join("Local");
            std::fs::create_dir_all(&appdata_dir)?;
            std::fs::create_dir_all(&local_appdata_dir)?;
            command
                .env("APPDATA", appdata_dir)
                .env("LOCALAPPDATA", local_appdata_dir);
        }
    }
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
                    "fetch-messages 缺少 chat 参数。",
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
                &format!("钉钉官方 CLI 暂不支持应用命令：{other}"),
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
                &format!("无法启动钉钉官方 CLI：{err}"),
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
            "钉钉官方 CLI 执行失败。".to_owned()
        } else {
            format!("钉钉官方 CLI 执行失败：{detail}")
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
                    "钉钉官方 CLI 执行失败。".to_owned()
                } else {
                    format!("钉钉官方 CLI 执行失败：{detail}")
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

fn classify_wecom_cli_error(detail: &str) -> Option<BridgeError> {
    if detail.contains("暂不支持授权机器人") && detail.contains("消息") {
        return Some(BridgeError {
            code: "WECOM_MESSAGE_PERMISSION_UNSUPPORTED".to_owned(),
            message: "当前企业或授权机器人暂不支持企业微信「消息」权限；IM 看板需要读取会话列表和聊天记录，因此无法同步企业微信消息。请在企业微信管理后台确认 API 模式智能机器人是否开放消息能力，或更换支持消息权限的企业/机器人后重新绑定。".to_owned(),
            recoverable: true,
        });
    }
    None
}

fn not_authenticated_message(platform_label: &str) -> String {
    format!(
        "{platform_label}尚未完成授权。请复制{platform_label}绑定命令，在{}运行并完成扫码后再测试读取。",
        platform_command_shell_name()
    )
}

fn platform_command_shell_name() -> &'static str {
    if cfg!(windows) {
        "Windows PowerShell"
    } else {
        "macOS终端"
    }
}

fn feishu_app_config_incomplete_message() -> String {
    format!(
        "飞书尚未完成授权。请复制飞书绑定命令，在{}运行，并按提示完成应用配置和用户授权后再测试读取。",
        platform_command_shell_name()
    )
}

fn feishu_user_auth_incomplete_message() -> String {
    format!(
        "授权不完整，飞书存在2次授权（应用配置&用户授权），请留意{}的提示重试。",
        platform_command_shell_name()
    )
}

fn validate_feishu_auth_status(raw: &serde_json::Value) -> Option<BridgeError> {
    let app_configured = raw
        .get("appId")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.trim().is_empty())
        || raw
            .get("brand")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty());
    let verified = raw
        .get("verified")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let token_valid = raw
        .get("tokenStatus")
        .and_then(|value| value.as_str())
        .is_some_and(|value| value == "valid");
    let scope = raw
        .get("scope")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let required_scopes = [
        "search:message",
        "im:chat:read",
        "im:message:readonly",
        "im:message.p2p_msg:get_as_user",
        "im:message.group_msg:get_as_user",
        "contact:user.base:readonly",
        "contact:user.basic_profile:readonly",
    ];
    let has_required_scopes = required_scopes
        .iter()
        .all(|required| scope.split_whitespace().any(|granted| granted == *required));
    if verified && token_valid && has_required_scopes {
        return None;
    }
    Some(BridgeError {
        code: "FEISHU_NOT_AUTHENTICATED".to_owned(),
        message: if app_configured {
            feishu_user_auth_incomplete_message()
        } else {
            feishu_app_config_incomplete_message()
        },
        recoverable: true,
    })
}

fn classify_feishu_cli_error(stdout: &str, stderr: &str) -> Option<BridgeError> {
    let detail = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let raw = serde_json::from_str::<serde_json::Value>(stdout).ok();
    let reason = raw
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("reason"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let error_type = raw
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("type"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let error_code = raw
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("code"))
        .and_then(|value| {
            value
                .as_i64()
                .map(|code| code.to_string())
                .or_else(|| value.as_str().map(str::to_owned))
        })
        .unwrap_or_default();
    let error_message = raw
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("message"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let normalized_detail = format!("{detail}\n{error_message}").to_ascii_lowercase();
    if error_code == "231204"
        || normalized_detail.contains("b2c app not support")
        || normalized_detail.contains("app type is not supported")
    {
        return Some(BridgeError {
            code: "FEISHU_B2C_APP_UNSUPPORTED".to_owned(),
            message: "该会话是飞书应用/机器人会话，飞书官方接口返回 231204（b2c app not support），当前 CLI 不能用用户身份读取这类会话历史；已跳过，不影响其他会话同步。".to_owned(),
            recoverable: true,
        });
    }
    if reason == "not_authenticated"
        || error_message == "not configured"
        || error_type == "config"
        || detail.contains("not_authenticated")
        || detail.contains("not configured")
        || detail.contains("not logged in")
        || detail.contains("未登录")
    {
        return Some(BridgeError {
            code: "FEISHU_NOT_AUTHENTICATED".to_owned(),
            message: feishu_app_config_incomplete_message(),
            recoverable: true,
        });
    }
    None
}

fn parse_feishu_cli_json(stdout: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(stdout.trim())
        .ok()
        .or_else(|| {
            let cleaned = strip_feishu_cli_progress_lines(stdout);
            serde_json::from_str::<serde_json::Value>(cleaned.trim()).ok()
        })
}

fn sanitize_feishu_cli_output(value: &str) -> String {
    strip_feishu_cli_progress_lines(&sanitize_log(value))
}

fn strip_feishu_cli_progress_lines(value: &str) -> String {
    value
        .lines()
        .filter(|line| !is_feishu_cli_progress_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_feishu_cli_progress_line(line: &str) -> bool {
    let Some(rest) = line.trim().strip_prefix("[page ") else {
        return false;
    };
    let Some((page, message)) = rest.split_once(']') else {
        return false;
    };
    let page = page.trim();
    if page.is_empty() || !page.chars().all(|value| value.is_ascii_digit()) {
        return false;
    }
    let message = message.trim().to_ascii_lowercase();
    matches!(message.as_str(), "fetching..." | "fetching") || message.starts_with("fetched ")
}

fn classify_dingtalk_cli_error(stdout: &str, stderr: &str) -> Option<BridgeError> {
    let detail = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let raw = serde_json::from_str::<serde_json::Value>(stdout).ok();
    let error = raw.as_ref().and_then(|value| value.get("error"));
    let reason = raw
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("reason"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let code = error
        .and_then(|value| value.get("code"))
        .or_else(|| raw.as_ref().and_then(|value| value.get("code")));
    let code_text = code
        .and_then(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .or_else(|| value.as_i64().map(|number| number.to_string()))
        })
        .unwrap_or_default();
    let category = error
        .and_then(|value| value.get("category"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let action_url = error
        .and_then(|value| value.get("action_url"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let nested_code = raw
        .as_ref()
        .and_then(|value| value.pointer("/error/data/code"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if reason == "not_authenticated"
        || detail.contains("not_authenticated")
        || detail.contains("未登录")
    {
        return Some(BridgeError {
            code: "DINGTALK_NOT_AUTHENTICATED".to_owned(),
            message: not_authenticated_message("钉钉"),
            recoverable: true,
        });
    }
    if code_text == "PAT_MEDIUM_RISK_NO_PERMISSION"
        || nested_code == "PAT_MEDIUM_RISK_NO_PERMISSION"
        || detail.contains("PAT_MEDIUM_RISK_NO_PERMISSION")
        || detail.contains("chat.message:list")
        || detail.contains("该组织尚未开启 CLI 数据访问权限")
        || detail.contains("TOKEN_VERIFIED_FAILED")
        || (code_text == "1"
            && category == "api"
            && (action_url.contains("developerSettings")
                || action_url.contains("developersSettings")
                || message.contains("developerSettings")
                || message.contains("developersSettings")
                || detail.contains("该组织尚未开启 CLI 数据访问权限")
                || detail.contains("TOKEN_VERIFIED_FAILED")))
    {
        return Some(BridgeError {
            code: "DINGTALK_MESSAGE_PERMISSION_MISSING".to_owned(),
            message: "钉钉当前账号或组织没有开通消息读取权限，可能缺少 chat.message:list 授权，或组织尚未开启 CLI 数据访问权限。请重新授权钉钉官方 CLI，或联系组织主管理员开启后再同步。".to_owned(),
            recoverable: true,
        });
    }
    None
}

fn unwrap_wecom_cli_payload(raw: serde_json::Value) -> serde_json::Value {
    if let Some(error) = raw.get("error") {
        return serde_json::json!({
            "errcode": -1,
            "errmsg": error
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("企业微信官方 CLI 返回错误。")
        });
    }

    let Some(result) = raw.get("result") else {
        return raw;
    };
    let Some(content) = result.get("content").and_then(|value| value.as_array()) else {
        return raw;
    };
    let Some(text) = content
        .iter()
        .find_map(|item| item.get("text").and_then(|value| value.as_str()))
    else {
        return raw;
    };
    serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({ "text": text }))
}

fn bridge_error(
    code: &str,
    message: &str,
    recoverable: bool,
    started_at: Instant,
) -> BridgeEnvelope {
    bridge_error_for(
        "wecom",
        "https://github.com/WecomTeam/wecom-cli",
        code,
        message,
        recoverable,
        started_at,
    )
}

fn bridge_error_for(
    platform: &str,
    source: &str,
    code: &str,
    message: &str,
    recoverable: bool,
    started_at: Instant,
) -> BridgeEnvelope {
    BridgeEnvelope {
        ok: false,
        data: serde_json::Value::Null,
        warnings: Vec::new(),
        error: Some(BridgeError {
            code: code.to_owned(),
            message: message.to_owned(),
            recoverable,
        }),
        meta: serde_json::json!({
            "platform": platform,
            "source": source,
            "duration_ms": started_at.elapsed().as_millis()
        }),
    }
}

fn normalize_feishu_chats(raw: &serde_json::Value) -> serde_json::Value {
    let chats = first_json_array(raw, &["items", "chats", "data"]);
    serde_json::Value::Array(
        chats
            .into_iter()
            .filter_map(|chat| {
                let chat_id = json_string(
                    chat,
                    &["chat_id", "chatId", "chat_id_v2", "id", "open_chat_id"],
                )?;
                let chat_name =
                    json_string(chat, &["name", "chat_name", "chatName", "description"])
                        .unwrap_or_else(|| chat_id.clone());
                let chat_type =
                    json_string(chat, &["chat_type", "chatType", "type"]).unwrap_or_default();
                Some(serde_json::json!({
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "isGroup": !chat_type.eq_ignore_ascii_case("p2p"),
                    "chatType": chat_type,
                    "raw": chat
                }))
            })
            .collect(),
    )
}

fn normalize_feishu_messages(
    raw: &serde_json::Value,
    args: &HashMap<String, String>,
) -> serde_json::Value {
    let chat_id = args.get("chat").cloned().unwrap_or_default();
    let chat_name = args
        .get("chat_name")
        .cloned()
        .unwrap_or_else(|| chat_id.clone());
    let is_group = args
        .get("chat_type")
        .map(|value| value == "2" || value == "group")
        .unwrap_or(true);
    let messages = first_json_array(raw, &["items", "messages", "data"]);
    serde_json::Value::Array(
        messages
            .into_iter()
            .enumerate()
            .filter_map(|(index, message)| {
                let timestamp = feishu_message_timestamp(message)?;
                let msg_type = json_string(message, &["msg_type", "msgType", "message_type", "type"]).unwrap_or_else(|| "text".to_owned());
                let sender_id = message
                    .get("sender")
                    .and_then(|sender| json_string(sender, &["id", "open_id", "user_id", "sender_id"]))
                    .or_else(|| json_string(message, &["sender_id", "senderId"]))
                    .unwrap_or_default();
                let sender_name = message
                    .get("sender")
                    .and_then(|sender| json_string(sender, &["name", "display_name", "displayName"]))
                    .unwrap_or_else(|| sender_id.clone());
                let content = feishu_message_content(message, &msg_type)?;
                Some(serde_json::json!({
                    "timestamp": timestamp,
                    "time": timestamp,
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "isGroup": is_group,
                    "senderId": sender_id,
                    "senderName": sender_name,
                    "content": content,
                    "msgType": msg_type,
                    "localId": json_string(message, &["message_id", "messageId", "id"]).unwrap_or_else(|| format!("{chat_id}_{timestamp}_{index}")),
                    "raw": message
                }))
            })
            .collect(),
    )
}

fn normalize_feishu_message_sessions(raw: &serde_json::Value) -> serde_json::Value {
    let messages = first_json_array(raw, &["items", "messages", "data"]);
    let mut latest_by_chat = HashMap::<String, serde_json::Value>::new();
    for message in messages {
        let Some(chat_id) = feishu_message_chat_id(message) else {
            continue;
        };
        let timestamp = feishu_message_timestamp(message).unwrap_or(0);
        let current_timestamp = latest_by_chat
            .get(&chat_id)
            .and_then(|session| session.get("lastMessageTimestamp"))
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        if timestamp < current_timestamp {
            continue;
        }
        let chat_name = feishu_message_chat_name(message).unwrap_or_else(|| chat_id.clone());
        let chat_type = json_string(message, &["chat_type", "chatType"]).unwrap_or_default();
        latest_by_chat.insert(
            chat_id.clone(),
            serde_json::json!({
                "chatId": chat_id,
                "chatName": chat_name,
                "isGroup": !chat_type.eq_ignore_ascii_case("p2p"),
                "chatType": chat_type,
                "lastMessageTimestamp": timestamp,
                "source": "message_search",
                "raw": message
            }),
        );
    }
    serde_json::Value::Array(latest_by_chat.into_values().collect())
}

fn normalize_dingtalk_chats(raw: &serde_json::Value) -> serde_json::Value {
    let chats = first_json_array(
        raw,
        &["items", "conversations", "value", "list", "data", "result"],
    );
    serde_json::Value::Array(
        chats
            .into_iter()
            .filter_map(|chat| {
                let chat_id = json_string(
                    chat,
                    &[
                        "openConversationId",
                        "openconversation_id",
                        "conversationId",
                        "conversation_id",
                        "chatId",
                        "chat_id",
                        "id",
                    ],
                )?;
                let chat_name = json_string(chat, &["title", "name", "chatName", "chat_name", "conversationName"])
                    .unwrap_or_else(|| chat_id.clone());
                Some(serde_json::json!({
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "isGroup": true,
                    "chatType": json_string(chat, &["type", "conversationType", "conversation_type"]).unwrap_or_else(|| "group".to_owned()),
                    "lastMessageTimestamp": dingtalk_json_timestamp(chat, &["lastMessageTime", "last_message_time", "time", "updatedAt"]),
                    "raw": chat
                }))
            })
            .collect(),
    )
}

fn normalize_dingtalk_messages(
    raw: &serde_json::Value,
    args: &HashMap<String, String>,
) -> serde_json::Value {
    let chat_id = args.get("chat").cloned().unwrap_or_default();
    let chat_name = args
        .get("chat_name")
        .cloned()
        .unwrap_or_else(|| chat_id.clone());
    let messages = first_json_array(
        raw,
        &[
            "items",
            "messages",
            "messageList",
            "conversationMessagesList",
            "list",
            "value",
            "data",
            "result",
        ],
    );
    serde_json::Value::Array(
        messages
            .into_iter()
            .enumerate()
            .filter_map(|(index, message)| {
                let timestamp = dingtalk_json_timestamp(
                    message,
                    &["timestamp", "createTime", "createdAt", "sendTime", "msgCreateTime", "time"],
                )?;
                let msg_type = json_string(message, &["msgType", "msg_type", "messageType", "type"]).unwrap_or_else(|| "text".to_owned());
                let sender_id = json_string(
                    message,
                    &["senderId", "sender_id", "senderStaffId", "sender", "fromUserId", "from"],
                )
                .unwrap_or_default();
                let sender_name = json_string(message, &["senderName", "sender_name", "fromName", "displayName"])
                    .unwrap_or_else(|| sender_id.clone());
                let content = dingtalk_message_content(message, &msg_type)?;
                let normalized_chat_id = json_string(message, &["openConversationId", "conversationId", "chatId", "chat_id"])
                    .unwrap_or_else(|| chat_id.clone());
                let normalized_chat_name = json_string(message, &["conversationTitle", "chatName", "chat_name"])
                    .unwrap_or_else(|| chat_name.clone());
                Some(serde_json::json!({
                    "timestamp": timestamp,
                    "time": timestamp,
                    "chatId": normalized_chat_id,
                    "chatName": normalized_chat_name,
                    "isGroup": dingtalk_is_group_message(message),
                    "senderId": sender_id,
                    "senderName": sender_name,
                    "content": content,
                    "msgType": msg_type,
                    "localId": json_string(message, &["openMessageId", "msgId", "messageId", "id"]).unwrap_or_else(|| format!("{chat_id}_{timestamp}_{index}")),
                    "raw": message
                }))
            })
            .collect(),
    )
}

fn dingtalk_is_group_message(message: &serde_json::Value) -> bool {
    if let Some(value) = message
        .get("isGroup")
        .or_else(|| message.get("is_group"))
        .and_then(|value| value.as_bool())
    {
        return value;
    }
    let kind = json_string(
        message,
        &[
            "conversationType",
            "conversation_type",
            "chatType",
            "chat_type",
            "type",
        ],
    )
    .unwrap_or_default()
    .to_ascii_lowercase();
    !(kind.contains("single") || kind.contains("private") || kind.contains("p2p") || kind == "1")
}

fn dingtalk_message_content(message: &serde_json::Value, msg_type: &str) -> Option<String> {
    if let Some(text) = json_string(message, &["content", "text", "summary", "body"]) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            return json_string(&json, &["text", "content", "title"]).or(Some(text));
        }
        return Some(text);
    }
    message.get(msg_type).and_then(|value| {
        json_string(value, &["text", "content", "title", "name"])
            .or_else(|| Some(format!("[{msg_type}]")))
    })
}

fn dingtalk_json_timestamp(value: &serde_json::Value, keys: &[&str]) -> Option<i64> {
    let raw = json_string(value, keys)?;
    parse_dingtalk_timestamp(&raw)
}

fn feishu_message_chat_id(message: &serde_json::Value) -> Option<String> {
    json_string(
        message,
        &["chat_id", "chatId", "container_id", "containerId"],
    )
    .or_else(|| {
        message
            .get("chat")
            .and_then(|chat| json_string(chat, &["chat_id", "chatId", "id"]))
    })
    .or_else(|| {
        message.get("context").and_then(|context| {
            json_string(
                context,
                &["chat_id", "chatId", "container_id", "containerId"],
            )
        })
    })
}

fn feishu_message_chat_name(message: &serde_json::Value) -> Option<String> {
    json_string(message, &["chat_name", "chatName"])
        .or_else(|| {
            message
                .get("chat")
                .and_then(|chat| json_string(chat, &["name", "chat_name", "chatName"]))
        })
        .or_else(|| {
            message
                .get("context")
                .and_then(|context| json_string(context, &["chat_name", "chatName", "name"]))
        })
}

fn first_json_array<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Vec<&'a serde_json::Value> {
    if let Some(array) = value.as_array() {
        return array.iter().collect();
    }
    for key in keys {
        if let Some(array) = value.get(*key).and_then(|inner| inner.as_array()) {
            return array.iter().collect();
        }
        if let Some(array) = value
            .get("data")
            .and_then(|data| data.get(*key))
            .and_then(|inner| inner.as_array())
        {
            return array.iter().collect();
        }
        if let Some(array) = value
            .get("result")
            .and_then(|result| result.get(*key))
            .and_then(|inner| inner.as_array())
        {
            return array.iter().collect();
        }
    }
    Vec::new()
}

fn normalize_wecom_chats(raw: &serde_json::Value, config: &serde_json::Value) -> serde_json::Value {
    let Some(chats) = raw.get("chats").and_then(|value| value.as_array()) else {
        return serde_json::Value::Array(Vec::new());
    };
    serde_json::Value::Array(
        chats
            .iter()
            .filter_map(|chat| {
                let chat_id = json_string(chat, &["chat_id", "chatid", "chatId", "id"])?;
                let chat_name = json_string(chat, &["chat_name", "chatName", "name"]).unwrap_or_else(|| chat_id.clone());
                Some(serde_json::json!({
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "lastMessageTime": json_string(chat, &["last_msg_time", "lastMessageTime"]),
                    "lastMessageTimestamp": json_string(chat, &["last_msg_time", "lastMessageTime"]).and_then(|value| parse_local_timestamp(&value)),
                    "msgCount": chat.get("msg_count").or_else(|| chat.get("msgCount")).cloned().unwrap_or(serde_json::Value::Number(0.into())),
                    "isGroup": is_wecom_group_chat(&chat_id, &chat_name, config),
                    "raw": chat
                }))
            })
            .collect(),
    )
}

fn normalize_wecom_contacts(raw: &serde_json::Value) -> serde_json::Value {
    let Some(users) = raw.get("userlist").and_then(|value| value.as_array()) else {
        return serde_json::Value::Array(Vec::new());
    };
    serde_json::Value::Array(
        users
            .iter()
            .filter_map(|user| {
                let user_id = json_string(user, &["userid", "userId", "id"])?;
                let name = json_string(user, &["name", "displayName", "alias"])
                    .unwrap_or_else(|| user_id.clone());
                Some(serde_json::json!({
                    "chatId": user_id,
                    "chatName": name,
                    "isGroup": false,
                    "chatType": 1,
                    "source": "contact",
                    "raw": user
                }))
            })
            .collect(),
    )
}

fn normalize_wecom_messages(
    raw: &serde_json::Value,
    args: &HashMap<String, String>,
    config: &serde_json::Value,
) -> serde_json::Value {
    let chat_id = args.get("chat").cloned().unwrap_or_default();
    let chat_name = args
        .get("chat_name")
        .cloned()
        .unwrap_or_else(|| chat_id.clone());
    let is_group = args
        .get("chat_type")
        .map(|value| value == "2" || value == "group")
        .unwrap_or_else(|| is_wecom_group_chat(&chat_id, &chat_name, config));
    let Some(messages) = raw.get("messages").and_then(|value| value.as_array()) else {
        return serde_json::Value::Array(Vec::new());
    };
    serde_json::Value::Array(
        messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                let send_time = json_string(message, &["send_time", "sendTime"])?;
                let timestamp = parse_local_timestamp(&send_time)?;
                let msg_type = json_string(message, &["msgtype", "msgType"]).unwrap_or_else(|| "text".to_owned());
                let sender_id = json_string(message, &["userid", "userId", "sender"]).unwrap_or_else(|| chat_id.clone());
                let content = wecom_message_content(message, &msg_type)?;
                Some(serde_json::json!({
                    "timestamp": timestamp,
                    "time": timestamp,
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "isGroup": is_group,
                    "senderId": sender_id,
                    "senderName": sender_id,
                    "content": content,
                    "msgType": msg_type,
                    "localId": json_string(message, &["msgid", "msg_id", "id"]).unwrap_or_else(|| format!("{chat_id}_{timestamp}_{index}")),
                    "raw": message
                }))
            })
            .collect(),
    )
}

fn wecom_message_content(message: &serde_json::Value, msg_type: &str) -> Option<String> {
    if msg_type == "text" {
        return message
            .get("text")
            .and_then(|value| value.get("content"))
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .or_else(|| json_string(message, &["content", "summary"]));
    }
    if let Some(body) = message.get(msg_type).and_then(|value| value.as_object()) {
        let name = body
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        return Some(if name.is_empty() {
            format!("[{msg_type}]")
        } else {
            format!("[{msg_type}] {name}")
        });
    }
    Some(format!("[{msg_type}]"))
}

fn feishu_message_timestamp(message: &serde_json::Value) -> Option<i64> {
    if let Some(value) = json_string(
        message,
        &[
            "create_time",
            "createTime",
            "update_time",
            "timestamp",
            "time",
            "msgCreateTime",
        ],
    ) {
        return parse_feishu_timestamp(&value);
    }
    None
}

fn feishu_message_content(message: &serde_json::Value, msg_type: &str) -> Option<String> {
    let raw = json_string(message, &["content", "text", "body", "summary"])?;
    if msg_type == "text" || raw.trim_start().starts_with('{') {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
            return json_string(&json, &["text", "content", "title"]).or(Some(raw));
        }
    }
    Some(raw)
}

fn json_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = value.get(*key).and_then(|inner| inner.as_str()) {
            if !text.trim().is_empty() {
                return Some(text.to_owned());
            }
        }
        if let Some(number) = value.get(*key).and_then(|inner| inner.as_i64()) {
            return Some(number.to_string());
        }
    }
    None
}

fn parse_feishu_timestamp(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if let Ok(number) = trimmed.parse::<i64>() {
        return Some(if number > 10_000_000_000 {
            number / 1000
        } else {
            number
        });
    }
    chrono::DateTime::parse_from_rfc3339(trimmed)
        .ok()
        .map(|datetime| datetime.timestamp())
        .or_else(|| parse_local_timestamp(trimmed))
}

fn feishu_time_arg(value: &str) -> Option<String> {
    parse_feishu_timestamp(value).map(|timestamp| {
        Local
            .timestamp_opt(timestamp, 0)
            .single()
            .unwrap_or_else(Local::now)
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string()
    })
}

fn parse_dingtalk_timestamp(value: &str) -> Option<i64> {
    parse_feishu_timestamp(value)
}

fn dingtalk_time_arg(value: &str) -> Option<String> {
    parse_dingtalk_timestamp(value).map(|timestamp| {
        Local
            .timestamp_opt(timestamp, 0)
            .single()
            .unwrap_or_else(Local::now)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    })
}

fn is_wecom_group_chat(chat_id: &str, chat_name: &str, config: &serde_json::Value) -> bool {
    if let Some(value) = config
        .get("chatTypeOverrides")
        .and_then(|overrides| overrides.get(chat_id))
    {
        if value == 2 || value == "2" || value == "group" {
            return true;
        }
        if value == 1 || value == "1" || value == "direct" {
            return false;
        }
    }
    let lowered = chat_id.to_ascii_lowercase();
    lowered.starts_with("wr") || lowered.starts_with("group") || chat_name.contains('群')
}

fn parse_local_timestamp(value: &str) -> Option<i64> {
    ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
        .into_iter()
        .find_map(|format| {
            chrono::NaiveDateTime::parse_from_str(value, format)
                .ok()
                .and_then(|naive| naive.and_local_timezone(Local).single())
                .map(|datetime| datetime.timestamp())
        })
}

fn today_start_text() -> String {
    Local::now().format("%Y-%m-%d 00:00:00").to_string()
}

fn now_text() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn remove_empty_json_fields(value: serde_json::Value) -> serde_json::Value {
    let Some(object) = value.as_object() else {
        return value;
    };
    serde_json::Value::Object(
        object
            .iter()
            .filter(|(_, value)| !value.as_str().is_some_and(str::is_empty))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn register_pid(active_pids: Option<&Mutex<Vec<u32>>>, child_id: Option<u32>) {
    if let (Some(active_pids), Some(child_id)) = (active_pids, child_id) {
        if let Ok(mut pids) = active_pids.lock() {
            pids.push(child_id);
        }
    }
}

fn unregister_pid(active_pids: Option<&Mutex<Vec<u32>>>, child_id: Option<u32>) {
    if let (Some(active_pids), Some(child_id)) = (active_pids, child_id) {
        if let Ok(mut pids) = active_pids.lock() {
            pids.retain(|pid| *pid != child_id);
        }
    }
}

#[cfg(windows)]
struct WindowsWechatProfilePaths {
    config_path: PathBuf,
    keys_path: PathBuf,
    cache_dir: PathBuf,
}

#[cfg(windows)]
async fn run_tracked_output(
    mut command: Command,
    active_pids: Option<&Mutex<Vec<u32>>>,
) -> anyhow::Result<std::process::Output> {
    let child = command.spawn()?;
    let child_id = child.id();
    register_pid(active_pids, child_id);
    let output = child.wait_with_output().await?;
    unregister_pid(active_pids, child_id);
    Ok(output)
}

#[cfg(windows)]
fn resolve_windows_wechat_cli(profile: Option<&ImProfile>, resource_dir: &Path) -> Option<PathBuf> {
    if let Some(path) = resolve_hot_updated_windows_wechat_cli(resource_dir) {
        return Some(path);
    }
    if let Some(configured) = profile
        .and_then(|profile| profile.config_json.get("cliPath"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        if let Some(path) = resolve_windows_command(configured) {
            return Some(path);
        }
    }
    if let Ok(configured) = std::env::var("IMD_WECHAT_QUERY_CLI") {
        if let Some(path) = resolve_windows_command(&configured) {
            return Some(path);
        }
    }
    None
}

#[cfg(windows)]
fn resolve_hot_updated_windows_wechat_cli(resource_dir: &Path) -> Option<PathBuf> {
    official_cli_roots(resource_dir)
        .into_iter()
        .flat_map(|root| {
            let package_root = root.join("wechat").join("node_modules");
            [
                root.join("wechat").join("bin").join("wechat-cli.cmd"),
                package_root.join(".bin").join("wechat-cli.exe"),
                package_root.join(".bin").join("wechat-cli"),
            ]
        })
        .find(|path| {
            path.exists()
                && (is_dependency_free_cli(path) || is_windows_wechat_python_launcher(path))
        })
}

#[cfg(windows)]
fn resolve_windows_command(command: &str) -> Option<PathBuf> {
    let expanded = expand_home(command);
    if expanded.is_absolute() || command.contains('\\') || command.contains('/') {
        return expanded.exists().then_some(expanded);
    }
    let path = std::env::var_os("PATH")?;
    let extensions: &[&str] = if Path::new(command).extension().is_some() {
        &[""]
    } else {
        &["", ".exe", ".cmd", ".bat"]
    };
    std::env::split_paths(&path).find_map(|entry| {
        extensions
            .iter()
            .map(|extension| entry.join(format!("{command}{extension}")))
            .find(|candidate| candidate.exists())
    })
}

#[cfg(windows)]
fn windows_command_for_path(path: &Path) -> Command {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "cmd" || extension == "bat" {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(path);
        hide_windows_console(&mut command);
        command
    } else {
        let mut command = Command::new(path);
        hide_windows_console(&mut command);
        command
    }
}

#[cfg(windows)]
fn windows_wechat_profile_paths(
    profile: &ImProfile,
    cache_root: &Path,
) -> WindowsWechatProfilePaths {
    let app_dir = dirs::data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(APP_DATA_DIR_NAME);
    let profile_dir = profile
        .config_json
        .get("profileDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .unwrap_or_else(|| app_dir.join("Profiles").join(&profile.id));
    let config_path = profile
        .config_json
        .get("configPath")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .unwrap_or_else(|| profile_dir.join("config.json"));
    let keys_path = profile
        .config_json
        .get("keysPath")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .unwrap_or_else(|| profile_dir.join("all_keys.json"));
    let cache_dir = profile
        .config_json
        .get("cacheDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .unwrap_or_else(|| cache_root.join(&profile.id));
    WindowsWechatProfilePaths {
        config_path,
        keys_path,
        cache_dir,
    }
}

#[cfg(windows)]
fn wechat_command_key(command: &str) -> String {
    match command {
        "list-chats" | "sessions" => "listChats".to_owned(),
        "fetch-messages" | "history" | "fts-history" => "fetchMessages".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(windows)]
fn wechat_cli_command(config: &serde_json::Value, command: &str) -> String {
    if let Some(commands) = config
        .get("cliCommands")
        .and_then(|value| value.as_object())
    {
        for key in [
            wechat_command_key(command),
            command.to_owned(),
            command.replace('-', "_"),
        ] {
            if let Some(value) = commands
                .get(&key)
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
            {
                return value.trim().to_owned();
            }
        }
    }
    match wechat_command_key(command).as_str() {
        "listChats" => "sessions".to_owned(),
        "fetchMessages" => "history".to_owned(),
        _ => command.to_owned(),
    }
}

#[cfg(windows)]
fn wechat_cli_arg_name(config: &serde_json::Value, command: &str, arg_name: &str) -> String {
    if let Some(command_args) = config
        .get("cliArgs")
        .and_then(|value| value.as_object())
        .and_then(|arg_maps| {
            [
                wechat_command_key(command),
                command.to_owned(),
                command.replace('-', "_"),
            ]
            .into_iter()
            .find_map(|key| arg_maps.get(&key).and_then(|value| value.as_object()))
        })
    {
        for key in [
            arg_name.to_owned(),
            arg_name.replace('_', "-"),
            camel_name(arg_name),
        ] {
            if let Some(value) = command_args
                .get(&key)
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
            {
                return value.trim().trim_start_matches('-').to_owned();
            }
        }
    }
    arg_name.replace('_', "-")
}

#[cfg(windows)]
fn wechat_cli_arg_placement(config: &serde_json::Value, command: &str, arg_name: &str) -> String {
    if let Some(command_args) = config
        .get("cliArgPlacement")
        .and_then(|value| value.as_object())
        .and_then(|placements| {
            [
                wechat_command_key(command),
                command.to_owned(),
                command.replace('-', "_"),
            ]
            .into_iter()
            .find_map(|key| placements.get(&key).and_then(|value| value.as_object()))
        })
    {
        for key in [arg_name.to_owned(), camel_name(arg_name)] {
            if let Some(value) = command_args
                .get(&key)
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
            {
                return value.trim().to_owned();
            }
        }
    }
    if wechat_command_key(command) == "fetchMessages" && arg_name == "chat" {
        "positional".to_owned()
    } else {
        "option".to_owned()
    }
}

#[cfg(windows)]
fn append_wechat_option(
    command: &mut Command,
    config: &serde_json::Value,
    cli_command: &str,
    arg_name: &str,
    value: &str,
) {
    command
        .arg(format!(
            "--{}",
            wechat_cli_arg_name(config, cli_command, arg_name)
        ))
        .arg(value);
}

#[cfg(windows)]
fn camel_name(name: &str) -> String {
    let mut parts = name.split('_');
    let Some(first) = parts.next() else {
        return String::new();
    };
    let mut output = first.to_owned();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first_char) = chars.next() {
            output.extend(first_char.to_uppercase());
            output.push_str(chars.as_str());
        }
    }
    output
}

fn resolve_bridge_executable(resource_dir: &std::path::Path, request: &BridgeRequest) -> PathBuf {
    if let Some(profile) = &request.profile {
        if let Some(bridge_path) = profile
            .config_json
            .get("bridgePath")
            .and_then(|value| value.as_str())
        {
            let expanded = expand_home(bridge_path);
            if expanded.exists() {
                return expanded;
            }
        }
    }
    let platform = request.platform.as_str();
    let file_name = match platform {
        "wechat" => "bridge_main",
        "wecom" => "wecom-bridge",
        "feishu" => "feishu-bridge",
        "dingtalk" => "dingtalk-bridge",
        _ => "bridge_main",
    };
    if let Ok(root) = std::env::var("IM_BOARD_BRIDGE_ROOT") {
        let configured = PathBuf::from(root);
        if configured.is_absolute() {
            return configured.join(platform).join(file_name);
        }
        let cwd_candidate = std::env::current_dir()
            .unwrap_or_else(|_| resource_dir.to_path_buf())
            .join(&configured)
            .join(platform)
            .join(file_name);
        if cwd_candidate.exists() {
            return cwd_candidate;
        }
        let resource_candidate = resource_dir
            .join("..")
            .join(&configured)
            .join(platform)
            .join(file_name);
        if resource_candidate.exists() {
            return resource_candidate;
        }
        let tauri_parent_resource_candidate = resource_dir
            .join("_up_")
            .join(&configured)
            .join(platform)
            .join(file_name);
        if tauri_parent_resource_candidate.exists() {
            return tauri_parent_resource_candidate;
        }
        return configured.join(platform).join(file_name);
    }

    let candidates = [
        resource_dir.join("bridges").join(platform).join(file_name),
        resource_dir
            .join("_up_")
            .join("bridges")
            .join(platform)
            .join(file_name),
        resource_dir
            .join("..")
            .join("bridges")
            .join(platform)
            .join(file_name),
        std::env::current_dir()
            .unwrap_or_else(|_| resource_dir.to_path_buf())
            .join("bridges")
            .join(platform)
            .join(file_name),
    ];
    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| resource_dir.join("bridges").join(platform).join(file_name))
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn feishu_lark_config_dir(config: &serde_json::Value) -> Option<PathBuf> {
    if let Some(config_dir) = config
        .get("larkConfigDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        return Some(expand_home(config_dir));
    }
    if let Some(home_dir) = config
        .get("homeDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        let legacy_dir = expand_home(home_dir).join(".lark-cli");
        if legacy_dir.join("config.json").exists() {
            return Some(legacy_dir);
        }
    }
    config
        .get("configDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| expand_home(value).join(".lark-cli"))
}

fn resolve_official_cli_for_runtime(
    resource_dir: &Path,
    profile: Option<&ImProfile>,
    platform: &str,
    package: &str,
    bin: &str,
) -> Option<PathBuf> {
    let configured = profile
        .and_then(|profile| profile.config_json.get("cliPath"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty());
    resolve_hot_updated_official_cli(resource_dir, platform, package, bin).or_else(|| {
        configured
            .and_then(resolve_existing_command)
            .filter(|path| is_dependency_free_cli(path))
    })
}

fn resolve_hot_updated_official_cli(
    resource_dir: &Path,
    platform: &str,
    package: &str,
    bin: &str,
) -> Option<PathBuf> {
    official_cli_roots(resource_dir)
        .into_iter()
        .flat_map(|root| official_cli_candidates(&root, platform, package, bin))
        .find(|path| path.exists() && is_dependency_free_cli(path))
}

fn official_cli_roots(resource_dir: &Path) -> Vec<PathBuf> {
    [
        std::env::var("IM_BOARD_OFFICIAL_CLI_ROOT")
            .ok()
            .map(PathBuf::from),
        dirs::data_dir().map(|path| path.join(APP_DATA_DIR_NAME).join("OfficialCli")),
        Some(resource_dir.join("official-cli")),
        Some(resource_dir.join("_up_").join("official-cli")),
        Some(resource_dir.join("..").join("official-cli")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn official_cli_candidates(root: &Path, platform: &str, package: &str, bin: &str) -> Vec<PathBuf> {
    let package_root = root.join(platform).join("node_modules");
    let package_dir = package.split('/').collect::<PathBuf>();
    let mut candidates = Vec::new();
    if cfg!(windows) {
        match platform {
            "wecom" => candidates.push(
                package_root
                    .join("@wecom")
                    .join("cli-win32-x64")
                    .join("bin")
                    .join("wecom-cli.exe"),
            ),
            "feishu" => {
                candidates.push(
                    package_root
                        .join(&package_dir)
                        .join("bin")
                        .join("lark-cli-windows-x64.exe"),
                );
                candidates.push(
                    package_root
                        .join(&package_dir)
                        .join("bin")
                        .join("lark-cli.exe"),
                );
            }
            "dingtalk" => {
                candidates.push(
                    package_root
                        .join(&package_dir)
                        .join("vendor")
                        .join("dws-windows-x64.exe"),
                );
                candidates.push(
                    package_root
                        .join(&package_dir)
                        .join("vendor")
                        .join("dws.exe"),
                );
            }
            _ => {}
        }
        candidates.push(package_root.join(".bin").join(format!("{bin}.exe")));
        return candidates;
    }
    let npm_arch = current_npm_arch();
    match platform {
        "wecom" => candidates.push(
            package_root
                .join(format!("@wecom/cli-darwin-{npm_arch}"))
                .join("bin")
                .join("wecom-cli"),
        ),
        "feishu" => {
            candidates.push(
                package_root
                    .join(&package_dir)
                    .join("bin")
                    .join(format!("lark-cli-darwin-{npm_arch}")),
            );
            candidates.push(package_root.join(&package_dir).join("bin").join("lark-cli"));
        }
        "dingtalk" => {
            candidates.push(
                package_root
                    .join(&package_dir)
                    .join("vendor")
                    .join(format!("dws-darwin-{npm_arch}")),
            );
            candidates.push(package_root.join(&package_dir).join("vendor").join("dws"));
            candidates.push(package_root.join(&package_dir).join("bin").join("dws.js"));
        }
        _ => {}
    }
    candidates.push(package_root.join(".bin").join(bin));
    candidates
}

fn current_npm_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        std::env::consts::ARCH
    }
}

fn resolve_existing_command(command: &str) -> Option<PathBuf> {
    let expanded = expand_home(command);
    if expanded.is_absolute() || command.contains('\\') || command.contains('/') {
        return expanded.exists().then_some(expanded);
    }
    let path = std::env::var_os("PATH")?;
    let extensions: &[&str] = if cfg!(windows) && Path::new(command).extension().is_none() {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(&path).find_map(|entry| {
        extensions
            .iter()
            .map(|extension| entry.join(format!("{command}{extension}")))
            .find(|candidate| candidate.exists())
    })
}

fn is_dependency_free_cli(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "js" | "cmd" | "bat") {
        return false;
    }
    if cfg!(windows) && extension != "exe" {
        return false;
    }
    if let Ok(bytes) = std::fs::read(path) {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(96)]).to_ascii_lowercase();
        if head.contains("/usr/bin/env node") || head.contains("node ") {
            return false;
        }
    }
    true
}

fn is_node_cli_entry(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("js"))
}

#[cfg(windows)]
fn is_windows_wechat_python_launcher(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("wechat-cli.cmd"))
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("bin"))
        && path
            .parent()
            .and_then(|parent| parent.parent())
            .and_then(|parent| parent.file_name())
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("wechat"))
        && windows_wechat_launcher_python_exists(path)
}

#[cfg(windows)]
fn windows_wechat_launcher_python_exists(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    // AppData 中热更新的 Windows 微信启动器会引用应用内置 Python。
    // 如果安装包升级或移动后旧路径失效，不能继续把这个 cmd 当成可用入口。
    text.lines()
        .find_map(extract_quoted_python_path)
        .is_some_and(|python| python.exists())
}

#[cfg(windows)]
fn extract_quoted_python_path(line: &str) -> Option<PathBuf> {
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('"') else {
            return None;
        };
        let candidate = &rest[..end];
        if candidate.to_ascii_lowercase().ends_with("python.exe") {
            return Some(PathBuf::from(candidate));
        }
        rest = &rest[end + 1..];
    }
    None
}

#[cfg(windows)]
fn official_cli_command(path: &Path) -> Command {
    if is_node_cli_entry(path) {
        let mut command = Command::new("node");
        command.arg(path);
        hide_windows_console(&mut command);
        return command;
    }
    windows_command_for_path(path)
}

#[cfg(not(windows))]
fn official_cli_command(path: &Path) -> Command {
    if is_node_cli_entry(path) {
        let mut command = Command::new("node");
        command.arg(path);
        return command;
    }
    Command::new(path)
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

#[cfg(windows)]
fn hide_windows_console(command: &mut Command) {
    // Windows GUI 版同步消息时会频繁启动官方 CLI；隐藏子进程控制台，避免每次拉取会话历史都弹出终端窗口。
    command.creation_flags(CREATE_NO_WINDOW);
}

fn apply_official_cli_env(command: &mut Command) {
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
