use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::Emitter;

mod archives;
mod bind_commands;
pub(crate) mod dws_bundle;
mod extraction;
mod native_artifacts;
mod resolver;

use archives::{download_first_verified, extract_tgz_bytes, find_file_named, make_executable};
pub use bind_commands::{
    command_shell_name, default_bind_command, platform_cli_label, platform_label,
};
pub use resolver::{
    cli_spec, official_cli_version, resolve_official_cli, writable_cli_install_root,
};

#[derive(Clone, Copy)]
pub struct PlatformCliSpec {
    pub platform: &'static str,
    pub package: &'static str,
    pub bin: &'static str,
    pub source: &'static str,
    pub compatible_version: &'static str,
}

pub const OFFICIAL_CLIS: &[PlatformCliSpec] = &[
    PlatformCliSpec {
        platform: "wecom",
        package: "@wecom/cli",
        bin: "wecom-cli",
        source: "https://github.com/WecomTeam/wecom-cli",
        compatible_version: "1.3.4",
    },
    PlatformCliSpec {
        platform: "feishu",
        package: "@larksuite/cli",
        bin: "lark-cli",
        source: "https://github.com/larksuite/cli",
        compatible_version: "1.0.98",
    },
    PlatformCliSpec {
        platform: "dingtalk",
        package: "dingtalk-workspace-cli",
        bin: "dws",
        source: "https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli",
        compatible_version: dws_bundle::VERSION,
    },
];

const PLATFORM_CLI_PROGRESS_EVENT: &str = "platform-cli-deployment-progress";
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
    let _install_lock = installer::INSTALL_LOCK.lock().await;
    installer::recover_interrupted_install(install_root)?;
    let target = native_target();
    let bundled_dws = package == "dingtalk-workspace-cli";
    if bundled_dws {
        if version != dws_bundle::VERSION {
            return Err("未经验证的钉钉运行时版本。".into());
        }
    } else {
        native_artifacts::packages(package, version, &target)?;
    }
    if let Some(progress) = progress {
        emit_platform_cli_progress(
            progress,
            "installing",
            format!("正在后台更新{}@{}…", package, version),
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
    let _staging_guard = installer::StagingGuard(staging_root.clone());
    if bundled_dws {
        dws_bundle::stage(resource_dir, &staging_root)?;
    } else {
        install_native_packages(&staging_root, package, version, &target).await?;
        ensure_platform_binary(&staging_root, package, version).await?;
    }
    let binary = native_binary_path(&staging_root, package)?;
    installer::check_binary(&binary, &staging_root).await?;
    replace_install_root_atomically(install_root, &staging_root)?;
    if let Some(progress) = progress {
        emit_platform_cli_progress(
            progress,
            "installed",
            format!("{}@{}后台更新完成。", package, version),
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
    let compatible_version = spec.compatible_version.to_owned();
    if let (Some(path), Some(current)) = (&existing, &current_version) {
        if current == &compatible_version {
            if let Some(progress) = progress {
                emit_platform_cli_progress(
                    progress,
                    "local_ready",
                    format!(
                        "已找到兼容的{} v{}。",
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
        &compatible_version,
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
            Some(&compatible_version),
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
            Some(&compatible_version),
        );
    }
    Ok(path)
}

mod installer;
use installer::{install_native_packages, replace_install_root_atomically};

async fn ensure_platform_binary(
    install_root: &Path,
    package: &str,
    version: &str,
) -> Result<(), String> {
    match package {
        "@wecom/cli" => ensure_wecom_binary(install_root),
        "@larksuite/cli" => ensure_feishu_binary(install_root, version).await,
        _ => Err(format!("不支持的官方CLI包：{package}")),
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

fn native_target() -> String {
    format!("darwin-{}", current_npm_arch())
}

fn native_binary_path(root: &Path, package: &str) -> Result<PathBuf, String> {
    let target = native_target();
    let relative = match package {
        "@wecom/cli" => format!("@wecom/cli-{target}/bin/wecom-cli"),
        "@larksuite/cli" => format!(
            "@larksuite/cli/bin/lark-cli-{}",
            target.replace("win32", "windows")
        ),
        "dingtalk-workspace-cli" => format!(
            "dingtalk-workspace-cli/vendor/dws-{}",
            target.replace("win32", "windows")
        ),
        _ => return Err(format!("不支持的官方CLI包：{package}")),
    };
    Ok(root.join("node_modules").join(relative))
}

include!("official_cli/platform_binaries.rs");
// Progress payload fields map one-to-one to the frontend event contract.
#[allow(clippy::too_many_arguments)]
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
            let version = spec.compatible_version.to_owned();
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
