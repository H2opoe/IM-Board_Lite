use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::Ordering;

use futures::StreamExt;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::process::Command;
use tokio::time::{interval, sleep, Duration, Instant};

use crate::ai;
use crate::storage::models::{AiConfig, LocalModelDownloadProgress, LocalModelStatus};
use crate::storage::AppState;

const LOCAL_DEEPSEEK_FILE_NAME: &str = "DeepSeek-R1-Distill-Qwen-7B-Q4_K_M.gguf";
const LOCAL_DEEPSEEK_SOURCE_URL: &str = "https://hf-mirror.com/bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF/resolve/main/DeepSeek-R1-Distill-Qwen-7B-Q4_K_M.gguf";
const LOCAL_DEEPSEEK_FALLBACK_SOURCE_URL: &str = "https://huggingface.co/bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF/resolve/main/DeepSeek-R1-Distill-Qwen-7B-Q4_K_M.gguf";
const LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES: i64 = 4_683_073_504;
const LOCAL_DEEPSEEK_BASE_URL: &str = "http://127.0.0.1:11434/v1";
const LOCAL_LLAMA_CPP_VERSION: &str = "b8987";
const LOCAL_MODEL_PROGRESS_EVENT: &str = "local-model-download-progress";
const LOCAL_MODEL_PROGRESS_EMIT_STEP_BYTES: i64 = 8 * 1024 * 1024;

#[tauri::command]
pub fn get_ai_config(state: State<'_, AppState>) -> Result<AiConfig, String> {
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    ai::get_config(&conn).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn save_ai_config(state: State<'_, AppState>, config: AiConfig) -> Result<AiConfig, String> {
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    ai::save_config(&conn, config).map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn test_ai_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    config: AiConfig,
) -> Result<String, String> {
    if is_managed_local_deepseek_config(&config) {
        if !local_deepseek_status(&state).installed {
            start_local_deepseek_download(&app, &state)?;
            return Ok("downloading".to_owned());
        }
        if config.enabled {
            ensure_local_deepseek_runtime(&app, &state).await?;
        }
    }
    if !config.enabled {
        return Ok("disabled".to_owned());
    }
    if config.model.trim().is_empty() {
        return Err("模型不能为空".to_owned());
    }
    if !ai::is_local_provider(&config) && config.api_key.trim().is_empty() {
        return Err("API Key不能为空".to_owned());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|err| format!("构建 API 请求客户端失败：{err}"))?;
    let base_url = config.base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err("Base URL不能为空".to_owned());
    }

    let response = if config.provider == "Claude" || base_url.contains("anthropic.com") {
        client
            .post(format!("{base_url}/messages"))
            .header("x-api-key", config.api_key.trim())
            .header("anthropic-version", "2023-06-01")
            .json(&serde_json::json!({
                "model": config.model,
                "max_tokens": 1,
                "messages": [{ "role": "user", "content": "ping" }]
            }))
            .send()
            .await
            .map_err(|err| ai::describe_request_error("API请求未发出", err))?
    } else {
        let request = client.post(format!("{base_url}/chat/completions"));
        let request = if config.api_key.trim().is_empty() {
            request
        } else {
            request.bearer_auth(config.api_key.trim())
        };
        let request = ai::with_provider_headers(request, &config, base_url);
        request
            .json(&serde_json::json!({
                "model": config.model,
                "messages": [{ "role": "user", "content": "ping" }],
                "max_tokens": 1,
                "temperature": 0
            }))
            .send()
            .await
            .map_err(|err| ai::describe_request_error("API请求未发出", err))?
    };

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let detail = if body.trim().is_empty() {
            status.to_string()
        } else {
            body.chars().take(500).collect()
        };
        return Err(format!("API返回错误：{status}：{detail}"));
    }

    Ok("ready".to_owned())
}

#[tauri::command]
pub fn get_local_deepseek_status(state: State<'_, AppState>) -> Result<LocalModelStatus, String> {
    Ok(local_deepseek_status(&state))
}

