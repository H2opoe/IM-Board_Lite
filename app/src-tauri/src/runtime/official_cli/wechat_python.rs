async fn install_windows_wechat_python_cli(
    install_root: &Path,
    version: &str,
    resource_dir: &Path,
    progress: Option<&PlatformCliProgressContext<'_>>,
) -> Result<(), String> {
    let python = bundled_windows_python(resource_dir).ok_or_else(|| {
        "微信CLI需要应用内置Windows Python运行时，但当前安装包缺少runtime/python。请使用重新打包后的IM-Board。".to_owned()
    })?;
    let site_dir = install_root.join("python-site");
    let bin_dir = install_root.join("bin");
    let pip_cache = install_root.join("pip-cache");
    fs::create_dir_all(&site_dir).map_err(|err| {
        format!(
            "无法创建微信CLI Python依赖目录 {}：{err}",
            site_dir.display()
        )
    })?;
    fs::create_dir_all(&bin_dir)
        .map_err(|err| format!("无法创建微信CLI启动目录 {}：{err}", bin_dir.display()))?;
    fs::create_dir_all(&pip_cache)
        .map_err(|err| format!("无法创建微信CLI pip缓存目录 {}：{err}", pip_cache.display()))?;

    if let Some(progress) = progress {
        emit_platform_cli_progress(
            progress,
            "installing",
            "正在用应用内置Python准备Windows微信CLI...".to_owned(),
            3,
            5,
            None,
            Some("wechat-cli"),
            Some(version),
        );
    }

    run_python_install_step(
        &python,
        &["-m", "ensurepip", "--upgrade"],
        &pip_cache,
        "初始化内置 Python pip 失败",
    )?;
    let site_dir_arg = site_dir.to_string_lossy().to_string();
    let mut installed_source = "";
    let mut install_errors = Vec::new();
    for source_archive in WECHAT_CLI_SOURCE_ARCHIVES {
        match run_python_install_step(
            &python,
            &[
                "-m",
                "pip",
                "install",
                "--upgrade",
                "--target",
                &site_dir_arg,
                "--no-warn-script-location",
                "--index-url",
                PYPI_MIRROR_INDEX_URL,
                "--trusted-host",
                PYPI_MIRROR_TRUSTED_HOST,
                "--retries",
                "3",
                "--timeout",
                "30",
                source_archive,
            ],
            &pip_cache,
            "安装Windows微信CLI失败",
        ) {
            Ok(()) => {
                installed_source = source_archive;
                break;
            }
            Err(err) => install_errors.push(format!("{source_archive}：{err}")),
        }
    }
    if installed_source.is_empty() {
        return Err(format!(
            "安装Windows微信CLI失败，已尝试全部镜像源：{}",
            install_errors.join("\n")
        ));
    }

    let launcher = bin_dir.join("wechat-cli.cmd");
    let launcher_text = format!(
        "@echo off\r\nset \"IM_BOARD_WECHAT_CLI_ROOT=%~dp0..\"\r\nset \"PYTHONPATH=%IM_BOARD_WECHAT_CLI_ROOT%\\python-site;%PYTHONPATH%\"\r\n\"{}\" -m wechat_cli.main %*\r\n",
        python.to_string_lossy()
    );
    fs::write(&launcher, launcher_text)
        .map_err(|err| format!("无法写入微信CLI启动器 {}：{err}", launcher.display()))?;
    fs::write(
        install_root.join("package.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "name": "@canghe_ai/wechat-cli",
            "version": version,
            "source": installed_source,
            "pythonIndexUrl": PYPI_MIRROR_INDEX_URL,
            "runtime": "bundled-python"
        }))
        .map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("无法写入微信CLI元数据：{err}"))?;
    Ok(())
}

#[cfg(windows)]
fn bundled_windows_python(resource_dir: &Path) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok();
    [
        Some(resource_dir.join("runtime")),
        Some(resource_dir.join("_up_").join("runtime")),
        Some(resource_dir.join("..").join("runtime")),
        cwd.as_ref().map(|path| path.join("runtime")),
        cwd.as_ref().map(|path| path.join("..").join("runtime")),
    ]
    .into_iter()
    .flatten()
    .map(|root| {
        root.join("python")
            .join(PYTHON_STANDALONE_VERSION)
            .join("win-x64")
            .join("python")
            .join("python.exe")
    })
    .find(|path| path.exists())
}

#[cfg(windows)]
fn run_python_install_step(
    python: &Path,
    args: &[&str],
    pip_cache: &Path,
    context: &str,
) -> Result<(), String> {
    let output = std::process::Command::new(python)
        .args(args)
        .env("PIP_CACHE_DIR", pip_cache)
        .env("PIP_INDEX_URL", PYPI_MIRROR_INDEX_URL)
        .env("PIP_TRUSTED_HOST", PYPI_MIRROR_TRUSTED_HOST)
        .env("PIP_DEFAULT_TIMEOUT", "30")
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .map_err(|err| format!("{context}：无法启动内置 Python：{err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = [
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    ]
    .into_iter()
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>()
    .join("\n");
    Err(format!("{context}：{detail}"))
}

