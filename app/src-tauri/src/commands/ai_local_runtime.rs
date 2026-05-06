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
    "llama-server"
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
        _ => Err(format!(
            "当前系统暂不支持自动下载本地推理运行时：{os}/{arch}"
        )),
    }
}

async fn extract_llama_cpp_runtime(archive_path: &Path, runtime_dir: &Path) -> Result<(), String> {
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