#[tauri::command]
pub fn get_local_deepseek_download_progress(
    state: State<'_, AppState>,
) -> Result<Option<LocalModelDownloadProgress>, String> {
    state
        .local_model_download_progress
        .lock()
        .map(|progress| progress.clone())
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn cancel_local_deepseek_download(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LocalModelStatus, String> {
    state
        .local_model_download_cancel_requested
        .store(true, Ordering::SeqCst);

    let partial_path = local_deepseek_partial_path(&state.app_dir);
    let _ = std::fs::remove_file(&partial_path);

    let progress = current_local_deepseek_progress(&state)?;
    let (downloaded_bytes, total_bytes) = progress
        .map(|progress| (progress.downloaded_bytes, progress.total_bytes))
        .unwrap_or((0, LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES));
    emit_local_deepseek_progress(&app, "cancelled", downloaded_bytes, total_bytes);

    Ok(local_deepseek_status(&state))
}

#[tauri::command]
pub async fn install_local_deepseek_model(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LocalModelStatus, String> {
    let installed = local_deepseek_status(&state);
    if installed.installed {
        ensure_local_deepseek_runtime(&app, &state).await?;
        emit_local_deepseek_progress(
            &app,
            "done",
            installed.size_bytes.max(installed.expected_size_bytes),
            installed.expected_size_bytes,
        );
        return Ok(local_deepseek_status(&state));
    }
    start_local_deepseek_download(&app, &state)?;
    Ok(local_deepseek_status(&state))
}

#[tauri::command]
pub fn clear_local_deepseek_model(state: State<'_, AppState>) -> Result<LocalModelStatus, String> {
    stop_tracked_llama_server(&state)?;
    let models_dir = state.app_dir.join("Models");
    let model_path = models_dir.join(LOCAL_DEEPSEEK_FILE_NAME);
    let partial_path = model_path.with_extension("gguf.part");
    remove_file_if_exists(&model_path, "本地DeepSeek模型文件")?;
    remove_file_if_exists(&partial_path, "本地DeepSeek模型临时文件")?;

    let runtime_dir = state.app_dir.join("Runtime").join("llama.cpp");
    remove_dir_if_exists(&runtime_dir, "本地DeepSeek运行时部署目录")?;

    if let Ok(mut progress) = state.local_model_download_progress.lock() {
        *progress = None;
    }

    let conn = state.db.lock().map_err(|err| err.to_string())?;
    let config = ai::get_config(&conn).map_err(|err| err.to_string())?;
    if is_managed_local_deepseek_config(&config) {
        let mut disabled = config;
        disabled.enabled = false;
        disabled.test_status = "untested".to_owned();
        ai::save_config(&conn, disabled).map_err(|err| err.to_string())?;
    }

    Ok(local_deepseek_status(&state))
}

fn start_local_deepseek_download(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let installed = local_deepseek_status(state);
    if installed.installed {
        emit_local_deepseek_progress(
            app,
            "done",
            installed.size_bytes.max(installed.expected_size_bytes),
            installed.expected_size_bytes,
        );
        return Ok(());
    }
    if let Some(progress) = current_local_deepseek_progress(state)? {
        if is_local_model_download_active(&progress) {
            let _ = app.emit(LOCAL_MODEL_PROGRESS_EVENT, progress);
            return Ok(());
        }
    }

    let app_dir = state.app_dir.clone();
    let app_for_task = app.clone();
    state
        .local_model_download_cancel_requested
        .store(false, Ordering::SeqCst);
    emit_local_deepseek_progress(app, "starting", 0, LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES);
    tauri::async_runtime::spawn(async move {
        if let Err(err) = download_local_deepseek_model(app_for_task.clone(), app_dir).await {
            eprintln!("本地 DeepSeek 模型下载失败：{err}");
        }
    });
    Ok(())
}

fn stop_tracked_llama_server(state: &AppState) -> Result<(), String> {
    let pid = state
        .local_model_server_pid
        .lock()
        .map_err(|err| err.to_string())?
        .take();
    let Some(pid) = pid else {
        return Ok(());
    };

    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path, label: &str) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_file(path).map_err(|err| format!("删除{label}失败：{err}"))?;
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path, label: &str) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|err| format!("删除{label}失败：{err}"))?;
    }
    Ok(())
}

