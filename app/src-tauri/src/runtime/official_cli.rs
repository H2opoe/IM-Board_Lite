use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::Emitter;

mod archives;
mod bind_commands;
mod npm_registry;
mod resolver;
mod versioning;

use archives::{
    download_first_available, extract_tgz_bytes, extract_zip_bytes, find_file_named,
    make_executable,
};
pub use bind_commands::{
    command_shell_name, default_bind_command, platform_cli_label, platform_label,
};
pub use npm_registry::npm_latest_version;
pub use resolver::{
    cli_spec, official_cli_version, resolve_official_cli, writable_cli_install_root,
};
pub use versioning::version_is_newer;

#[derive(Clone, Copy)]
pub struct PlatformCliSpec {
    pub platform: &'static str,
    pub package: &'static str,
    pub bin: &'static str,
    pub source: &'static str,
}

pub const OFFICIAL_CLIS: &[PlatformCliSpec] = &[
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
    pub command_shell: String,
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

pub async fn install_package(
    install_root: &Path,
    package: &str,
    version: &str,
    resource_dir: &Path,
    progress: Option<&PlatformCliProgressContext<'_>>,
) -> Result<(), String> {
    let _ = resource_dir;
    if let Some(progress) = progress {
        emit_platform_cli_progress(
            progress,
            "installing",
            format!("正在后台更新 {}@{}…", package, version),
            3,
            5,
            None,
            Some(package),
            Some(version),
        );
    }
    let parent = install_root
        .parent()
        .ok_or_else(|| format!("官方CLI热更新目录无效：{}", install_root.display()))?;
    fs::create_dir_all(parent).map_err(|err| {
        format!(
            "无法创建{}官方CLI热更新目录 {}：{err}",
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
            "无法创建{}官方CLI临时热更新目录 {}：{err}",
            platform_label(package),
            staging_root.display()
        )
    })?;
    let install_result = async {
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
                            "远程版本核查失败，继续使用已准备好的{}。",
                            platform_cli_label(spec.platform)
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
                        "已找到最新版{} v{}。",
                        platform_cli_label(spec.platform),
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
            format!("正在定位{}可执行入口…", platform_cli_label(spec.platform)),
            4,
            5,
            None,
            Some(spec.package),
            Some(&latest_version),
        );
    }
    let path = resolve_official_cli(resource_dir, app_dir, spec).ok_or_else(|| {
        format!(
            "{}已准备完成，但未找到无需用户依赖的可执行入口。",
            platform_cli_label(spec.platform)
        )
    })?;
    if let Some(progress) = progress {
        emit_platform_cli_progress(
            progress,
            "ready",
            format!("已准备好{}。", platform_cli_label(spec.platform)),
            5,
            5,
            None,
            Some(spec.package),
            Some(&latest_version),
        );
    }
    Ok(path)
}

mod installer;
pub use installer::remove_platform_cli_staging_dirs;
use installer::{install_npm_package_tree, replace_install_root_atomically};

async fn ensure_platform_binary(
    install_root: &Path,
    package: &str,
    version: &str,
) -> Result<(), String> {
    match package {
        "@wecom/cli" => ensure_wecom_binary(install_root),
        "@larksuite/cli" => ensure_feishu_binary(install_root, version).await,
        "dingtalk-workspace-cli" => ensure_dingtalk_binary(install_root),
        _ => Ok(()),
    }
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

include!("official_cli/platform_binaries.rs");
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
