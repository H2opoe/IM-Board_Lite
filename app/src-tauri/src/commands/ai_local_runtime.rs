async fn ensure_local_deepseek_runtime(app: &AppHandle, state: &AppState) -> Result<(), String> {
    ensure_local_deepseek_runtime_for_dir(app, &state.app_dir).await
}

async fn ensure_local_deepseek_runtime_for_dir(
    app: &AppHandle,
    app_dir: &Path,
) -> Result<(), String> {
    let llama_server = ensure_llama_server_binary(app, app_dir).await?;
    if is_llama_server_ready().await {
        return Ok(());
    }
    start_llama_server_process(app_dir, &llama_server)?;
    if wait_for_llama_server_ready(Duration::from_secs(30)).await {
        return Ok(());
    }
    Err("本地DeepSeek推理服务启动超时。".to_owned())
}

async fn ensure_llama_server_binary(app: &AppHandle, app_dir: &Path) -> Result<PathBuf, String> {
    let arch = bundled_llama_runtime_arch()?;
    let runtime_dir = app_dir
        .join("runtime")
        .join("llama.cpp")
        .join(LOCAL_LLAMA_CPP_VERSION)
        .join(arch)
        .join(format!("llama-{LOCAL_LLAMA_CPP_VERSION}"));
    let binary = runtime_dir.join(llama_server_binary_name());
    if binary.exists() {
        return Ok(binary);
    }
    install_bundled_llama_runtime(app, app_dir, &runtime_dir)?;
    if binary.exists() {
        return Ok(binary);
    }
    Err(format!("本地推理运行时缺少：{}", binary.display()))
}

fn bundled_llama_runtime_arch() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok("win-cpu-x64"),
        (os, arch) => Err(format!("当前架构暂不支持包内本地推理运行时：{os}/{arch}")),
    }
}

fn install_bundled_llama_runtime(
    app: &AppHandle,
    app_dir: &Path,
    runtime_dir: &Path,
) -> Result<(), String> {
    let arch = bundled_llama_runtime_arch()?;
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    let candidates = [
        resource_dir
            .join("runtime")
            .join("llama.cpp")
            .join(LOCAL_LLAMA_CPP_VERSION)
            .join(arch),
        resource_dir
            .join("_up_")
            .join("runtime")
            .join("llama.cpp")
            .join(LOCAL_LLAMA_CPP_VERSION)
            .join(arch),
        app_dir
            .join("runtime")
            .join("llama.cpp")
            .join(LOCAL_LLAMA_CPP_VERSION)
            .join(arch),
    ];
    for source in candidates {
        let source_runtime = source.join(format!("llama-{LOCAL_LLAMA_CPP_VERSION}"));
        if source_runtime.exists() {
            copy_dir_all(&source_runtime, runtime_dir.to_path_buf())?;
            return Ok(());
        }
    }
    Err(format!("包内本地推理运行时缺失：{arch}"))
}

fn copy_dir_all(source: &Path, destination: PathBuf) -> Result<(), String> {
    if destination.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&destination).map_err(|err| err.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let file_type = entry.file_type().map_err(|err| err.to_string())?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), target)?;
        } else {
            std::fs::copy(entry.path(), target).map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

fn llama_server_binary_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

fn llama_cpp_archive_name() -> Result<String, String> {
    Ok(format!(
        "llama-{}-{}.zip",
        LOCAL_LLAMA_CPP_VERSION,
        bundled_llama_runtime_arch()?
    ))
}

async fn extract_llama_cpp_runtime(_archive_path: &Path, _runtime_dir: &Path) -> Result<(), String> {
    Ok(())
}

fn extract_zip_archive(_archive_path: &Path, _destination: &Path) -> Result<(), String> {
    Ok(())
}

async fn download_llama_cpp_runtime(_archive_name: &str, _archive_path: &Path) -> Result<(), String> {
    Err("当前Windows包不支持在线下载本地推理运行时。".to_owned())
}

fn mark_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn start_llama_server_process(app_dir: &Path, llama_server: &Path) -> Result<(), String> {
    let model_path = app_dir.join("models").join(LOCAL_DEEPSEEK_FILE_NAME);
    if !model_path.exists() {
        return Err(format!("Local DeepSeek model is missing: {}", model_path.display()));
    }
    let mut command = Command::new(llama_server);
    command
        .arg("--model")
        .arg(model_path)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg("11434")
        .arg("--ctx-size")
        .arg("4096")
        .arg("--embedding")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map_err(|err| format!("启动本地推理运行时失败：{err}"))?;
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