async fn download_local_deepseek_model(app: AppHandle, app_dir: PathBuf) -> Result<(), String> {
    let models_dir = app_dir.join("Models");
    std::fs::create_dir_all(&models_dir)
        .map_err(|err| format!("创建本地 DeepSeek 模型目录失败：{err}"))?;
    let final_path = models_dir.join(LOCAL_DEEPSEEK_FILE_NAME);
    let partial_path = local_deepseek_partial_path(&app_dir);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1800))
        .build()
        .map_err(|err| format!("构建本地 DeepSeek 下载客户端失败：{err}"))?;
    let response = tokio::select! {
        result = request_local_deepseek_download(&client) => result.map_err(|err| {
            emit_local_deepseek_progress(&app, "failed", 0, LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES);
            err
        })?,
        _ = wait_for_local_model_download_cancel(&app) => {
            return cancel_local_deepseek_download_file(&app, &partial_path, 0, LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES);
        }
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return emit_failed_and_err(
            &app,
            0,
            LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES,
            format!(
                "本地DeepSeek模型下载失败：{}：{}",
                status,
                body.chars().take(500).collect::<String>()
            ),
        );
    }

    let total_bytes = response
        .content_length()
        .map(|value| value as i64)
        .filter(|value| *value > 0)
        .unwrap_or(LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES);
    emit_local_deepseek_progress(&app, "starting", 0, total_bytes);

    let mut file = std::fs::File::create(&partial_path).map_err(|err| {
        emit_local_deepseek_progress(&app, "failed", 0, total_bytes);
        format!("创建本地 DeepSeek 模型临时文件失败：{err}")
    })?;
    let mut stream = response.bytes_stream();
    let mut downloaded_bytes = 0i64;
    let mut last_emitted_bytes = 0i64;
    let mut cancel_check = interval(Duration::from_millis(200));
    loop {
        let chunk = tokio::select! {
            _ = cancel_check.tick() => {
                if is_local_model_download_cancel_requested(&app) {
                    drop(file);
                    return cancel_local_deepseek_download_file(&app, &partial_path, downloaded_bytes, total_bytes);
                }
                continue;
            }
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk.map_err(|err| {
            emit_local_deepseek_progress(&app, "failed", downloaded_bytes, total_bytes);
            ai::describe_request_error("本地DeepSeek模型下载中断", err)
        })?;
        if is_local_model_download_cancel_requested(&app) {
            drop(file);
            return cancel_local_deepseek_download_file(
                &app,
                &partial_path,
                downloaded_bytes,
                total_bytes,
            );
        }
        downloaded_bytes += chunk.len() as i64;
        file.write_all(&chunk).map_err(|err| {
            emit_local_deepseek_progress(&app, "failed", downloaded_bytes, total_bytes);
            format!("写入本地 DeepSeek 模型文件失败：{err}")
        })?;
        if is_local_model_download_cancel_requested(&app) {
            drop(file);
            return cancel_local_deepseek_download_file(
                &app,
                &partial_path,
                downloaded_bytes,
                total_bytes,
            );
        }
        if downloaded_bytes - last_emitted_bytes >= LOCAL_MODEL_PROGRESS_EMIT_STEP_BYTES
            || downloaded_bytes >= total_bytes
        {
            emit_local_deepseek_progress(&app, "downloading", downloaded_bytes, total_bytes);
            last_emitted_bytes = downloaded_bytes;
        }
    }
    file.flush().map_err(|err| {
        emit_local_deepseek_progress(&app, "failed", downloaded_bytes, total_bytes);
        format!("刷新本地 DeepSeek 模型文件失败：{err}")
    })?;
    drop(file);
    std::fs::rename(&partial_path, &final_path).map_err(|err| {
        emit_local_deepseek_progress(&app, "failed", downloaded_bytes, total_bytes);
        format!("保存本地 DeepSeek 模型文件失败：{err}")
    })?;
    if let Err(err) = ensure_local_deepseek_runtime_for_dir(&app, &app_dir).await {
        emit_local_deepseek_progress(&app, "failed", downloaded_bytes, total_bytes);
        return Err(err);
    }
    emit_local_deepseek_progress(&app, "done", downloaded_bytes.max(total_bytes), total_bytes);

    Ok(())
}

