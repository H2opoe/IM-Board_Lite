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
            eprintln!("本地DeepSeek模型下载失败：{err}");
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

include!("ai_local_download.rs");

include!("ai_local_runtime.rs");

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
