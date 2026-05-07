fn ensure_wecom_binary(install_root: &Path) -> Result<(), String> {
    let binary_path = install_root
        .join("node_modules")
        .join(format!(
            "@wecom/cli-{}-{}",
            current_npm_os(),
            current_npm_arch()
        ))
        .join("bin")
        .join(native_binary_name("wecom-cli"));
    if !binary_path.exists() {
        return Err(format!(
            "企业微信官方CLI已下载，但缺少无需用户依赖的原生执行文件：{}",
            binary_path.display()
        ));
    }
    make_executable(&binary_path)
}

async fn ensure_feishu_binary(install_root: &Path, version: &str) -> Result<(), String> {
    let package_root = install_root
        .join("node_modules")
        .join("@larksuite")
        .join("cli");
    let (platform, archive_arch, archive_ext) = if cfg!(windows) {
        if cfg!(target_arch = "aarch64") {
            ("windows", "arm64", "zip")
        } else {
            ("windows", "amd64", "zip")
        }
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        ("darwin", "arm64", "tar.gz")
    } else if cfg!(target_os = "macos") {
        ("darwin", "amd64", "tar.gz")
    } else {
        return Err(format!(
            "当前平台暂不支持包内飞书官方CLI原生入口：{}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    };
    let binary_name = native_binary_name("lark-cli");
    let binary_path = package_root.join("bin").join(&binary_name);
    if binary_path.exists() {
        return Ok(());
    }
    let archive_name = format!("lark-cli-{version}-{platform}-{archive_arch}.{archive_ext}");
    let urls = [
        format!("https://registry.npmmirror.com/-/binary/lark-cli/v{version}/{archive_name}"),
        format!("https://github.com/larksuite/cli/releases/download/v{version}/{archive_name}"),
    ];
    let bytes = download_first_available(&urls).await?;
    let temp_dir = install_root.join(".tmp").join("lark-cli");
    fs::remove_dir_all(&temp_dir).ok();
    fs::create_dir_all(&temp_dir).map_err(|err| err.to_string())?;
    if archive_ext == "zip" {
        extract_zip_bytes(&bytes, &temp_dir)?;
    } else {
        extract_tgz_bytes(&bytes, &temp_dir, false)?;
    }
    let extracted = find_file_named(&temp_dir, &binary_name)
        .ok_or_else(|| format!("{binary_name} not found in {archive_name}"))?;
    fs::create_dir_all(binary_path.parent().unwrap()).map_err(|err| err.to_string())?;
    fs::copy(&extracted, &binary_path).map_err(|err| {
        format!(
            "无法安装飞书官方CLI可执行文件 {}：{err}",
            binary_path.display()
        )
    })?;
    make_executable(&binary_path)?;
    fs::remove_dir_all(&temp_dir).ok();
    Ok(())
}

fn ensure_dingtalk_binary(install_root: &Path) -> Result<(), String> {
    let package_root = install_root
        .join("node_modules")
        .join("dingtalk-workspace-cli");
    let (platform, archive_arch, archive_ext) = if cfg!(windows) {
        if cfg!(target_arch = "aarch64") {
            ("windows", "arm64", "zip")
        } else {
            ("windows", "amd64", "zip")
        }
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        ("darwin", "arm64", "tar.gz")
    } else if cfg!(target_os = "macos") {
        ("darwin", "amd64", "tar.gz")
    } else {
        return Err(format!(
            "当前平台暂不支持包内钉钉官方CLI原生入口：{}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    };
    let binary_name = native_binary_name("dws");
    let binary_path = package_root.join("vendor").join(&binary_name);
    if binary_path.exists() {
        return Ok(());
    }
    let archive_path = package_root
        .join("assets")
        .join(format!("dws-{platform}-{archive_arch}.{archive_ext}"));
    let bytes = fs::read(&archive_path)
        .map_err(|err| format!("无法读取钉钉官方CLI资源 {}：{err}", archive_path.display()))?;
    let temp_dir = install_root.join(".tmp").join("dws");
    fs::remove_dir_all(&temp_dir).ok();
    fs::create_dir_all(&temp_dir).map_err(|err| err.to_string())?;
    if archive_ext == "zip" {
        extract_zip_bytes(&bytes, &temp_dir)?;
    } else {
        extract_tgz_bytes(&bytes, &temp_dir, false)?;
    }
    let extracted = find_file_named(&temp_dir, &binary_name)
        .ok_or_else(|| format!("{binary_name} not found in {}", archive_path.display()))?;
    fs::create_dir_all(binary_path.parent().unwrap()).map_err(|err| err.to_string())?;
    fs::copy(&extracted, &binary_path).map_err(|err| {
        format!(
            "无法安装钉钉官方CLI可执行文件 {}：{err}",
            binary_path.display()
        )
    })?;
    make_executable(&binary_path)?;
    fs::remove_dir_all(&temp_dir).ok();
    Ok(())
}