async fn ensure_local_deepseek_runtime(app: &AppHandle, state: &AppState) -> Result<(), String> {
    if is_llama_server_ready().await {
        return Ok(());
    }
    let has_tracked_server = state
        .local_model_server_pid
        .lock()
        .map(|pid| pid.is_some())
        .unwrap_or(false);
    if has_tracked_server && wait_for_llama_server_ready(Duration::from_secs(20)).await {
        return Ok(());
    }
    ensure_local_deepseek_runtime_for_dir(app, &state.app_dir).await
}

async fn ensure_local_deepseek_runtime_for_dir(
    app: &AppHandle,
    app_dir: &Path,
) -> Result<(), String> {
    if is_llama_server_ready().await {
        return Ok(());
    }
    let server_path = ensure_llama_server_binary(app, app_dir).await?;
    let model_path = app_dir.join("Models").join(LOCAL_DEEPSEEK_FILE_NAME);
    if !model_path.exists() {
        return Err(format!(
            "本地DeepSeek模型文件不存在：{}",
            model_path.to_string_lossy()
        ));
    }
    start_llama_server_process(app, &server_path, &model_path)?;
    if wait_for_llama_server_ready(Duration::from_secs(90)).await {
        return Ok(());
    }
    Err("本地DeepSeek推理服务启动超时。请确认机器内存足够，或稍后再次点击“启用”。".to_owned())
}

async fn ensure_llama_server_binary(app: &AppHandle, app_dir: &Path) -> Result<PathBuf, String> {
    let runtime_dir = app_dir
        .join("Runtime")
        .join("llama.cpp")
        .join(LOCAL_LLAMA_CPP_VERSION);
    let server_path = runtime_dir
        .join(format!("llama-{LOCAL_LLAMA_CPP_VERSION}"))
        .join(llama_server_binary_name());
    if server_path.exists() {
        mark_executable(&server_path)?;
        return Ok(server_path);
    }
    if install_bundled_llama_runtime(app, &runtime_dir, &server_path)? {
        mark_executable(&server_path)?;
        return Ok(server_path);
    }
    std::fs::create_dir_all(&runtime_dir)
        .map_err(|err| format!("创建本地推理运行时目录失败：{err}"))?;
    let archive_name = llama_cpp_archive_name()?;
    let archive_path = runtime_dir.join(&archive_name);
    download_llama_cpp_runtime(&archive_name, &archive_path).await?;
    extract_llama_cpp_runtime(&archive_path, &runtime_dir).await?;
    if !server_path.exists() {
        return Err(format!(
            "本地推理运行时缺少 {}：{}",
            llama_server_binary_name(),
            server_path.to_string_lossy()
        ));
    }
    mark_executable(&server_path)?;
    Ok(server_path)
}

fn bundled_llama_runtime_arch() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("darwin-arm64"),
        ("macos", "x86_64") => Ok("darwin-x64"),
        ("windows", "x86_64") => Ok("win-cpu-x64"),
        (os, arch) => Err(format!("当前架构暂不支持包内本地推理运行时：{os}/{arch}")),
    }
}

fn install_bundled_llama_runtime(
    app: &AppHandle,
    runtime_dir: &Path,
    server_path: &Path,
) -> Result<bool, String> {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return Ok(false);
    };
    let arch = bundled_llama_runtime_arch()?;
    let bundled_dir = [
        resource_dir.join("runtime"),
        resource_dir.join("_up_").join("runtime"),
        resource_dir.join("..").join("runtime"),
    ]
    .into_iter()
    .map(|root| {
        root.join("llama.cpp")
            .join(LOCAL_LLAMA_CPP_VERSION)
            .join(arch)
            .join(format!("llama-{LOCAL_LLAMA_CPP_VERSION}"))
    })
    .find(|candidate| candidate.exists());
    let Some(bundled_dir) = bundled_dir else {
        return Ok(false);
    };
    std::fs::create_dir_all(runtime_dir)
        .map_err(|err| format!("创建本地推理运行时目录失败：{err}"))?;
    copy_dir_all(
        &bundled_dir,
        runtime_dir.join(format!("llama-{LOCAL_LLAMA_CPP_VERSION}")),
    )?;
    if !server_path.exists() {
        return Err(format!(
            "包内本地推理运行时缺少 {}：{}",
            llama_server_binary_name(),
            server_path.to_string_lossy()
        ));
    }
    Ok(true)
}

