async fn ensure_local_deepseek_runtime(
    app: &AppHandle,
    state: &AppState,
    force_restart: bool,
) -> Result<String, String> {
    let _startup_guard = state.local_model_runtime.startup_guard().await;
    if !force_restart
        && state
            .local_model_runtime
            .is_owned_runtime_ready(ai::LOCAL_DEEPSEEK_MODEL)
            .await
    {
        return Ok(state.local_model_runtime.base_url());
    }
    if state.local_model_runtime.has_owned_process() {
        let _ = state.local_model_runtime.stop();
    }

    SystemCapabilities::detect(&state.app_dir).ensure_local_model_disk_space()?;
    let model_path = state.app_dir.join("Models").join(LOCAL_DEEPSEEK_FILE_NAME);
    if let Err(error) = verify_local_deepseek_model(&model_path).await {
        if model_path.exists() {
            move_corrupt_model_to_partial(&state.app_dir, &model_path)?;
        }
        return Err(format!("{error} 已转为可重试下载状态。"));
    }
    write_local_deepseek_verification_marker(&state.app_dir)?;
    let server_path = ensure_llama_server_binary(app, &state.app_dir).await?;
    for attempt in 0..3 {
        state.local_model_runtime.renew_endpoint()?;
        start_llama_server_process(
            app,
            &server_path,
            &model_path,
            state.local_model_runtime.port(),
        )?;
        if state
            .local_model_runtime
            .wait_until_ready(ai::LOCAL_DEEPSEEK_MODEL, Duration::from_secs(90))
            .await
        {
            return Ok(state.local_model_runtime.base_url());
        }
        let exited_early = !state.local_model_runtime.has_owned_process();
        let _ = state.local_model_runtime.stop();
        if !exited_early || attempt == 2 {
            break;
        }
    }
    Err("本地DeepSeek推理服务启动超时。请确认机器内存足够，或稍后再次点击“启用”。".to_owned())
}

async fn ensure_local_deepseek_runtime_for_dir(
    app: &AppHandle,
    app_dir: &Path,
) -> Result<String, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| format!("本地推理运行时尚未初始化：{}", app_dir.display()))?;
    ensure_local_deepseek_runtime(app, &state, false).await
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
    Err(format!(
        "安装包缺少经过校验的本地推理运行时（{}）。请使用完整安装包重新安装；应用不会从未锁定的镜像临时下载可执行程序。",
        llama_server_binary_name()
    ))
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

fn mark_executable(_path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let metadata =
            std::fs::metadata(_path).map_err(|err| format!("读取本地推理运行时权限失败：{err}"))?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(_path, permissions)
            .map_err(|err| format!("设置本地推理运行时可执行权限失败：{err}"))?;
    }
    Ok(())
}

fn start_llama_server_process(
    app: &AppHandle,
    server_path: &Path,
    model_path: &Path,
    port: u16,
) -> Result<(), String> {
    let working_dir = server_path
        .parent()
        .ok_or_else(|| "本地推理运行时路径异常。".to_owned())?;
    let runtime_log_path = local_deepseek_runtime_log_path(app, working_dir);
    let stderr = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&runtime_log_path)
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null());
    let mut command = std::process::Command::new(server_path);
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "本地推理运行时状态不可用。".to_owned())?;
    let tuning = SystemCapabilities::detect(&state.app_dir);
    command
        .current_dir(working_dir)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--model")
        .arg(model_path)
        .arg("--alias")
        .arg(ai::LOCAL_DEEPSEEK_MODEL)
        .arg("--ctx-size")
        .arg(tuning.local_model_context_size.to_string())
        .arg("--n-gpu-layers")
        .arg(tuning.local_model_gpu_layers.to_string())
        .arg("--threads")
        .arg(tuning.local_model_threads.to_string())
        .stdout(Stdio::null())
        .stderr(stderr);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|err| format!("启动本地DeepSeek推理服务失败：{err}"))?;
    let child_pid = child.id();
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "本地推理运行时状态不可用。".to_owned())?;
    let generation = state.local_model_runtime.register(child_pid);
    let app_for_wait = app.clone();
    std::thread::spawn(move || {
        let exit_result = child.wait();
        let Some(state) = app_for_wait.try_state::<AppState>() else {
            return;
        };
        let was_active_server = state
            .local_model_runtime
            .clear_if_owned(child_pid, generation);
        if !was_active_server {
            return;
        }
        let (message, exit_status) = match exit_result {
            Ok(status) if status.success() => return,
            Ok(status) => (
                format!("本地DeepSeek推理服务异常退出：{status}"),
                status.to_string(),
            ),
            Err(err) => (
                format!("等待本地DeepSeek推理服务退出失败：{err}"),
                "wait_failed".to_owned(),
            ),
        };
        crate::diagnostics::record_error_event(
            &state,
            crate::diagnostics::local_ai_error_event(
                "llama_server_exit",
                &message,
                serde_json::json!({
                    "provider": "本地DeepSeek",
                    "pid": child_pid,
                    "exitStatus": exit_status,
                    "runtimeLogPath": runtime_log_path.to_string_lossy(),
                    "runtimeLogTail": local_deepseek_runtime_log_tail(&runtime_log_path, 2_000),
                }),
            ),
        );
    });
    Ok(())
}

fn local_deepseek_runtime_log_path(app: &AppHandle, working_dir: &Path) -> PathBuf {
    app.try_state::<AppState>()
        .map(|state| state.cache_dir.join("local-deepseek-runtime.log"))
        .unwrap_or_else(|| working_dir.join("local-deepseek-runtime.log"))
}

fn local_deepseek_runtime_log_tail(path: &Path, max_chars: usize) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let char_count = content.chars().count();
    if char_count <= max_chars {
        return content;
    }
    content
        .chars()
        .skip(char_count.saturating_sub(max_chars))
        .collect()
}
