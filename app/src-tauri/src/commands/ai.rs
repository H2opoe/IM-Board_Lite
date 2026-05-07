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
            start_local_deepseek_download(&app, &state).map_err(|err| {
                record_local_ai_error(&state, "start_local_deepseek_download", &err);
                err
            })?;
            return Ok("downloading".to_owned());
        }
        if config.enabled {
            ensure_local_deepseek_runtime(&app, &state)
                .await
                .map_err(|err| {
                    record_local_ai_error(&state, "ensure_local_deepseek_runtime", &err);
                    err
                })?;
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
            .map_err(|err| {
                let message = ai::describe_request_error("API请求未发出", err);
                record_ai_config_test_error(&state, &message, None);
                message
            })?
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
            .map_err(|err| {
                let message = ai::describe_request_error("API请求未发出", err);
                record_ai_config_test_error(&state, &message, None);
                message
            })?
    };

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let detail = if body.trim().is_empty() {
            status.to_string()
        } else {
            body.chars().take(500).collect()
        };
        let message = format!("API返回错误：{status}：{detail}");
        let diagnostic = serde_json::json!({
            "httpStatus": status.as_u16(),
            "responseBodySnippet": detail,
        });
        record_ai_config_test_error(&state, &message, Some(diagnostic.clone()));
        return Err(crate::diagnostics::classify_ai_user_message(
            &message,
            Some(&diagnostic),
        ));
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
        ensure_local_deepseek_runtime(&app, &state)
            .await
            .map_err(|err| {
                record_local_ai_error(&state, "ensure_local_deepseek_runtime", &err);
                err
            })?;
        emit_local_deepseek_progress(
            &app,
            "done",
            installed.size_bytes.max(installed.expected_size_bytes),
            installed.expected_size_bytes,
        );
        return Ok(local_deepseek_status(&state));
    }
    start_local_deepseek_download(&app, &state).map_err(|err| {
        record_local_ai_error(&state, "start_local_deepseek_download", &err);
        err
    })?;
    Ok(local_deepseek_status(&state))
}

fn record_ai_config_test_error(
    state: &AppState,
    message: &str,
    diagnostic: Option<serde_json::Value>,
) {
    crate::diagnostics::record_error_event(
        state,
        crate::diagnostics::ai_error_event(None, "test_ai_connection", message, diagnostic),
    );
}

fn record_local_ai_error(state: &AppState, operation: &str, message: &str) {
    crate::diagnostics::record_error_event(
        state,
        crate::diagnostics::local_ai_error_event(
            operation,
            message,
            serde_json::json!({ "provider": "本地DeepSeek" }),
        ),
    );
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

include!("ai_local_model.rs");
