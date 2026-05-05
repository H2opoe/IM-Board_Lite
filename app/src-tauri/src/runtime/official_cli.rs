use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use serde::Serialize;
use tauri::Emitter;

#[derive(Clone, Copy)]
pub struct PlatformCliSpec {
    pub platform: &'static str,
    pub package: &'static str,
    pub bin: &'static str,
    pub source: &'static str,
}

pub const OFFICIAL_CLIS: &[PlatformCliSpec] = &[
    PlatformCliSpec {
        platform: "wechat",
        package: "@canghe_ai/wechat-cli",
        bin: "wechat-cli",
        source: "https://github.com/huohuoer/wechat-cli",
    },
    PlatformCliSpec {
        platform: "wecom",
        package: "@wecom/cli",
        bin: "wecom-cli",
        source: "https://github.com/WecomTeam/wecom-cli",
    },
    PlatformCliSpec {
        platform: "feishu",
        package: "@larksuite/cli",
        bin: "lark-cli",
        source: "https://github.com/larksuite/cli",
    },
    PlatformCliSpec {
        platform: "dingtalk",
        package: "dingtalk-workspace-cli",
        bin: "dws",
        source: "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
    },
];

const NPM_REGISTRIES: &[&str] = &[
    "https://registry.npmmirror.com",
    "https://registry.npmjs.org",
];
#[cfg(windows)]
const WECHAT_CLI_SOURCE_ARCHIVES: &[&str] = &[
    "https://gh-proxy.com/https://github.com/huohuoer/wechat-cli/archive/refs/heads/main.zip",
    "https://gh.llkk.cc/https://github.com/huohuoer/wechat-cli/archive/refs/heads/main.zip",
    "https://codeload.github.com/huohuoer/wechat-cli/zip/refs/heads/main",
    "https://github.com/huohuoer/wechat-cli/archive/refs/heads/main.zip",
];
#[cfg(windows)]
const PYPI_MIRROR_INDEX_URL: &str = "https://pypi.tuna.tsinghua.edu.cn/simple";
#[cfg(windows)]
const PYPI_MIRROR_TRUSTED_HOST: &str = "pypi.tuna.tsinghua.edu.cn";
#[cfg(windows)]
const PYTHON_STANDALONE_VERSION: &str = "20260414";

const PLATFORM_CLI_PROGRESS_EVENT: &str = "platform-cli-deployment-progress";
const WINDOWS_DINGTALK_REGISTRY_KEY: &str = r"HKCU:\Software\DwsCli\keychain\dws-cli";
const WINDOWS_DINGTALK_AUTH_TOKEN_VALUE: &str = "YXV0aC10b2tlbg";
const WINDOWS_DINGTALK_PROFILE_TOKEN_FILE: &str = "windows-auth-token.regvalue";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformDeployment {
    pub platform: String,
    pub cli_path: String,
    pub config_dir: String,
    pub command: String,
    pub source: String,
    pub current_version: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCliVersionStatus {
    pub platform: String,
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub source: String,
    pub checked_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformCliDeploymentProgress {
    platform: String,
    profile_id: String,
    phase: String,
    message: String,
    current: i64,
    total: i64,
    registry: Option<String>,
    package_name: Option<String>,
    version: Option<String>,
}

pub struct PlatformCliProgressContext<'a> {
    pub app: &'a tauri::AppHandle,
    pub platform: &'a str,
    pub profile_id: &'a str,
}

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

fn is_node_cli_entry(path: &Path) -> bool {
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
    // Windows 微信 CLI 启动器会固定调用应用内置 Python。App 升级或迁移后，
    // AppData 中旧启动器可能还在但内部 Python 路径失效，必须视为坏缓存并触发重装。
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
            "无法创建{}官方 CLI 更新目录 {}：{err}",
            platform_label(spec.platform),
            install_root.display()
        )
    })?;
    Ok(install_root)
}

fn package_dir(package: &str) -> PathBuf {
    package.split('/').collect()
}