fn copy_dir_all(source: &Path, destination: PathBuf) -> Result<(), String> {
    if destination.exists() {
        std::fs::remove_dir_all(&destination)
            .map_err(|err| format!("清理旧本地推理运行时失败：{err}"))?;
    }
    std::fs::create_dir_all(&destination)
        .map_err(|err| format!("创建本地推理运行时目录失败：{err}"))?;
    for entry in
        std::fs::read_dir(source).map_err(|err| format!("读取包内本地推理运行时失败：{err}"))?
    {
        let entry = entry.map_err(|err| format!("读取包内本地推理运行时失败：{err}"))?;
        let file_type = entry
            .file_type()
            .map_err(|err| format!("读取包内本地推理运行时文件类型失败：{err}"))?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .map_err(|err| format!("复制包内本地推理运行时失败：{err}"))?;
        }
    }
    Ok(())
}

fn llama_server_binary_name() -> &'static str {
    if std::env::consts::OS == "windows" {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

fn llama_cpp_archive_name() -> Result<String, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("macos", "aarch64") => Ok(format!(
            "llama-{LOCAL_LLAMA_CPP_VERSION}-bin-macos-arm64.tar.gz"
        )),
        ("macos", "x86_64") => Ok(format!(
            "llama-{LOCAL_LLAMA_CPP_VERSION}-bin-macos-x64.tar.gz"
        )),
        ("windows", "x86_64") => Ok(format!(
            "llama-{LOCAL_LLAMA_CPP_VERSION}-bin-win-cpu-x64.zip"
        )),
        ("windows", "aarch64") => Ok(format!(
            "llama-{LOCAL_LLAMA_CPP_VERSION}-bin-win-cpu-arm64.zip"
        )),
        _ => Err(format!(
            "当前系统暂不支持自动下载本地推理运行时：{os}/{arch}"
        )),
    }
}

async fn extract_llama_cpp_runtime(archive_path: &Path, runtime_dir: &Path) -> Result<(), String> {
    if std::env::consts::OS == "windows" {
        return extract_zip_archive(archive_path, runtime_dir);
    }
    let archive = archive_path.to_string_lossy();
    let destination = runtime_dir.to_string_lossy();
    let output = Command::new("tar")
        .args(["-xzf", archive.as_ref(), "-C", destination.as_ref()])
        .output()
        .await
        .map_err(|err| format!("解压本地推理运行时失败：{err}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "解压本地推理运行时失败：{}",
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(500)
            .collect::<String>()
    ))
}

fn extract_zip_archive(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive_path)
        .map_err(|err| format!("打开本地推理运行时压缩包失败：{err}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| format!("读取本地推理运行时压缩包失败：{err}"))?;
    std::fs::create_dir_all(destination)
        .map_err(|err| format!("创建本地推理运行时目录失败：{err}"))?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| format!("读取本地推理运行时压缩包条目失败：{err}"))?;
        let Some(relative_path) = file.enclosed_name().map(|path| path.to_owned()) else {
            continue;
        };
        let output_path = destination.join(relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&output_path)
                .map_err(|err| format!("创建本地推理运行时目录失败：{err}"))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("创建本地推理运行时目录失败：{err}"))?;
        }
        let mut output = std::fs::File::create(&output_path)
            .map_err(|err| format!("写入本地推理运行时文件失败：{err}"))?;
        std::io::copy(&mut file, &mut output)
            .map_err(|err| format!("写入本地推理运行时文件失败：{err}"))?;
    }
    Ok(())
}

