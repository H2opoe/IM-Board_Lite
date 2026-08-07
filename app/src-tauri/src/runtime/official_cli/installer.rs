use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::archives::download_and_extract_tgz;
use super::npm_registry::{npm_metadata, resolve_npm_version, NpmVersionMetadata};
use super::resolver::package_dir;

pub(super) fn replace_install_root_atomically(
    install_root: &Path,
    staging_root: &Path,
) -> Result<(), String> {
    let parent = install_root
        .parent()
        .ok_or_else(|| format!("官方CLI热更新目录无效：{}", install_root.display()))?;
    let backup_root = parent.join(format!(
        ".{}-{}.bak",
        install_root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("cli"),
        uuid::Uuid::new_v4()
    ));
    fs::remove_dir_all(&backup_root).ok();
    if install_root.exists() {
        fs::rename(install_root, &backup_root)
            .map_err(|err| format!("无法备份旧官方CLI目录{}：{err}", install_root.display()))?;
    }
    if let Err(err) = fs::rename(staging_root, install_root) {
        if backup_root.exists() {
            let _ = fs::rename(&backup_root, install_root);
        }
        return Err(format!(
            "无法启用新的官方CLI目录{}：{err}",
            install_root.display()
        ));
    }
    fs::remove_dir_all(&backup_root).ok();
    Ok(())
}

pub fn remove_platform_cli_staging_dirs(app_dir: &Path, platform: &str) -> Result<(), String> {
    let install_root = app_dir.join("OfficialCli").join(platform);
    let Some(parent) = install_root.parent().map(Path::to_path_buf) else {
        return Ok(());
    };
    if !parent.exists() {
        return Ok(());
    }
    let temp_prefix = format!(".{platform}-");
    for entry in fs::read_dir(&parent)
        .map_err(|err| format!("读取官方CLI热更新目录{}失败：{err}", parent.display()))?
    {
        let entry = entry.map_err(|err| err.to_string())?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with(&temp_prefix)
            && (file_name.ends_with(".tmp") || file_name.ends_with(".bak"))
        {
            fs::remove_dir_all(entry.path()).map_err(|err| {
                format!("删除官方CLI临时目录{}失败：{err}", entry.path().display())
            })?;
        }
    }
    Ok(())
}

pub(super) async fn install_npm_package_tree(
    install_root: &Path,
    package: &str,
    version: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let mut installed = BTreeSet::<String>::new();
    let mut pending = vec![(package.to_owned(), version.to_owned())];
    while let Some((package_name, version_range)) = pending.pop() {
        let metadata = npm_metadata(&package_name, None).await?;
        let resolved_version = resolve_npm_version(&metadata, &version_range)
            .ok_or_else(|| format!("未找到满足{package_name}@{version_range}的官方CLI包版本。"))?;
        let key = format!("{package_name}@{resolved_version}");
        if !installed.insert(key) {
            continue;
        }
        let version_metadata = metadata
            .versions
            .get(&resolved_version)
            .cloned()
            .ok_or_else(|| format!("官方源缺少 {package_name}@{resolved_version} 元数据。"))?;
        let package_dir = install_root
            .join("node_modules")
            .join(package_dir(&version_metadata.name));
        download_and_extract_tgz(&client, &version_metadata.dist.tarball, &package_dir).await?;
        write_minimal_package_metadata(&package_dir, &version_metadata)?;
        for (dependency, dependency_range) in &version_metadata.dependencies {
            pending.push((dependency.clone(), dependency_range.clone()));
        }
        for optional in selected_optional_dependencies(&version_metadata) {
            pending.push(optional);
        }
    }
    Ok(())
}

fn write_minimal_package_metadata(
    package_dir: &Path,
    metadata: &NpmVersionMetadata,
) -> Result<(), String> {
    let package_json = package_dir.join("package.json");
    if package_json.exists() {
        return Ok(());
    }
    let value = serde_json::json!({
        "name": metadata.name,
        "version": metadata.version
    });
    fs::create_dir_all(package_dir).map_err(|err| err.to_string())?;
    fs::write(
        &package_json,
        serde_json::to_vec_pretty(&value).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("无法写入 {}：{err}", package_json.display()))
}

fn selected_optional_dependencies(metadata: &NpmVersionMetadata) -> Vec<(String, String)> {
    metadata
        .optional_dependencies
        .iter()
        .filter(|(name, _)| optional_dependency_matches_current_target(name))
        .map(|(name, version)| (name.clone(), version.clone()))
        .collect()
}

fn optional_dependency_matches_current_target(name: &str) -> bool {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        ""
    };
    let arch = current_npm_arch();
    let windows_marker = ["win", "32-"].concat();
    name.contains(&format!("{os}-{arch}"))
        || (!name.contains("darwin-")
            && !name.contains(&windows_marker)
            && !name.contains("linux-"))
}

fn current_npm_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        std::env::consts::ARCH
    }
}