#[derive(Debug, serde::Deserialize)]
struct NpmPackageMetadata {
    #[serde(rename = "dist-tags")]
    dist_tags: BTreeMap<String, String>,
    versions: BTreeMap<String, NpmVersionMetadata>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct NpmVersionMetadata {
    name: String,
    version: String,
    dist: NpmDistMetadata,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(rename = "optionalDependencies", default)]
    optional_dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct NpmDistMetadata {
    tarball: String,
}

pub async fn npm_latest_version(
    package: &str,
    progress: Option<&PlatformCliProgressContext<'_>>,
) -> Result<String, String> {
    let metadata = npm_metadata(package, progress).await?;
    metadata
        .dist_tags
        .get("latest")
        .cloned()
        .or_else(|| metadata.versions.keys().next_back().cloned())
        .ok_or_else(|| "官方源未返回版本号。".to_owned())
}

async fn npm_metadata(
    package: &str,
    progress: Option<&PlatformCliProgressContext<'_>>,
) -> Result<NpmPackageMetadata, String> {
    let client = reqwest::Client::new();
    let mut last_error = String::new();
    for (index, registry) in NPM_REGISTRIES.iter().enumerate() {
        if let Some(progress) = progress {
            emit_platform_cli_progress(
                progress,
                "checking_remote",
                format!("正在从镜像源核查 {} 最新版本...", package),
                2,
                5,
                Some(registry),
                Some(package),
                None,
            );
        }
        let url = format!(
            "{}/{}",
            registry.trim_end_matches('/'),
            npm_package_url(package)
        );
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                match response.json::<NpmPackageMetadata>().await {
                    Ok(metadata) => {
                        if let Some(version) = metadata.dist_tags.get("latest") {
                            if let Some(progress) = progress {
                                emit_platform_cli_progress(
                                    progress,
                                    "version_resolved",
                                    format!("已确认 {} 最新版本 v{}。", package, version),
                                    2,
                                    5,
                                    Some(registry),
                                    Some(package),
                                    Some(version),
                                );
                            }
                        }
                        return Ok(metadata);
                    }
                    Err(err) => {
                        last_error = format!("{registry}：解析版本信息失败：{err}");
                    }
                }
            }
            Ok(response) => {
                last_error = format!("{registry}：HTTP {}", response.status());
            }
            Err(err) => {
                last_error = format!("{registry}：{err}");
            }
        }
        if let Some(progress) = progress {
            let has_fallback = index + 1 < NPM_REGISTRIES.len();
            emit_platform_cli_progress(
                progress,
                "checking_remote_retry",
                if has_fallback {
                    format!("镜像源暂不可用，正在切换备用源核查 {}。", package)
                } else {
                    format!("{} 版本核查失败。", package)
                },
                2,
                5,
                Some(registry),
                Some(package),
                None,
            );
        }
    }
    Err(format!("核查官方 CLI 最新版本失败：{last_error}"))
}

pub async fn install_package(
    install_root: &Path,
    package: &str,
    version: &str,
    resource_dir: &Path,
    progress: Option<&PlatformCliProgressContext<'_>>,
) -> Result<(), String> {
    #[cfg(not(windows))]
    let _ = resource_dir;
    if let Some(progress) = progress {
        emit_platform_cli_progress(
            progress,
            "installing",
            format!("正在后台更新 {}@{}...", package, version),
            3,
            5,
            None,
            Some(package),
            Some(version),
        );
    }
    let parent = install_root
        .parent()
        .ok_or_else(|| format!("官方 CLI 热更新目录无效：{}", install_root.display()))?;
    fs::create_dir_all(parent).map_err(|err| {
        format!(
            "无法创建{}官方 CLI 热更新目录 {}：{err}",
            platform_label(package),
            parent.display()
        )
    })?;
    let staging_root = parent.join(format!(
        ".{}-{}.tmp",
        install_root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("cli"),
        uuid::Uuid::new_v4()
    ));
    fs::remove_dir_all(&staging_root).ok();
    fs::create_dir_all(&staging_root).map_err(|err| {
        format!(
            "无法创建{}官方 CLI 临时热更新目录 {}：{err}",
            platform_label(package),
            staging_root.display()
        )
    })?;
    let install_result = async {
        #[cfg(windows)]
        {
            if package == "@canghe_ai/wechat-cli" {
                return install_windows_wechat_python_cli(
                    &staging_root,
                    version,
                    resource_dir,
                    progress,
                )
                .await;
            }
        }
        install_npm_package_tree(&staging_root, package, version).await?;
        ensure_platform_binary(&staging_root, package, version).await
    }
    .await;
    if let Err(err) = install_result {
        fs::remove_dir_all(&staging_root).ok();
        return Err(err);
    }
    replace_install_root_atomically(install_root, &staging_root)?;
    if let Some(progress) = progress {
        emit_platform_cli_progress(
            progress,
            "installed",
            format!("{}@{} 后台更新完成。", package, version),
            4,
            5,
            None,
            Some(package),
            Some(version),
        );
    }
    Ok(())
}

