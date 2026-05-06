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
    if cfg!(windows) {
        if spec.platform == "wechat" {
            candidates.push(root.join(spec.platform).join("bin").join("wechat-cli.cmd"));
        }
        if spec.platform == "wecom" {
            candidates.push(
                package_root
                    .join("@wecom")
                    .join("cli-win32-x64")
                    .join("bin")
                    .join("wecom-cli.exe"),
            );
        }
        if spec.platform == "feishu" {
            candidates.push(
                package_root
                    .join(package_dir(spec.package))
                    .join("bin")
                    .join("lark-cli-windows-x64.exe"),
            );
            candidates.push(
                package_root
                    .join(package_dir(spec.package))
                    .join("bin")
                    .join("lark-cli.exe"),
            );
        }
        if spec.platform == "dingtalk" {
            candidates.push(
                package_root
                    .join(package_dir(spec.package))
                    .join("vendor")
                    .join("dws-windows-x64.exe"),
            );
            candidates.push(
                package_root
                    .join(package_dir(spec.package))
                    .join("vendor")
                    .join("dws.exe"),
            );
        }
        candidates.push(package_root.join(".bin").join(format!("{}.cmd", spec.bin)));
        candidates.push(package_root.join(".bin").join(format!("{}.exe", spec.bin)));
        if spec.platform == "feishu" {
            candidates.push(
                package_root
                    .join(package_dir(spec.package))
                    .join("scripts")
                    .join("run.js"),
            );
        }
        if spec.platform == "dingtalk" {
            candidates.push(
                package_root
                    .join(package_dir(spec.package))
                    .join("bin")
                    .join("dws.js"),
            );
        }
        candidates.push(package_root.join(".bin").join(spec.bin));
        return candidates;
    }
    let npm_arch = current_npm_arch();
    if spec.platform == "wechat" {
        candidates.push(
            package_root
                .join(format!("@canghe_ai/wechat-cli-darwin-{npm_arch}"))
                .join("bin")
                .join("wechat-cli"),
        );
        candidates.push(
            package_root
                .join(package_dir(spec.package))
                .join("bin")
                .join("wechat-cli"),
        );
        candidates.push(
            package_root
                .join(package_dir(spec.package))
                .join("bin")
                .join("wechat-cli.js"),
        );
    }
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
        candidates.push(
            package_root
                .join(package_dir(spec.package))
                .join("scripts")
                .join("run.js"),
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
        candidates.push(
            package_root
                .join(package_dir(spec.package))
                .join("bin")
                .join("dws.js"),
        );
    }
    candidates.push(package_root.join(".bin").join(spec.bin));
    candidates
}

fn is_dependency_free_cli(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "js" | "cmd" | "bat") {
        return false;
    }
    if cfg!(windows) && extension != "exe" {
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

fn is_usable_cli_candidate(path: &Path, spec: &PlatformCliSpec) -> bool {
    is_dependency_free_cli(path)
        || (spec.platform == "wechat"
            && ((!cfg!(windows) && is_node_cli_entry(path))
                || is_windows_wechat_python_launcher(path)))
}

pub(super) fn is_node_cli_entry(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("js"))
}

fn is_windows_wechat_python_launcher(path: &Path) -> bool {
    cfg!(windows)
        && path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("wechat-cli.cmd"))
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("bin"))
        && path
            .parent()
            .and_then(|parent| parent.parent())
            .and_then(|parent| parent.file_name())
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("wechat"))
        && windows_wechat_launcher_python_exists(path)
}

fn windows_wechat_launcher_python_exists(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    // Windows微信CLI启动器会固定调用应用内置Python。App升级或迁移后，
    // AppData中旧启动器可能还在但内部Python路径失效，必须视为坏缓存并触发重装。
    text.lines()
        .find_map(extract_quoted_python_path)
        .is_some_and(|python| python.exists())
}

fn extract_quoted_python_path(line: &str) -> Option<PathBuf> {
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('"') else {
            return None;
        };
        let candidate = &rest[..end];
        if candidate.to_ascii_lowercase().ends_with("python.exe") {
            return Some(PathBuf::from(candidate));
        }
        rest = &rest[end + 1..];
    }
    None
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
    std::fs::create_dir_all(&install_root).map_err(|err| {
        format!(
            "无法创建{}官方CLI更新目录 {}：{err}",
            platform_label(spec.platform),
            install_root.display()
        )
    })?;
    Ok(install_root)
}

pub(super) fn package_dir(package: &str) -> PathBuf {
    package.split('/').collect()
}
