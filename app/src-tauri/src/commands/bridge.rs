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
    if crate::connectors::find(&request.platform).is_none() {
        return Err("暂不支持该平台。".to_owned());
    }
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    let diagnostic_request = request.clone();
    let envelope = bridge_runner::run_bridge(request, resource_dir, state.cache_dir.clone())
        .await
        .map_err(|err| {
            let message = err.to_string();
            crate::diagnostics::record_bridge_failure(&state, &diagnostic_request, &message);
            message
        })?;
    crate::diagnostics::record_bridge_envelope(&state, &diagnostic_request, &envelope);
    Ok(envelope)
}

#[tauri::command]
pub async fn deploy_platform_bridge(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    platform: String,
    profile_id: String,
) -> Result<PlatformDeployment, String> {
    let platform = platform.trim().to_ascii_lowercase();
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    let spec = official_cli::cli_spec(&platform).ok_or_else(|| "暂不支持该平台。".to_owned())?;
    let progress = PlatformCliProgressContext {
        app: &app,
        platform: &platform,
        profile_id: &profile_id,
    };
    official_cli::emit_platform_cli_progress(
        &progress,
        "checking_local",
        format!(
            "Checking {} local readiness...",
            official_cli::platform_cli_label(&platform)
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
    .await
    .map_err(|err| {
        crate::diagnostics::record_cli_lifecycle_error(
            &state,
            &platform,
            Some(profile_id.clone()),
            "deploy_platform_bridge",
            &err,
        );
        err
    })?;
    let current_version = official_cli::official_cli_version(&cli_path, spec).unwrap_or_default();
    let config_dir = state
        .app_dir
        .join("Profiles")
        .join(&profile_id)
        .join(&platform);
    std::fs::create_dir_all(&config_dir).map_err(|err| {
        let message = err.to_string();
        crate::diagnostics::record_cli_lifecycle_error(
            &state,
            &platform,
            Some(profile_id.clone()),
            "deploy_platform_bridge",
            &message,
        );
        message
    })?;

    Ok(PlatformDeployment {
        platform: platform.clone(),
        cli_path: cli_path.to_string_lossy().to_string(),
        config_dir: config_dir.to_string_lossy().to_string(),
        command: official_cli::default_bind_command(&platform, &cli_path, &config_dir),
        command_shell: official_cli::command_shell_name().to_owned(),
        source_dir: None,
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
            let message = format!(
                "{} has not been prepared.",
                official_cli::platform_cli_label(&platform)
            );
            crate::diagnostics::record_cli_lifecycle_error(
                &state,
                &platform,
                None,
                "check_platform_cli_update",
                &message,
            );
            message
        })?;
    let current_version = official_cli::official_cli_version(&cli_path, spec).ok_or_else(|| {
        let message = "无法读取CLI版本。".to_owned();
        crate::diagnostics::record_cli_lifecycle_error(
            &state,
            &platform,
            None,
            "check_platform_cli_update",
            &message,
        );
        message
    })?;
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
    let latest_version = official_cli::npm_latest_version(spec.package, None)
        .await
        .map_err(|err| {
            let message = err.to_string();
            crate::diagnostics::record_cli_lifecycle_error(
                &state,
                &platform,
                None,
                "update_platform_cli",
                &message,
            );
            message
        })?;
    let install_root =
        official_cli::writable_cli_install_root(&state.app_dir, spec).map_err(|err| {
            let message = err.to_string();
            crate::diagnostics::record_cli_lifecycle_error(
                &state,
                &platform,
                None,
                "update_platform_cli",
                &message,
            );
            message
        })?;
    official_cli::install_package(
        &install_root,
        spec.package,
        &latest_version,
        &resource_dir,
        None,
    )
    .await
    .map_err(|err| {
        let message = err.to_string();
        crate::diagnostics::record_cli_lifecycle_error(
            &state,
            &platform,
            None,
            "update_platform_cli",
            &message,
        );
        message
    })?;

    let refreshed_cli_path =
        official_cli::resolve_official_cli(&resource_dir, &state.app_dir, spec).ok_or_else(
            || {
                let message = format!(
                    "{} was not found after update.",
                    official_cli::platform_cli_label(&platform)
                );
                crate::diagnostics::record_cli_lifecycle_error(
                    &state,
                    &platform,
                    None,
                    "update_platform_cli",
                    &message,
                );
                message
            },
        )?;
    let current_version = official_cli::official_cli_version(&refreshed_cli_path, spec)
        .ok_or_else(|| {
            let message = "更新后无法读取CLI版本。".to_owned();
            crate::diagnostics::record_cli_lifecycle_error(
                &state,
                &platform,
                None,
                "update_platform_cli",
                &message,
            );
            message
        })?;
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
                "删除{}缓存 {} 失败：{err}",
                official_cli::platform_cli_label(spec.platform),
                install_root.display()
            )
        })?;
    }
    official_cli::remove_platform_cli_staging_dirs(&state.app_dir, spec.platform)?;
    Ok(true)
}