async fn download_llama_cpp_runtime(archive_name: &str, archive_path: &Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|err| format!("构建本地推理运行时下载客户端失败：{err}"))?;
    let official_url = format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{LOCAL_LLAMA_CPP_VERSION}/{archive_name}"
    );
    let sources = [
        format!("https://gh-proxy.com/{official_url}"),
        format!("https://gh.llkk.cc/{official_url}"),
        official_url,
    ];
    let mut errors = Vec::new();
    for source_url in sources {
        match client.get(&source_url).send().await {
            Ok(response) if response.status().is_success() => {
                let mut file = std::fs::File::create(archive_path)
                    .map_err(|err| format!("写入本地推理运行时失败：{err}"))?;
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk
                        .map_err(|err| ai::describe_request_error("下载本地推理运行时中断", err))?;
                    file.write_all(&chunk)
                        .map_err(|err| format!("写入本地推理运行时失败：{err}"))?;
                }
                file.flush()
                    .map_err(|err| format!("写入本地推理运行时失败：{err}"))?;
                return Ok(());
            }
            Ok(response) => {
                let status = response.status();
                errors.push(format!("{source_url} 返回 {status}"));
            }
            Err(err) => errors.push(ai::describe_request_error(
                &format!("{source_url} 请求未发出"),
                err,
            )),
        }
    }
    Err(format!(
        "下载本地推理运行时失败，已尝试 GitHub 镜像和官方源：{}",
        errors.join("；")
    ))
}

fn mark_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let metadata =
            std::fs::metadata(path).map_err(|err| format!("读取本地推理运行时权限失败：{err}"))?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)
            .map_err(|err| format!("设置本地推理运行时可执行权限失败：{err}"))?;
    }
    Ok(())
}

fn start_llama_server_process(
    app: &AppHandle,
    server_path: &Path,
    model_path: &Path,
) -> Result<(), String> {
    let working_dir = server_path
        .parent()
        .ok_or_else(|| "本地推理运行时路径异常。".to_owned())?;
    let mut child = std::process::Command::new(server_path)
        .current_dir(working_dir)
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            "11434",
            "--model",
            model_path.to_string_lossy().as_ref(),
            "--alias",
            ai::LOCAL_DEEPSEEK_MODEL,
            "--ctx-size",
            "4096",
            "--n-gpu-layers",
            "99",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("启动本地DeepSeek推理服务失败：{err}"))?;
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut pid) = state.local_model_server_pid.lock() {
            *pid = Some(child.id());
        }
    }
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

async fn is_llama_server_ready() -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    else {
        return false;
    };
    client
        .get(format!("{LOCAL_DEEPSEEK_BASE_URL}/models"))
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

async fn wait_for_llama_server_ready(timeout: Duration) -> bool {
    let started_at = Instant::now();
    while started_at.elapsed() < timeout {
        if is_llama_server_ready().await {
            return true;
        }
        sleep(Duration::from_millis(700)).await;
    }
    false
}

async fn request_local_deepseek_download(
    client: &reqwest::Client,
) -> Result<reqwest::Response, String> {
    let sources = [
        LOCAL_DEEPSEEK_SOURCE_URL,
        LOCAL_DEEPSEEK_FALLBACK_SOURCE_URL,
    ];
    let mut errors = Vec::new();
    for source_url in sources {
        match client.get(source_url).send().await {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                errors.push(format!(
                    "{source_url} 返回 {status}：{}",
                    body.chars().take(240).collect::<String>()
                ));
            }
            Err(err) => errors.push(ai::describe_request_error(
                &format!("{source_url} 请求未发出"),
                err,
            )),
        }
    }
    Err(format!(
        "本地DeepSeek模型下载请求失败，已尝试镜像源和官方源：{}",
        errors.join("；")
    ))
}

fn local_deepseek_status(state: &AppState) -> LocalModelStatus {
    local_deepseek_status_from_dir(&state.app_dir)
}

fn local_deepseek_status_from_dir(app_dir: &Path) -> LocalModelStatus {
    let path = app_dir.join("Models").join(LOCAL_DEEPSEEK_FILE_NAME);
    let size_bytes = path
        .metadata()
        .map(|metadata| metadata.len() as i64)
        .unwrap_or_default();
    LocalModelStatus {
        provider: ai::LOCAL_DEEPSEEK_PROVIDER.to_owned(),
        model: ai::LOCAL_DEEPSEEK_MODEL.to_owned(),
        file_name: LOCAL_DEEPSEEK_FILE_NAME.to_owned(),
        file_path: path.to_string_lossy().to_string(),
        source_url: LOCAL_DEEPSEEK_SOURCE_URL.to_owned(),
        installed: size_bytes > 0,
        size_bytes,
        expected_size_bytes: LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES,
        updated_at: chrono::Local::now().to_rfc3339(),
    }
}

