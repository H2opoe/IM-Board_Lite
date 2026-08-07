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

    let model_path = state.app_dir.join("Models").join(LOCAL_DEEPSEEK_FILE_NAME);
    if let Err(error) = verify_local_deepseek_model(&model_path).await {
        if model_path.exists() {
            move_corrupt_model_to_partial(&state.app_dir, &model_path)?;
        }
        return Err(format!("{error} 已转为可重试下载状态。"));
    }
    write_local_deepseek_verification_marker(&state.app_dir)?;
    let server_path = ensure_llama_server_binary(app, &state.app_dir).await?;
    state.local_model_runtime.renew_endpoint()?;
    start_llama_server_process(app, &server_path, &model_path, state.local_model_runtime.port())?;
    if state
        .local_model_runtime
        .wait_until_ready(ai::LOCAL_DEEPSEEK_MODEL, Duration::from_secs(90))
        .await
    {
        return Ok(state.local_model_runtime.base_url());
    }
    let _ = state.local_model_runtime.stop();
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
        "下载本地推理运行时失败，已尝试GitHub镜像和官方源：{}",
        errors.join("；")
    ))
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
        .arg("4096")
        .arg("--n-gpu-layers")
        .arg("99")
        .stdout(Stdio::null())
        .stderr(stderr);
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
