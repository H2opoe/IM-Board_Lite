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
    mut request: BridgeRequest,
) -> Result<BridgeEnvelope, String> {
    let _activity = state.profile_activity.acquire()?;
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    if request.command != "init-profile" {
        if let Some(profile_id) = request.profile.as_ref().map(|profile| profile.id.clone()) {
            let profile = crate::profile_manager::drafts::resolve_for_command(
                &state,
                &profile_id,
                &request.platform,
                &request.command,
            )?;
            request.platform = profile.platform.clone();
            request.profile = Some(profile);
        }
    }

    let diagnostic_request = request.clone();
    let envelope = state
        .profile_activity
        .bridge_scope()?
        .run(bridge_runner::run_bridge(
            request,
            resource_dir,
            state.cache_dir.clone(),
        ))
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
    let _activity = state.profile_activity.acquire()?;
    let platform = platform.trim().to_ascii_lowercase();
    crate::profile_manager::drafts::resolve_for_command(
        &state,
        &profile_id,
        &platform,
        "auth-status",
    )?;
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
            "正在核查{}准备状态…",
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
    .inspect_err(|err| {
        crate::diagnostics::record_cli_lifecycle_error(
            &state,
            &platform,
            Some(profile_id.clone()),
            "deploy_platform_bridge",
            err,
        );
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

    crate::profile_manager::drafts::record_deployment(
        &state,
        &profile_id,
        &platform,
        &cli_path,
        &config_dir,
    )?;
    Ok(PlatformDeployment {
        platform: platform.clone(),
        cli_path: cli_path.to_string_lossy().to_string(),
        config_dir: config_dir.to_string_lossy().to_string(),
        command: official_cli::default_bind_command(&platform, &cli_path, &config_dir),
        command_shell: official_cli::command_shell_name().to_owned(),
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
                "尚未准备好{}。",
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
    let latest_version = spec.compatible_version.to_owned();
    Ok(PlatformCliVersionStatus {
        platform,
        update_available: latest_version != current_version,
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
    let latest_version = spec.compatible_version.to_owned();
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
                    "更新后未找到{}。",
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
        update_available: latest_version != current_version,
        current_version,
        latest_version,
        source: spec.source.to_owned(),
        checked_at: Local::now().to_rfc3339(),
    })
}
