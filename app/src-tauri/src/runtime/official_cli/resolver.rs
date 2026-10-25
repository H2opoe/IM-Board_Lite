use std::path::{Path, PathBuf};

use super::{current_npm_arch, platform_label, PlatformCliSpec, OFFICIAL_CLIS};

pub fn cli_spec(platform: &str) -> Option<&'static PlatformCliSpec> {
    OFFICIAL_CLIS.iter().find(|spec| spec.platform == platform)
}

pub fn resolve_official_cli(
    resource_dir: &Path,
    app_dir: &Path,
    spec: &PlatformCliSpec,
) -> Option<PathBuf> {
    official_cli_roots(resource_dir, app_dir)
        .into_iter()
        .flat_map(|root| official_cli_candidates(&root, spec))
        .find(|path| path.exists() && is_usable_cli_candidate(path, spec))
}

fn official_cli_roots(resource_dir: &Path, app_dir: &Path) -> Vec<PathBuf> {
    [
        std::env::var("IM_BOARD_OFFICIAL_CLI_ROOT")
            .ok()
            .map(PathBuf::from),
        Some(app_dir.join("OfficialCli")),
        Some(resource_dir.join("official-cli")),
        Some(resource_dir.join("_up_").join("official-cli")),
        Some(resource_dir.join("..").join("official-cli")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn official_cli_candidates(root: &Path, spec: &PlatformCliSpec) -> Vec<PathBuf> {
    let package_root = root.join(spec.platform).join("node_modules");
    let mut candidates = Vec::new();
    let npm_arch = current_npm_arch();
    if spec.platform == "wecom" {
        candidates.push(
            package_root
                .join(format!("@wecom/cli-darwin-{npm_arch}"))
                .join("bin")
                .join("wecom-cli"),
        );
    }
    if spec.platform == "feishu" {
        candidates.push(
            package_root
                .join(package_dir(spec.package))
                .join("bin")
                .join(format!("lark-cli-darwin-{npm_arch}")),
        );
        candidates.push(
            package_root
                .join(package_dir(spec.package))
                .join("bin")
                .join("lark-cli"),
        );
    }
    if spec.platform == "dingtalk" {
        candidates.push(
            package_root
                .join(package_dir(spec.package))
                .join("vendor")
                .join(format!("dws-darwin-{npm_arch}")),
        );
        candidates.push(
            package_root
                .join(package_dir(spec.package))
                .join("vendor")
                .join("dws"),
        );
    }
    candidates.push(package_root.join(".bin").join(spec.bin));
    candidates
}

fn is_dependency_free_cli(path: &Path) -> bool {
    crate::runtime::cli_candidate::is_dependency_free_cli(path, false)
}

fn is_usable_cli_candidate(path: &Path, spec: &PlatformCliSpec) -> bool {
    is_dependency_free_cli(path)
        && (spec.platform != "dingtalk" || super::dws_bundle::verified(path))
}

pub fn official_cli_version(cli_path: &Path, spec: &PlatformCliSpec) -> Option<String> {
    let package_path = cli_package_json(cli_path, spec)?;
    let text = std::fs::read_to_string(package_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("version")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

fn cli_package_json(cli_path: &Path, spec: &PlatformCliSpec) -> Option<PathBuf> {
    if let Some(package_path) = cli_package_json_from_node_modules(cli_path, spec) {
        return Some(package_path);
    }
    for ancestor in cli_path.ancestors() {
        let package_path = ancestor.join("package.json");
        if let Ok(text) = std::fs::read_to_string(&package_path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if json.get("name").and_then(|value| value.as_str()) == Some(spec.package) {
                    return Some(package_path);
                }
            }
        }
    }
    let package_path = cli_path
        .parent()?
        .parent()?
        .join(package_dir(spec.package))
        .join("package.json");
    package_path.exists().then_some(package_path)
}

fn cli_package_json_from_node_modules(cli_path: &Path, spec: &PlatformCliSpec) -> Option<PathBuf> {
    for ancestor in cli_path.ancestors() {
        if ancestor.file_name().and_then(|value| value.to_str()) == Some("node_modules") {
            let package_path = ancestor
                .join(package_dir(spec.package))
                .join("package.json");
            if package_path.exists() {
                return Some(package_path);
            }
        }
    }
    None
}

pub fn writable_cli_install_root(
    app_dir: &Path,
    spec: &PlatformCliSpec,
) -> Result<PathBuf, String> {
    let install_root = app_dir.join("OfficialCli").join(spec.platform);
    std::fs::create_dir_all(install_root.parent().ok_or("官方CLI安装目录无效。")?).map_err(
        |err| {
            format!(
                "无法创建{}官方CLI更新目录 {}：{err}",
                platform_label(spec.platform),
                install_root.display()
            )
        },
    )?;
    Ok(install_root)
}

pub(super) fn package_dir(package: &str) -> PathBuf {
    package.split('/').collect()
}

#[cfg(test)]
mod pinned_dws_tests {
    use super::*;
    #[test]
    fn dws_cannot_bypass_the_pinned_runtime_with_a_native_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("dws");
        std::fs::write(&binary, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(is_dependency_free_cli(&binary));
        assert!(!is_usable_cli_candidate(
            &binary,
            cli_spec("dingtalk").unwrap()
        ));
    }
}

#[cfg(test)]
mod candidate_selection_tests {
    use super::*;

    #[test]
    fn invalid_preferred_entry_does_not_hide_the_next_native_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let spec = cli_spec("feishu").unwrap();
        let candidates = official_cli_candidates(dir.path(), spec);
        let invalid = &candidates[0];
        let usable = &candidates[1];
        std::fs::create_dir_all(invalid).unwrap();
        std::fs::write(usable, b"synthetic native fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(usable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let selected = candidates
            .iter()
            .find(|path| path.exists() && is_usable_cli_candidate(path, spec));
        assert_eq!(selected, Some(usable));
        std::fs::remove_file(usable).unwrap();
        std::fs::write(usable, b"").unwrap();
        assert!(candidates
            .iter()
            .all(|path| !is_usable_cli_candidate(path, spec)));
    }
}