pub async fn ensure_platform_cli_ready(
    resource_dir: &Path,
    app_dir: &Path,
    spec: &PlatformCliSpec,
    progress: Option<&PlatformCliProgressContext<'_>>,
) -> Result<PathBuf, String> {
    let existing = resolve_official_cli(resource_dir, app_dir, spec);
    let current_version = existing
        .as_ref()
        .and_then(|path| official_cli_version(path, spec));
    let latest_version = match npm_latest_version(spec.package, progress).await {
        Ok(version) => version,
        Err(err) => {
            if let Some(path) = existing {
                if let Some(progress) = progress {
                    emit_platform_cli_progress(
                        progress,
                        "local_ready",
                        format!(
                            "远程版本核查失败，继续使用已准备好的 {} 官方 CLI。",
                            platform_label(spec.platform)
                        ),
                        5,
                        5,
                        None,
                        Some(spec.package),
                        current_version.as_deref(),
                    );
                }
                return Ok(path);
            }
            return Err(err);
        }
    };
    if let (Some(path), Some(current)) = (&existing, &current_version) {
        if !version_is_newer(&latest_version, current) {
            if let Some(progress) = progress {
                emit_platform_cli_progress(
                    progress,
                    "local_ready",
                    format!(
                        "已找到最新版 {} 官方 CLI v{}。",
                        platform_label(spec.platform),
                        current
                    ),
                    5,
                    5,
                    None,
                    Some(spec.package),
                    Some(current),
                );
            }
            return Ok(path.clone());
        }
    }

    let install_root = writable_cli_install_root(app_dir, spec)?;
    install_package(
        &install_root,
        spec.package,
        &latest_version,
        resource_dir,
        progress,
    )
    .await?;
    if let Some(progress) = progress {
        emit_platform_cli_progress(
            progress,
            "locating",
            format!(
                "正在定位 {} 官方 CLI 可执行入口...",
                platform_label(spec.platform)
            ),
            4,
            5,
            None,
            Some(spec.package),
            Some(&latest_version),
        );
    }
    let path = resolve_official_cli(resource_dir, app_dir, spec).ok_or_else(|| {
        format!(
            "{} 官方 CLI 已准备完成，但未找到无需用户依赖的可执行入口。",
            platform_label(spec.platform)
        )
    })?;
    if let Some(progress) = progress {
        emit_platform_cli_progress(
            progress,
            "ready",
            format!("{} 官方 CLI 已准备完成。", platform_label(spec.platform)),
            5,
            5,
            None,
            Some(spec.package),
            Some(&latest_version),
        );
    }
    Ok(path)
}

