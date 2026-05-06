use std::fs;

use chrono::Local;
use tauri::{Manager, State};

use crate::bridge_runner::{self, BridgeEnvelope, BridgeRequest};
use crate::runtime::official_cli::{
    self, PlatformCliProgressContext, PlatformCliVersionStatus, PlatformDeployment,
};
use crate::storage::AppState;

#[tauri::command]
pub async fn run_bridge_command(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: BridgeRequest,
) -> Result<BridgeEnvelope, String> {
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    bridge_runner::run_bridge(request, resource_dir, state.cache_dir.clone())
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn deploy_platform_bridge(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    platform: String,
    profile_id: String,
) -> Result<PlatformDeployment, String> {
    let platform = platform.trim().to_ascii_lowercase();
    let spec = official_cli::cli_spec(&platform).ok_or_else(|| "暂不支持该平台。".to_owned())?;
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    let progress = PlatformCliProgressContext {
        app: &app,
        platform: &platform,
        profile_id: &profile_id,
    };
    official_cli::emit_platform_cli_progress(
        &progress,
        "checking_local",
        format!(
            "正在核查 {} 官方CLI准备状态…",
            official_cli::platform_label(&platform)
        ),
        1,
        5,
        None,
        Some(spec.package),
        None,
    );
    let cli_path = official_cli::ensure_platform_cli_ready(
        &resource_dir,
        &state.app_dir,
        spec,
        Some(&progress),
    )
    .await?;
    let current_version = official_cli::official_cli_version(&cli_path, spec).unwrap_or_default();
    let config_dir = state
        .app_dir
        .join("Profiles")
        .join(&profile_id)
        .join(&platform);
    std::fs::create_dir_all(&config_dir).map_err(|err| err.to_string())?;

    Ok(PlatformDeployment {
        platform: platform.clone(),
        cli_path: cli_path.to_string_lossy().to_string(),
        config_dir: config_dir.to_string_lossy().to_string(),
        command: official_cli::default_bind_command(&platform, &cli_path, &config_dir),
        source: spec.source.to_owned(),
        current_version,
    })
}

#[tauri::command]
pub async fn check_platform_cli_update(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    platform: String,
) -> Result<PlatformCliVersionStatus, String> {
    let platform = platform.trim().to_ascii_lowercase();
    let spec = official_cli::cli_spec(&platform).ok_or_else(|| "暂不支持该平台。".to_owned())?;
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    let cli_path = official_cli::resolve_official_cli(&resource_dir, &state.app_dir, spec)
        .ok_or_else(|| {
            format!(
                "尚未准备好 {} 官方CLI。",
                official_cli::platform_label(&platform)
            )
        })?;
    let current_version = official_cli::official_cli_version(&cli_path, spec)
        .ok_or_else(|| "无法读取 CLI版本。".to_owned())?;
    let latest_version = official_cli::npm_latest_version(spec.package, None)
        .await
        .unwrap_or_else(|_| current_version.clone());
    Ok(PlatformCliVersionStatus {
        platform,
        update_available: official_cli::version_is_newer(&latest_version, &current_version),
        current_version,
        latest_version,
        source: spec.source.to_owned(),
        checked_at: Local::now().to_rfc3339(),
    })
}

#[tauri::command]
pub async fn update_platform_cli(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    platform: String,
) -> Result<PlatformCliVersionStatus, String> {
    let platform = platform.trim().to_ascii_lowercase();
    let spec = official_cli::cli_spec(&platform).ok_or_else(|| "暂不支持该平台。".to_owned())?;
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    let latest_version = official_cli::npm_latest_version(spec.package, None).await?;
    let install_root = official_cli::writable_cli_install_root(&state.app_dir, spec)?;
    official_cli::install_package(
        &install_root,
        spec.package,
        &latest_version,
        &resource_dir,
        None,
    )
    .await?;

    let refreshed_cli_path =
        official_cli::resolve_official_cli(&resource_dir, &state.app_dir, spec).ok_or_else(
            || {
                format!(
                    "更新后未找到 {} 官方CLI。",
                    official_cli::platform_label(&platform)
                )
            },
        )?;
    let current_version = official_cli::official_cli_version(&refreshed_cli_path, spec)
        .ok_or_else(|| "更新后无法读取 CLI版本。".to_owned())?;
    Ok(PlatformCliVersionStatus {
        platform,
        update_available: official_cli::version_is_newer(&latest_version, &current_version),
        current_version,
        latest_version,
        source: spec.source.to_owned(),
        checked_at: Local::now().to_rfc3339(),
    })
}

#[tauri::command]
pub fn cleanup_unused_platform_cli(
    state: State<'_, AppState>,
    platform: String,
) -> Result<bool, String> {
    let platform = platform.trim().to_ascii_lowercase();
    let spec = official_cli::cli_spec(&platform).ok_or_else(|| "暂不支持该平台。".to_owned())?;
    let bound_count = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        conn.query_row(
            "select count(*) from profiles where platform = ?1",
            [spec.platform],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| err.to_string())?
    };
    if bound_count > 0 {
        return Ok(false);
    }

    let install_root = state.app_dir.join("OfficialCli").join(spec.platform);
    if install_root.exists() {
        fs::remove_dir_all(&install_root).map_err(|err| {
            format!(
                "删除{}官方CLI缓存 {} 失败：{err}",
                official_cli::platform_label(spec.platform),
                install_root.display()
            )
        })?;
    }
    official_cli::remove_platform_cli_staging_dirs(&state.app_dir, spec.platform)?;
    Ok(true)
}
