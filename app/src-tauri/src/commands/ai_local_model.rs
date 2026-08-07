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
    let db_path = state.app_dir.join("app.sqlite");
    let app_for_task = app.clone();
    state
        .local_model_download_cancel_requested
        .store(false, Ordering::SeqCst);
    emit_local_deepseek_progress(app, "starting", 0, LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES);
    tauri::async_runtime::spawn(async move {
        if let Err(err) = download_local_deepseek_model(app_for_task.clone(), app_dir).await {
            eprintln!("本地DeepSeek模型下载失败：{err}");
            record_local_ai_error_to_db(&db_path, "download_local_deepseek_model", &err);
        }
    });
    Ok(())
}

fn record_local_ai_error_to_db(db_path: &Path, operation: &str, message: &str) {
    let Ok(conn) = rusqlite::Connection::open(db_path) else {
        return;
    };
    let event = crate::diagnostics::local_ai_error_event(
        operation,
        message,
        serde_json::json!({ "provider": "本地DeepSeek" }),
    );
    let _ = crate::diagnostics::record_error_event_conn(&conn, event);
}

fn stop_tracked_llama_server(state: &AppState) -> Result<(), String> {
    state.local_model_runtime.stop()?;
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

struct LocalDeepseekDownloadResponse {
    response: reqwest::Response,
    resumed_bytes: i64,
}

async fn request_local_deepseek_download(
    client: &reqwest::Client,
    resume_from_bytes: i64,
) -> Result<LocalDeepseekDownloadResponse, String> {
    let sources = [
        LOCAL_DEEPSEEK_SOURCE_URL,
        LOCAL_DEEPSEEK_HF_MIRROR_SOURCE_URL,
        LOCAL_DEEPSEEK_FALLBACK_SOURCE_URL,
    ];
    let mut errors = Vec::new();
    for source_url in sources {
        let request = client.get(source_url);
        let request = if resume_from_bytes > 0 {
            request.header(reqwest::header::RANGE, format!("bytes={resume_from_bytes}-"))
        } else {
            request
        };
        match request.send().await {
            Ok(response) if response.status().is_success() => {
                let status = response.status();
                if resume_from_bytes > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
                    return Ok(LocalDeepseekDownloadResponse {
                        response,
                        resumed_bytes: resume_from_bytes,
                    });
                }
                if resume_from_bytes > 0 && status == reqwest::StatusCode::OK {
                    errors.push(format!("{source_url} 不支持断点续传，已改为重新完整下载"));
                }
                return Ok(LocalDeepseekDownloadResponse {
                    response,
                    resumed_bytes: 0,
                });
            }
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
        "本地DeepSeek模型下载请求失败，已尝试魔搭、HF-Mirror和官方源：{}",
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
        installed: size_bytes == LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES
            && local_deepseek_verification_marker_is_valid(app_dir),
        size_bytes,
        expected_size_bytes: LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES,
        updated_at: chrono::Local::now().to_rfc3339(),
    }
}

async fn verified_local_deepseek_status(state: &AppState) -> LocalModelStatus {
    let status = local_deepseek_status(state);
    if status.installed || status.size_bytes != LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES {
        return status;
    }
    let model_path = state.app_dir.join("Models").join(LOCAL_DEEPSEEK_FILE_NAME);
    if verify_local_deepseek_model(&model_path).await.is_ok()
        && write_local_deepseek_verification_marker(&state.app_dir).is_ok()
    {
        return local_deepseek_status(state);
    }
    status
}

async fn verify_local_deepseek_model(path: &Path) -> Result<(), String> {
    let path = path.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || verify_local_deepseek_model_sync(&path))
        .await
        .map_err(|error| format!("校验本地DeepSeek模型任务失败：{error}"))?
}

fn verify_local_deepseek_model_sync(path: &Path) -> Result<(), String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("读取本地DeepSeek模型失败：{error}"))?;
    if metadata.len() as i64 != LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES {
        return Err(format!(
            "本地DeepSeek模型大小不完整：{} / {} 字节。",
            metadata.len(),
            LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES
        ));
    }
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("打开本地DeepSeek模型失败：{error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 8 * 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("读取本地DeepSeek模型失败：{error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != LOCAL_DEEPSEEK_EXPECTED_SHA256 {
        return Err(format!(
            "本地DeepSeek模型校验失败：SHA-256不匹配（{actual}）。"
        ));
    }
    Ok(())
}

fn move_corrupt_model_to_partial(app_dir: &Path, model_path: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(local_deepseek_verification_marker_path(app_dir));
    let partial_path = local_deepseek_partial_path(app_dir);
    if partial_path.exists() {
        std::fs::remove_file(&partial_path)
            .map_err(|error| format!("清理旧模型临时文件失败：{error}"))?;
    }
    std::fs::rename(model_path, partial_path)
        .map_err(|error| format!("将损坏模型转回可重试状态失败：{error}"))
}

fn local_deepseek_verification_marker_path(app_dir: &Path) -> PathBuf {
    app_dir.join("Models").join(format!("{LOCAL_DEEPSEEK_FILE_NAME}.verified"))
}

fn local_deepseek_verification_marker_is_valid(app_dir: &Path) -> bool {
    std::fs::read_to_string(local_deepseek_verification_marker_path(app_dir))
        .is_ok_and(|value| {
            value.trim()
                == format!("{LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES}:{LOCAL_DEEPSEEK_EXPECTED_SHA256}")
        })
}

fn write_local_deepseek_verification_marker(app_dir: &Path) -> Result<(), String> {
    std::fs::write(
        local_deepseek_verification_marker_path(app_dir),
        format!("{LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES}:{LOCAL_DEEPSEEK_EXPECTED_SHA256}\n"),
    )
    .map_err(|error| format!("记录本地DeepSeek模型校验结果失败：{error}"))
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

#[cfg(test)]
mod local_model_integrity_tests {
    use super::*;

    #[test]
    fn installed_status_requires_a_matching_verification_marker() {
        let app_dir = std::env::temp_dir().join(format!(
            "im-board-local-model-status-{}",
            uuid::Uuid::new_v4()
        ));
        let models_dir = app_dir.join("Models");
        std::fs::create_dir_all(&models_dir).expect("models dir");
        let model_path = models_dir.join(LOCAL_DEEPSEEK_FILE_NAME);
        let file = std::fs::File::create(&model_path).expect("model file");
        file.set_len(LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES as u64)
            .expect("sparse model size");

        assert!(!local_deepseek_status_from_dir(&app_dir).installed);
        write_local_deepseek_verification_marker(&app_dir).expect("verification marker");
        assert!(local_deepseek_status_from_dir(&app_dir).installed);
        std::fs::write(local_deepseek_verification_marker_path(&app_dir), "wrong")
            .expect("corrupt marker");
        assert!(!local_deepseek_status_from_dir(&app_dir).installed);

        std::fs::remove_dir_all(app_dir).expect("cleanup");
    }
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
    status.installed && status.model == config.model
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