fn replace_install_root_atomically(install_root: &Path, staging_root: &Path) -> Result<(), String> {
    let parent = install_root
        .parent()
        .ok_or_else(|| format!("官方 CLI 热更新目录无效：{}", install_root.display()))?;
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
            .map_err(|err| format!("无法备份旧官方 CLI 目录 {}：{err}", install_root.display()))?;
    }
    if let Err(err) = fs::rename(staging_root, install_root) {
        if backup_root.exists() {
            let _ = fs::rename(&backup_root, install_root);
        }
        return Err(format!(
            "无法启用新的官方 CLI 目录 {}：{err}",
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
        .map_err(|err| format!("读取官方 CLI 热更新目录 {} 失败：{err}", parent.display()))?
    {
        let entry = entry.map_err(|err| err.to_string())?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with(&temp_prefix)
            && (file_name.ends_with(".tmp") || file_name.ends_with(".bak"))
        {
            fs::remove_dir_all(entry.path()).map_err(|err| {
                format!(
                    "删除官方 CLI 临时目录 {} 失败：{err}",
                    entry.path().display()
                )
            })?;
        }
    }
    Ok(())
}

async fn install_npm_package_tree(
    install_root: &Path,
    package: &str,
    version: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let mut installed = BTreeSet::<String>::new();
    let mut pending = vec![(package.to_owned(), version.to_owned())];
    while let Some((package_name, version_range)) = pending.pop() {
        let metadata = npm_metadata(&package_name, None).await?;
        let resolved_version = resolve_npm_version(&metadata, &version_range).ok_or_else(|| {
            format!("未找到满足 {package_name}@{version_range} 的官方 CLI 包版本。")
        })?;
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
    let os = if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        ""
    };
    let arch = current_npm_arch();
    name.contains(&format!("{os}-{arch}"))
        || (!name.contains("darwin-") && !name.contains("win32-") && !name.contains("linux-"))
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

async fn ensure_platform_binary(
    install_root: &Path,
    package: &str,
    version: &str,
) -> Result<(), String> {
    match package {
        "@canghe_ai/wechat-cli" => {
            #[cfg(windows)]
            {
                let _ = (install_root, version);
                Ok(())
            }
            #[cfg(not(windows))]
            {
                ensure_wechat_binary(install_root, version).await
            }
        }
        "@wecom/cli" => ensure_wecom_binary(install_root),
        "@larksuite/cli" => ensure_feishu_binary(install_root, version).await,
        "dingtalk-workspace-cli" => ensure_dingtalk_binary(install_root),
        _ => Ok(()),
    }
}

#[cfg(windows)]
async fn install_windows_wechat_python_cli(
    install_root: &Path,
    version: &str,
    resource_dir: &Path,
    progress: Option<&PlatformCliProgressContext<'_>>,
) -> Result<(), String> {
    let python = bundled_windows_python(resource_dir).ok_or_else(|| {
        "微信 CLI 需要应用内置 Windows Python 运行时，但当前安装包缺少 runtime/python。请使用重新打包后的 IM-Board。".to_owned()
    })?;
    let site_dir = install_root.join("python-site");
    let bin_dir = install_root.join("bin");
    let pip_cache = install_root.join("pip-cache");
    fs::create_dir_all(&site_dir).map_err(|err| {
        format!(
            "无法创建微信 CLI Python 依赖目录 {}：{err}",
            site_dir.display()
        )
    })?;
    fs::create_dir_all(&bin_dir)
        .map_err(|err| format!("无法创建微信 CLI 启动目录 {}：{err}", bin_dir.display()))?;
    fs::create_dir_all(&pip_cache).map_err(|err| {
        format!(
            "无法创建微信 CLI pip 缓存目录 {}：{err}",
            pip_cache.display()
        )
    })?;

    if let Some(progress) = progress {
        emit_platform_cli_progress(
            progress,
            "installing",
            "正在用应用内置 Python 准备 Windows 微信 CLI...".to_owned(),
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
            "安装 Windows 微信 CLI 失败",
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
            "安装 Windows 微信 CLI 失败，已尝试全部镜像源：{}",
            install_errors.join("\n")
        ));
    }

    let launcher = bin_dir.join("wechat-cli.cmd");
    let launcher_text = format!(
        "@echo off\r\nset \"IM_BOARD_WECHAT_CLI_ROOT=%~dp0..\"\r\nset \"PYTHONPATH=%IM_BOARD_WECHAT_CLI_ROOT%\\python-site;%PYTHONPATH%\"\r\n\"{}\" -m wechat_cli.main %*\r\n",
        python.to_string_lossy()
    );
    fs::write(&launcher, launcher_text)
        .map_err(|err| format!("无法写入微信 CLI 启动器 {}：{err}", launcher.display()))?;
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
    .map_err(|err| format!("无法写入微信 CLI 元数据：{err}"))?;
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

#[cfg(not(windows))]
async fn ensure_wechat_binary(install_root: &Path, version: &str) -> Result<(), String> {
    let binary_path = install_root
        .join("node_modules")
        .join(format!(
            "@canghe_ai/wechat-cli-darwin-{}",
            current_npm_arch()
        ))
        .join("bin")
        .join("wechat-cli");
    if binary_path.exists() {
        make_executable(&binary_path)?;
        return Ok(());
    }
    {
        let js_path = install_root
            .join("node_modules")
            .join(package_dir("@canghe_ai/wechat-cli"))
            .join("bin")
            .join("wechat-cli.js");
        if js_path.exists() {
            make_executable(&js_path)?;
            return Ok(());
        }
    }
    let package_name = format!("@canghe_ai/wechat-cli-darwin-{}", current_npm_arch());
    let package_name = package_name.as_str();
    install_npm_package_tree(install_root, package_name, version)
        .await
        .map_err(|err| {
            format!(
                "{} 官方源尚未发布当前系统可用的原生执行包 {}@{}，IM-Board 不能依赖用户安装 Node.js 或 npm 来运行微信 CLI。上游发布后可直接在应用内热更新。原始错误：{err}",
                platform_label("wechat"),
                package_name,
                version
            )
        })?;
    if !binary_path.exists() {
        {
            let js_path = install_root
                .join("node_modules")
                .join(package_dir("@canghe_ai/wechat-cli"))
                .join("bin")
                .join("wechat-cli.js");
            if js_path.exists() {
                make_executable(&js_path)?;
                return Ok(());
            }
        }
        return Err(format!(
            "微信官方 CLI 已下载，但缺少无需用户依赖的原生执行文件：{}",
            binary_path.display()
        ));
    }
    make_executable(&binary_path)
}

fn ensure_wecom_binary(install_root: &Path) -> Result<(), String> {
    let binary_path = if cfg!(windows) {
        install_root
            .join("node_modules")
            .join("@wecom")
            .join("cli-win32-x64")
            .join("bin")
            .join("wecom-cli.exe")
    } else {
        install_root
            .join("node_modules")
            .join(format!("@wecom/cli-darwin-{}", current_npm_arch()))
            .join("bin")
            .join("wecom-cli")
    };
    if !binary_path.exists() {
        return Err(format!(
            "企业微信官方 CLI 已下载，但缺少无需用户依赖的原生执行文件：{}",
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
    let (platform, archive_arch, target_arch, binary_name, archive_ext) = if cfg!(windows) {
        ("windows", "amd64", "x64", "lark-cli.exe", "zip")
    } else if cfg!(target_arch = "aarch64") {
        ("darwin", "arm64", "arm64", "lark-cli", "tar.gz")
    } else {
        ("darwin", "amd64", "x64", "lark-cli", "tar.gz")
    };
    let binary_path = package_root.join("bin").join(format!(
        "lark-cli-{platform}-{target_arch}{}",
        if cfg!(windows) { ".exe" } else { "" }
    ));
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
    let extracted = find_file_named(&temp_dir, binary_name)
        .ok_or_else(|| format!("{binary_name} not found in {archive_name}"))?;
    fs::create_dir_all(binary_path.parent().unwrap()).map_err(|err| err.to_string())?;
    fs::copy(&extracted, &binary_path).map_err(|err| {
        format!(
            "无法安装飞书官方 CLI 可执行文件 {}：{err}",
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
    let (platform, archive_arch, target_arch, binary_name, archive_ext) = if cfg!(windows) {
        ("windows", "amd64", "x64", "dws.exe", "zip")
    } else if cfg!(target_arch = "aarch64") {
        ("darwin", "arm64", "arm64", "dws", "tar.gz")
    } else {
        ("darwin", "amd64", "x64", "dws", "tar.gz")
    };
    let binary_path = package_root.join("vendor").join(format!(
        "dws-{platform}-{target_arch}{}",
        if cfg!(windows) { ".exe" } else { "" }
    ));
    if binary_path.exists() {
        return Ok(());
    }
    let archive_path = package_root
        .join("assets")
        .join(format!("dws-{platform}-{archive_arch}.{archive_ext}"));
    let bytes = fs::read(&archive_path).map_err(|err| {
        format!(
            "无法读取钉钉官方 CLI 资源 {}：{err}",
            archive_path.display()
        )
    })?;
    let temp_dir = install_root.join(".tmp").join("dws");
    fs::remove_dir_all(&temp_dir).ok();
    fs::create_dir_all(&temp_dir).map_err(|err| err.to_string())?;
    if archive_ext == "zip" {
        extract_zip_bytes(&bytes, &temp_dir)?;
    } else {
        extract_tgz_bytes(&bytes, &temp_dir, false)?;
    }
    let extracted = find_file_named(&temp_dir, binary_name)
        .ok_or_else(|| format!("{binary_name} not found in {}", archive_path.display()))?;
    fs::create_dir_all(binary_path.parent().unwrap()).map_err(|err| err.to_string())?;
    fs::copy(&extracted, &binary_path).map_err(|err| {
        format!(
            "无法安装钉钉官方 CLI 可执行文件 {}：{err}",
            binary_path.display()
        )
    })?;
    make_executable(&binary_path)?;
    fs::remove_dir_all(&temp_dir).ok();
    Ok(())
}

async fn download_and_extract_tgz(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
) -> Result<(), String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("下载官方 CLI 包失败：{err}"))?;
    if !response.status().is_success() {
        return Err(format!("下载官方 CLI 包失败：HTTP {}", response.status()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("读取官方 CLI 包失败：{err}"))?;
    fs::remove_dir_all(destination).ok();
    fs::create_dir_all(destination).map_err(|err| err.to_string())?;
    extract_tgz_bytes(&bytes, destination, true)
}

async fn download_first_available(urls: &[String]) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::new();
    let mut last_error = String::new();
    for url in urls {
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                return response
                    .bytes()
                    .await
                    .map(|bytes| bytes.to_vec())
                    .map_err(|err| format!("读取下载内容失败：{err}"));
            }
            Ok(response) => last_error = format!("{url}: HTTP {}", response.status()),
            Err(err) => last_error = format!("{url}: {err}"),
        }
    }
    Err(format!("下载官方 CLI 可执行文件失败：{last_error}"))
}

fn extract_tgz_bytes(
    bytes: &[u8],
    destination: &Path,
    strip_package_prefix: bool,
) -> Result<(), String> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|err| format!("读取 tgz 失败：{err}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|err| format!("读取 tgz 条目失败：{err}"))?;
        let raw_path = entry.path().map_err(|err| err.to_string())?.to_path_buf();
        let relative = safe_archive_path(&raw_path, strip_package_prefix)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let output = destination.join(relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        entry
            .unpack(&output)
            .map_err(|err| format!("解压官方 CLI 文件 {} 失败：{err}", output.display()))?;
    }
    Ok(())
}

fn extract_zip_bytes(bytes: &[u8], destination: &Path) -> Result<(), String> {
    let reader = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|err| format!("读取 zip 失败：{err}"))?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|err| err.to_string())?;
        let Some(path) = file.enclosed_name().map(|path| path.to_path_buf()) else {
            continue;
        };
        let output = destination.join(path);
        if file.is_dir() {
            fs::create_dir_all(&output).map_err(|err| err.to_string())?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let mut output_file = fs::File::create(&output).map_err(|err| err.to_string())?;
        std::io::copy(&mut file, &mut output_file).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn safe_archive_path(path: &Path, strip_package_prefix: bool) -> Result<PathBuf, String> {
    let mut output = PathBuf::new();
    for (index, component) in path.components().enumerate() {
        if strip_package_prefix && index == 0 {
            continue;
        }
        match component {
            Component::Normal(value) => output.push(value),
            Component::CurDir => {}
            _ => return Err(format!("官方 CLI 包含不安全路径：{}", path.display())),
        }
    }
    Ok(output)
}

fn find_file_named(root: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().and_then(|value| value.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_file_named(&path, name) {
                return Some(found);
            }
        }
    }
    None
}

fn make_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)
            .map_err(|err| err.to_string())?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn npm_package_url(package: &str) -> String {
    package.replace('/', "%2F")
}

fn resolve_npm_version(metadata: &NpmPackageMetadata, requirement: &str) -> Option<String> {
    if requirement == "latest" || requirement == "*" {
        return metadata.dist_tags.get("latest").cloned();
    }
    if metadata.versions.contains_key(requirement) {
        return Some(requirement.to_owned());
    }
    metadata
        .versions
        .keys()
        .filter(|version| version_satisfies(version, requirement))
        .max_by(|left, right| compare_versions(left, right))
        .cloned()
}

fn version_satisfies(version: &str, requirement: &str) -> bool {
    let requirement = requirement.trim();
    if requirement == "*" || requirement.is_empty() {
        return true;
    }
    let version_parts = parse_version(version);
    let base = parse_version(requirement.trim_start_matches(['^', '~', '=']));
    if base.is_empty() {
        return false;
    }
    if requirement.starts_with('^') {
        return version_parts >= base && version_parts.first() == base.first();
    }
    if requirement.starts_with('~') {
        return version_parts >= base
            && version_parts.first() == base.first()
            && version_parts.get(1) == base.get(1);
    }
    version == requirement
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    parse_version(left).cmp(&parse_version(right))
}

pub fn emit_platform_cli_progress(
    context: &PlatformCliProgressContext<'_>,
    phase: &str,
    message: String,
    current: i64,
    total: i64,
    registry: Option<&str>,
    package_name: Option<&str>,
    version: Option<&str>,
) {
    let _ = context.app.emit(
        PLATFORM_CLI_PROGRESS_EVENT,
        PlatformCliDeploymentProgress {
            platform: context.platform.to_owned(),
            profile_id: context.profile_id.to_owned(),
            phase: phase.to_owned(),
            message,
            current,
            total,
            registry: registry.map(str::to_owned),
            package_name: package_name.map(str::to_owned),
            version: version.map(str::to_owned),
        },
    );
}

pub fn default_bind_command(platform: &str, cli_path: &Path, config_dir: &Path) -> String {
    if cfg!(windows) {
        return default_bind_command_windows(platform, cli_path, config_dir);
    }
    match platform {
        "wechat" => {
            let cli_invocation = shell_cli_invocation(cli_path);
            format!(
                "mkdir -p '{}' && {} init --config '{}/config.json' --keys-file '{}/all_keys.json' --force",
                shell_single_quote(config_dir),
                cli_invocation,
                shell_single_quote(config_dir),
                shell_single_quote(config_dir)
            )
        }
        "wecom" => format!(
            "WECOM_CLI_CONFIG_DIR='{}' '{}' init",
            shell_single_quote(config_dir),
            shell_single_quote(cli_path)
        ),
        "feishu" => {
            let lark_config_dir = config_dir.join(".lark-cli");
            let required_scopes = [
                "search:message",
                "im:chat:read",
                "im:message:readonly",
                "im:message.p2p_msg:get_as_user",
                "im:message.group_msg:get_as_user",
                "contact:user.base:readonly",
                "contact:user.basic_profile:readonly",
            ]
            .join(" ");
            let profile_name = config_dir
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|value| value.to_str())
                .unwrap_or("im-board-feishu");
            format!(
                "LARKSUITE_CLI_CONFIG_DIR='{}' '{}' config init --new --brand feishu --name '{}' && LARKSUITE_CLI_CONFIG_DIR='{}' '{}' --profile '{}' auth login --scope '{}' && LARKSUITE_CLI_CONFIG_DIR='{}' '{}' --profile '{}' auth status --verify",
                shell_single_quote(&lark_config_dir),
                shell_single_quote(cli_path),
                shell_single_quote_text(profile_name),
                shell_single_quote(&lark_config_dir),
                shell_single_quote(cli_path),
                shell_single_quote_text(profile_name),
                shell_single_quote_text(&required_scopes),
                shell_single_quote(&lark_config_dir),
                shell_single_quote(cli_path),
                shell_single_quote_text(profile_name)
            )
        }
        "dingtalk" => {
            let cache_dir = config_dir.join("cache");
            let keychain_dir = config_dir.join("keychain");
            let auth_identity = config_dir
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|value| value.to_str())
                .unwrap_or("im-board-dingtalk");
            format!(
                "DWS_CONFIG_DIR='{}' DWS_CACHE_DIR='{}' DWS_KEYCHAIN_DIR='{}' DWS_AUTH_IDENTITY='{}' DWS_TENANT='{}' DINGTALK_DWS_AGENTCODE='{}' '{}' auth login --force &&\nDWS_CONFIG_DIR='{}' DWS_CACHE_DIR='{}' DWS_KEYCHAIN_DIR='{}' DWS_AUTH_IDENTITY='{}' DWS_TENANT='{}' DINGTALK_DWS_AGENTCODE='{}' '{}' auth status --format json &&\nDWS_CONFIG_DIR='{}' DWS_CACHE_DIR='{}' DWS_KEYCHAIN_DIR='{}' DWS_AUTH_IDENTITY='{}' DWS_TENANT='{}' DINGTALK_DWS_AGENTCODE='{}' '{}' pat chmod chat.message:list --agentCode '{}' --grant-type permanent &&\nDWS_CONFIG_DIR='{}' DWS_CACHE_DIR='{}' DWS_KEYCHAIN_DIR='{}' DWS_AUTH_IDENTITY='{}' DWS_TENANT='{}' DINGTALK_DWS_AGENTCODE='{}' '{}' contact user get-self --format json",
                shell_single_quote(config_dir),
                shell_single_quote(&cache_dir),
                shell_single_quote(&keychain_dir),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote(cli_path),
                shell_single_quote(config_dir),
                shell_single_quote(&cache_dir),
                shell_single_quote(&keychain_dir),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote(cli_path),
                shell_single_quote(config_dir),
                shell_single_quote(&cache_dir),
                shell_single_quote(&keychain_dir),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote(cli_path),
                shell_single_quote_text(auth_identity),
                shell_single_quote(config_dir),
                shell_single_quote(&cache_dir),
                shell_single_quote(&keychain_dir),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote(cli_path)
            )
        }
        _ => shell_single_quote(cli_path),
    }
}

fn default_bind_command_windows(platform: &str, cli_path: &Path, config_dir: &Path) -> String {
    match platform {
        "wechat" => format!(
            "New-Item -ItemType Directory -Force -Path {} | Out-Null; {} init --config {} --keys-file {} --force",
            powershell_single_quote(config_dir),
            powershell_cli_invocation(cli_path),
            powershell_single_quote(&config_dir.join("config.json")),
            powershell_single_quote(&config_dir.join("all_keys.json"))
        ),
        "wecom" => format!(
            "$env:WECOM_CLI_CONFIG_DIR = {}; {} init",
            powershell_single_quote(config_dir),
            powershell_cli_invocation(cli_path)
        ),
        "feishu" => {
            let home_dir = config_dir.join("home");
            let lark_config_dir = config_dir.join(".lark-cli");
            let appdata_dir = home_dir.join("AppData").join("Roaming");
            let local_appdata_dir = home_dir.join("AppData").join("Local");
            let required_scopes = [
                "search:message",
                "im:chat:read",
                "im:message:readonly",
                "im:message.p2p_msg:get_as_user",
                "im:message.group_msg:get_as_user",
                "contact:user.base:readonly",
                "contact:user.basic_profile:readonly",
            ]
            .join(" ");
            let profile_name = config_dir
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|value| value.to_str())
                .unwrap_or("im-board-feishu");
            let cli = powershell_cli_invocation(cli_path);
            format!(
                "$env:HOME = {}; $env:USERPROFILE = {}; $env:APPDATA = {}; $env:LOCALAPPDATA = {}; $env:LARKSUITE_CLI_CONFIG_DIR = {}; {} config init --new --brand feishu --name {}; if ($LASTEXITCODE -eq 0) {{ {} --profile {} auth login --scope {} }}; if ($LASTEXITCODE -eq 0) {{ {} --profile {} auth status --verify }}",
                powershell_single_quote(&home_dir),
                powershell_single_quote(&home_dir),
                powershell_single_quote(&appdata_dir),
                powershell_single_quote(&local_appdata_dir),
                powershell_single_quote(&lark_config_dir),
                cli,
                powershell_single_quote_text(profile_name),
                cli,
                powershell_single_quote_text(profile_name),
                powershell_single_quote_text(&required_scopes),
                cli,
                powershell_single_quote_text(profile_name)
            )
        }
        "dingtalk" => {
            let cache_dir = config_dir.join("cache");
            let keychain_dir = config_dir.join("keychain");
            let home_dir = config_dir.join("home");
            let appdata_dir = home_dir.join("AppData").join("Roaming");
            let local_appdata_dir = home_dir.join("AppData").join("Local");
            let token_path = keychain_dir.join(WINDOWS_DINGTALK_PROFILE_TOKEN_FILE);
            let auth_identity = config_dir
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|value| value.to_str())
                .unwrap_or("im-board-dingtalk");
            let cli = powershell_cli_invocation(cli_path);
            format!(
                "$env:HOME = {}; $env:USERPROFILE = {}; $env:APPDATA = {}; $env:LOCALAPPDATA = {}; $env:DWS_CONFIG_DIR = {}; $env:DWS_CACHE_DIR = {}; $env:DWS_KEYCHAIN_DIR = {}; $env:DWS_AUTH_IDENTITY = {}; $env:DWS_TENANT = {}; $env:DINGTALK_DWS_AGENTCODE = {}\n$dwsRegistryKey = {}; $dwsTokenValue = {}; $dwsProfileTokenPath = {}; $dwsPreviousToken = $null\nif (Test-Path $dwsRegistryKey) {{ $dwsPrevious = Get-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -ErrorAction SilentlyContinue; if ($null -ne $dwsPrevious) {{ $dwsPreviousToken = $dwsPrevious.$dwsTokenValue }} }}\nfunction Set-DwsProfileToken {{ New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dwsProfileTokenPath) | Out-Null; if (Test-Path $dwsProfileTokenPath) {{ New-Item -Force -Path $dwsRegistryKey | Out-Null; Set-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -Value ([System.IO.File]::ReadAllText($dwsProfileTokenPath).Trim()) }} elseif (Test-Path $dwsRegistryKey) {{ Remove-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -ErrorAction SilentlyContinue }} }}\nfunction Save-DwsProfileToken {{ $dwsCurrent = Get-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -ErrorAction SilentlyContinue; if ($null -eq $dwsCurrent) {{ throw '钉钉授权没有完成：未写入当前账号的授权令牌。' }}; [System.IO.File]::WriteAllText($dwsProfileTokenPath, [string]$dwsCurrent.$dwsTokenValue, [System.Text.Encoding]::ASCII) }}\nfunction Restore-DwsPreviousToken {{ if ($null -eq $dwsPreviousToken) {{ if (Test-Path $dwsRegistryKey) {{ Remove-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -ErrorAction SilentlyContinue }} }} else {{ New-Item -Force -Path $dwsRegistryKey | Out-Null; Set-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -Value $dwsPreviousToken }} }}\ntry {{ Set-DwsProfileToken; {} auth login --force; if ($LASTEXITCODE -eq 0) {{ {} auth status --format json }}; if ($LASTEXITCODE -eq 0) {{ {} pat chmod chat.message:list --agentCode {} --grant-type permanent }}; if ($LASTEXITCODE -eq 0) {{ {} contact user get-self --format json }}; if ($LASTEXITCODE -eq 0) {{ Save-DwsProfileToken }} }} finally {{ Restore-DwsPreviousToken }}",
                powershell_single_quote(&home_dir),
                powershell_single_quote(&home_dir),
                powershell_single_quote(&appdata_dir),
                powershell_single_quote(&local_appdata_dir),
                powershell_single_quote(config_dir),
                powershell_single_quote(&cache_dir),
                powershell_single_quote(&keychain_dir),
                powershell_single_quote_text(auth_identity),
                powershell_single_quote_text(auth_identity),
                powershell_single_quote_text(auth_identity),
                powershell_single_quote_text(WINDOWS_DINGTALK_REGISTRY_KEY),
                powershell_single_quote_text(WINDOWS_DINGTALK_AUTH_TOKEN_VALUE),
                powershell_single_quote(&token_path),
                cli,
                cli,
                cli,
                powershell_single_quote_text(auth_identity),
                cli
            )
        }
        _ => powershell_single_quote(cli_path),
    }
}