fn local_deepseek_partial_path(app_dir: &Path) -> PathBuf {
    app_dir
        .join("Models")
        .join(LOCAL_DEEPSEEK_FILE_NAME)
        .with_extension("gguf.part")
}

fn current_local_deepseek_progress(
    state: &AppState,
) -> Result<Option<LocalModelDownloadProgress>, String> {
    state
        .local_model_download_progress
        .lock()
        .map(|progress| progress.clone())
        .map_err(|err| format!("读取本地模型下载进度失败：{err}"))
}

fn is_local_model_download_active(progress: &LocalModelDownloadProgress) -> bool {
    matches!(progress.status.as_str(), "starting" | "downloading")
}

async fn wait_for_local_model_download_cancel(app: &AppHandle) {
    loop {
        if is_local_model_download_cancel_requested(app) {
            return;
        }
        sleep(Duration::from_millis(200)).await;
    }
}

fn is_local_model_download_cancel_requested(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| {
            state
                .local_model_download_cancel_requested
                .load(Ordering::SeqCst)
        })
        .unwrap_or(false)
}

fn is_managed_local_deepseek_config(config: &AiConfig) -> bool {
    config.provider == ai::LOCAL_DEEPSEEK_PROVIDER || config.model == ai::LOCAL_DEEPSEEK_MODEL
}

pub(crate) fn is_configured_for_current_runtime(config: &AiConfig, state: &AppState) -> bool {
    if !ai::is_configured(config) {
        return false;
    }
    if is_managed_local_deepseek_config(config) {
        return is_local_deepseek_available_for_config(config, state);
    }
    true
}

fn is_local_deepseek_available_for_config(config: &AiConfig, state: &AppState) -> bool {
    let status = local_deepseek_status(state);
    status.installed
        && status.model == config.model
        && config.base_url.trim().trim_end_matches('/') == LOCAL_DEEPSEEK_BASE_URL
}

fn emit_failed_and_err<T>(
    app: &AppHandle,
    downloaded_bytes: i64,
    total_bytes: i64,
    message: String,
) -> Result<T, String> {
    emit_local_deepseek_progress(app, "failed", downloaded_bytes, total_bytes);
    Err(message)
}

fn cancel_local_deepseek_download_file(
    app: &AppHandle,
    partial_path: &Path,
    downloaded_bytes: i64,
    total_bytes: i64,
) -> Result<(), String> {
    if partial_path.exists() {
        std::fs::remove_file(partial_path)
            .map_err(|err| format!("删除本地DeepSeek模型临时文件失败：{err}"))?;
    }
    emit_local_deepseek_progress(app, "cancelled", downloaded_bytes, total_bytes);
    if let Some(state) = app.try_state::<AppState>() {
        state
            .local_model_download_cancel_requested
            .store(false, Ordering::SeqCst);
    }
    Ok(())
}

fn emit_local_deepseek_progress(
    app: &AppHandle,
    status: &str,
    downloaded_bytes: i64,
    total_bytes: i64,
) {
    let total_bytes = total_bytes.max(1);
    let percent = ((downloaded_bytes.max(0) as f64 / total_bytes as f64) * 100.0).clamp(0.0, 100.0);
    let progress = LocalModelDownloadProgress {
        provider: ai::LOCAL_DEEPSEEK_PROVIDER.to_owned(),
        model: ai::LOCAL_DEEPSEEK_MODEL.to_owned(),
        status: status.to_owned(),
        downloaded_bytes,
        total_bytes,
        percent,
    };
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut current) = state.local_model_download_progress.lock() {
            *current = Some(progress.clone());
        }
    }
    let _ = app.emit(LOCAL_MODEL_PROGRESS_EVENT, progress);
}
