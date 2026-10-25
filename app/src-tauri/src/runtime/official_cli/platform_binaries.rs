fn ensure_wecom_binary(install_root: &Path) -> Result<(), String> {
    let binary_path = install_root
        .join("node_modules")
        .join(format!("@wecom/cli-darwin-{}", current_npm_arch()))
        .join("bin")
        .join("wecom-cli");
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
    let (archive_arch, target_arch) = if cfg!(target_arch = "aarch64") {
        ("arm64", "arm64")
    } else {
        ("amd64", "x64")
    };
    let binary_path = package_root
        .join("bin")
        .join(format!("lark-cli-darwin-{target_arch}"));
    if binary_path.exists() {
        return Ok(());
    }
    let archive_name = format!("lark-cli-{version}-darwin-{archive_arch}.tar.gz");
    let artifact = native_artifacts::feishu_asset(&archive_name)?;
    let urls = [
        format!("https://registry.npmmirror.com/-/binary/lark-cli/v{version}/{archive_name}"),
        artifact.url,
    ];
    let bytes = download_first_verified(&urls, &artifact.sha256).await?;
    let temp_dir = install_root.join(".tmp").join("lark-cli");
    fs::remove_dir_all(&temp_dir).ok();
    fs::create_dir_all(&temp_dir).map_err(|err| err.to_string())?;
    extract_tgz_bytes(&bytes, &temp_dir, false)?;
    let extracted = find_file_named(&temp_dir, "lark-cli")
        .ok_or_else(|| format!("lark-cli not found in {archive_name}"))?;
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