pub fn version_is_newer(latest: &str, current: &str) -> bool {
    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);
    latest_parts > current_parts
}

fn parse_version(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

pub fn platform_label(platform: &str) -> &str {
    match platform {
        "wechat" => "微信",
        "wecom" => "企业微信",
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        _ => platform,
    }
}

fn shell_single_quote(path: &Path) -> String {
    shell_single_quote_text(&path.to_string_lossy())
}

fn shell_cli_invocation(path: &Path) -> String {
    let quoted = shell_single_quote(path);
    if is_node_cli_entry(path) {
        format!("node {quoted}")
    } else {
        quoted
    }
}

fn shell_single_quote_text(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn powershell_single_quote(path: &Path) -> String {
    powershell_single_quote_text(&path.to_string_lossy())
}

fn powershell_single_quote_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn powershell_cli_invocation(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "js" {
        format!("node {}", powershell_single_quote(path))
    } else {
        format!("& {}", powershell_single_quote(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "downloads official CLI packages from npm registries"]
    async fn hot_update_installs_official_clis_without_system_npm() {
        let root = std::env::temp_dir().join(format!(
            "im-board-official-cli-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        for spec in OFFICIAL_CLIS {
            let version = npm_latest_version(spec.package, None).await.unwrap();
            let install_root = root.join("OfficialCli").join(spec.platform);
            install_package(
                &install_root,
                spec.package,
                &version,
                Path::new("/missing-resource-dir"),
                None,
            )
            .await
            .unwrap();
            let resolved = resolve_official_cli(Path::new("/missing-resource-dir"), &root, spec)
                .unwrap_or_else(|| panic!("{} CLI was not resolved", spec.platform));
            assert!(resolved.exists(), "{} missing", resolved.display());
            assert_eq!(
                official_cli_version(&resolved, spec).as_deref(),
                Some(version.as_str())
            );
        }
        fs::remove_dir_all(root).ok();
    }
}
