async fn download_local_deepseek_model(app: AppHandle, app_dir: PathBuf) -> Result<(), String> {
    let models_dir = app_dir.join("Models");
    std::fs::create_dir_all(&models_dir)
        .map_err(|err| format!("创建本地DeepSeek模型目录失败：{err}"))?;
    let final_path = models_dir.join(LOCAL_DEEPSEEK_FILE_NAME);
    let partial_path = local_deepseek_partial_path(&app_dir);
    let existing_partial_bytes = partial_path
        .metadata()
        .map(|metadata| metadata.len() as i64)
        .ok()
        .filter(|size| *size > 0 && *size < LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES)
        .unwrap_or_default();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1800))
        .build()
        .map_err(|err| format!("构建本地DeepSeek下载客户端失败：{err}"))?;
    let download_response = tokio::select! {
        result = request_local_deepseek_download(&client, existing_partial_bytes) => result.map_err(|err| {
            emit_local_deepseek_progress(&app, "failed", existing_partial_bytes, LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES);
            err
        })?,
        _ = wait_for_local_model_download_cancel(&app) => {
            return cancel_local_deepseek_download_file(&app, &partial_path, existing_partial_bytes, LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES);
        }
    };
    let response = download_response.response;
    let resumed_bytes = download_response.resumed_bytes;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return emit_failed_and_err(
            &app,
            resumed_bytes,
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
        .map(|value| value as i64 + resumed_bytes)
        .filter(|value| *value > 0)
        .unwrap_or(LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES)
        .max(LOCAL_DEEPSEEK_EXPECTED_SIZE_BYTES);
    emit_local_deepseek_progress(&app, "starting", resumed_bytes, total_bytes);

    // 中断重试时复用 .part 文件，并通过 HTTP Range 从已落盘字节继续下载。
    // 如果镜像源不支持 Range，request_local_deepseek_download 会把 resumed_bytes 置 0，
    // 这里会覆盖旧临时文件并重新完整下载。
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed_bytes > 0)
        .truncate(resumed_bytes == 0)
        .open(&partial_path)
        .map_err(|err| {
            emit_local_deepseek_progress(&app, "failed", resumed_bytes, total_bytes);
            format!("创建本地DeepSeek模型临时文件失败：{err}")
        })?;
    let mut stream = response.bytes_stream();
    let mut downloaded_bytes = resumed_bytes;
    let mut last_emitted_bytes = resumed_bytes;
    if downloaded_bytes > 0 {
        emit_local_deepseek_progress(&app, "downloading", downloaded_bytes, total_bytes);
    }
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
            format!("写入本地DeepSeek模型文件失败：{err}")
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
    if downloaded_bytes < total_bytes {
        return emit_failed_and_err(
            &app,
            downloaded_bytes,
            total_bytes,
            format!("本地DeepSeek模型下载不完整：已下载 {downloaded_bytes} / {total_bytes} 字节，可重试继续下载。"),
        );
    }
    file.flush().map_err(|err| {
        emit_local_deepseek_progress(&app, "failed", downloaded_bytes, total_bytes);
        format!("刷新本地DeepSeek模型文件失败：{err}")
    })?;
    drop(file);
    verify_local_deepseek_model(&partial_path).await.map_err(|error| {
        emit_local_deepseek_progress(&app, "failed", downloaded_bytes, total_bytes);
        format!("下载完成但模型完整性校验失败，可重试重新下载：{error}")
    })?;
    std::fs::rename(&partial_path, &final_path).map_err(|err| {
        emit_local_deepseek_progress(&app, "failed", downloaded_bytes, total_bytes);
        format!("保存本地DeepSeek模型文件失败：{err}")
    })?;
    write_local_deepseek_verification_marker(&app_dir)?;
    if let Err(err) = ensure_local_deepseek_runtime_for_dir(&app, &app_dir).await {
        emit_local_deepseek_progress(&app, "failed", downloaded_bytes, total_bytes);
        return Err(err);
    }
    emit_local_deepseek_progress(&app, "done", downloaded_bytes.max(total_bytes), total_bytes);

    Ok(())
}
