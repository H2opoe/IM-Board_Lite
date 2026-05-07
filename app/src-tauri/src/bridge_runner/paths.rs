use std::path::{Path, PathBuf};

use crate::storage::models::ImProfile;

use super::{BridgeRequest, APP_DATA_DIR_NAME};

pub(super) fn resolve_bridge_executable(resource_dir: &Path, request: &BridgeRequest) -> PathBuf {
    if let Some(profile) = &request.profile {
        if let Some(bridge_path) = profile
            .config_json
            .get("bridgePath")
            .and_then(|value| value.as_str())
        {
            let expanded = expand_home(bridge_path);
            if expanded.exists() {
                return expanded;
            }
        }
    }
    let platform = request.platform.as_str();
    let file_name = match platform {
        "wechat" => "bridge_main",
        "wecom" => "wecom-bridge",
        "feishu" => "feishu-bridge",
        "dingtalk" => "dingtalk-bridge",
        _ => "bridge_main",
    };
    if let Ok(root) = std::env::var("IM_BOARD_BRIDGE_ROOT") {
        let configured = PathBuf::from(root);
        if configured.is_absolute() {
            return configured.join(platform).join(file_name);
        }
        let cwd_candidate = std::env::current_dir()
            .unwrap_or_else(|_| resource_dir.to_path_buf())
            .join(&configured)
            .join(platform)
            .join(file_name);
        if cwd_candidate.exists() {
            return cwd_candidate;
        }
        let resource_candidate = resource_dir
            .join("..")
            .join(&configured)
            .join(platform)
            .join(file_name);
        if resource_candidate.exists() {
            return resource_candidate;
        }
        let tauri_parent_resource_candidate = resource_dir
            .join("_up_")
            .join(&configured)
            .join(platform)
            .join(file_name);
        if tauri_parent_resource_candidate.exists() {
            return tauri_parent_resource_candidate;
        }
        return configured.join(platform).join(file_name);
    }

    let candidates = [
        resource_dir.join("bridges").join(platform).join(file_name),
        resource_dir
            .join("_up_")
            .join("bridges")
            .join(platform)
            .join(file_name),
        resource_dir
            .join("..")
            .join("bridges")
            .join(platform)
            .join(file_name),
        std::env::current_dir()
            .unwrap_or_else(|_| resource_dir.to_path_buf())
            .join("bridges")
            .join(platform)
            .join(file_name),
    ];
    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| resource_dir.join("bridges").join(platform).join(file_name))
}

pub(super) fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

pub(super) fn resolve_official_cli_for_runtime(
    resource_dir: &Path,
    profile: Option<&ImProfile>,
    platform: &str,
    package: &str,
    bin: &str,
) -> Option<PathBuf> {
    let configured = profile
        .and_then(|profile| profile.config_json.get("cliPath"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty());
    // 官方CLI优先走应用后台热更新目录；用户配置路径只作为兜底，避免把开发机上的 node/npm 依赖带入打包版运行边界。
    resolve_hot_updated_official_cli(resource_dir, platform, package, bin).or_else(|| {
        configured
            .and_then(resolve_existing_command)
            .filter(|path| is_dependency_free_cli(path))
    })
}

fn resolve_hot_updated_official_cli(
    resource_dir: &Path,
    platform: &str,
    package: &str,
    bin: &str,
) -> Option<PathBuf> {
    official_cli_roots(resource_dir)
        .into_iter()
        .flat_map(|root| official_cli_candidates(&root, platform, package, bin))
        .find(|path| path.exists() && is_dependency_free_cli(path))
}

pub(super) fn official_cli_roots(resource_dir: &Path) -> Vec<PathBuf> {
    [
        std::env::var("IM_BOARD_OFFICIAL_CLI_ROOT")
            .ok()
            .map(PathBuf::from),
        dirs::data_dir().map(|path| path.join(APP_DATA_DIR_NAME).join("OfficialCli")),
        Some(resource_dir.join("official-cli")),
        Some(resource_dir.join("_up_").join("official-cli")),
        Some(resource_dir.join("..").join("official-cli")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn official_cli_candidates(root: &Path, platform: &str, package: &str, bin: &str) -> Vec<PathBuf> {
    let package_root = root.join(platform).join("node_modules");
    let package_dir = package.split('/').collect::<PathBuf>();
    let mut candidates = Vec::new();
    let npm_arch = current_npm_arch();
    match platform {
        "wecom" => candidates.push(
            package_root
                .join(format!("@wecom/cli-darwin-{npm_arch}"))
                .join("bin")
                .join("wecom-cli"),
        ),
        "feishu" => {
            candidates.push(
                package_root
                    .join(&package_dir)
                    .join("bin")
                    .join(format!("lark-cli-darwin-{npm_arch}")),
            );
            candidates.push(package_root.join(&package_dir).join("bin").join("lark-cli"));
        }
        "dingtalk" => {
            candidates.push(
                package_root
                    .join(&package_dir)
                    .join("vendor")
                    .join(format!("dws-darwin-{npm_arch}")),
            );
            candidates.push(package_root.join(&package_dir).join("vendor").join("dws"));
            candidates.push(package_root.join(&package_dir).join("bin").join("dws.js"));
        }
        _ => {}
    }
    candidates.push(package_root.join(".bin").join(bin));
    candidates
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

fn resolve_existing_command(command: &str) -> Option<PathBuf> {
    let expanded = expand_home(command);
    if expanded.is_absolute() || command.contains('\\') || command.contains('/') {
        return expanded.exists().then_some(expanded);
    }
    let path = std::env::var_os("PATH")?;
    let extensions: &[&str] = &[""];
    std::env::split_paths(&path).find_map(|entry| {
        extensions
            .iter()
            .map(|extension| entry.join(format!("{command}{extension}")))
            .find(|candidate| candidate.exists())
    })
}

pub(super) fn is_dependency_free_cli(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "js" | "cmd" | "bat") {
        return false;
    }
    if let Ok(bytes) = std::fs::read(path) {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(96)]).to_ascii_lowercase();
        if head.contains("/usr/bin/env node") || head.contains("node ") {
            return false;
        }
    }
    true
}
