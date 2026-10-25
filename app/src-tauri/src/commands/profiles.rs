use tauri::State;

#[tauri::command]
pub fn get_connector_capabilities() -> Vec<crate::connectors::ConnectorCapabilityDescriptor> {
    crate::connectors::capability_descriptors()
}

use crate::profile_manager;
use crate::storage::models::ImProfile;
use crate::storage::AppState;

const SUPPORTED_PLATFORMS: &[&str] = &["wecom", "feishu", "dingtalk"];

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Result<Vec<ImProfile>, String> {
    let mut conn = state.db.lock().map_err(|err| err.to_string())?;
    profile_manager::list_profiles(&mut conn).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn create_profile_draft(
    state: State<'_, AppState>,
    platform: String,
    sort_order: i64,
) -> Result<ImProfile, String> {
    if !SUPPORTED_PLATFORMS.contains(&platform.as_str()) {
        return Err("不支持的账号平台。".to_owned());
    }

    let now = chrono::Local::now();
    let id = format!("{platform}_{}", uuid::Uuid::new_v4());
    let profile_dir = state.app_dir.join("Profiles").join(&id);
    let cache_dir = state.cache_dir.join(&id);
    let mut config = serde_json::json!({
        "authType": "cli_session",
        "profileDir": profile_dir.to_string_lossy(),
        "configPath": profile_dir.join("config.json").to_string_lossy(),
        "cacheDir": cache_dir.to_string_lossy(),
        "tmpDir": cache_dir.join("tmp").to_string_lossy()
    });
    let config_map = config
        .as_object_mut()
        .ok_or_else(|| "创建账号配置失败。".to_owned())?;
    match platform.as_str() {
        "wecom" => {
            config_map.insert(
                "configDir".to_owned(),
                serde_json::json!(profile_dir.join("wecom").to_string_lossy()),
            );
            config_map.insert("cliPath".to_owned(), serde_json::json!("wecom-cli"));
        }
        "dingtalk" => {
            config_map.insert(
                "configDir".to_owned(),
                serde_json::json!(profile_dir.join("dingtalk").to_string_lossy()),
            );
            config_map.insert("cliPath".to_owned(), serde_json::json!("dws"));
        }
        "feishu" => {}
        _ => unreachable!(),
    }

    let timestamp = now.to_rfc3339();
    let draft = ImProfile {
        id,
        platform: platform.clone(),
        label: platform_label(&platform).to_owned(),
        enabled: true,
        config_json: config,
        status: "normal".to_owned(),
        sort_order,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };
    crate::profile_manager::drafts::register(&state, draft.clone())?;
    Ok(draft)
}

#[tauri::command]
pub fn reorder_profiles(
    state: State<'_, AppState>,
    profile_ids: Vec<String>,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|err| err.to_string())?;
    profile_manager::reorder_profiles(&mut conn, &profile_ids).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn upsert_profile(state: State<'_, AppState>, profile: ImProfile) -> Result<ImProfile, String> {
    if !SUPPORTED_PLATFORMS.contains(&profile.platform.as_str()) {
        return Err("不支持的账号平台。".to_owned());
    }
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    let mut drafts = state.binding_drafts.lock().map_err(|err| err.to_string())?;
    profile_manager::drafts::save_registered(&conn, &mut drafts, profile)
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    profile_manager::runtime_paths::validate_profile_id(&profile_id)?;
    let _activity = state.profile_activity.begin_delete()?;
    // Keep the established DB -> drafts lock order through planning and cleanup.
    // Other account saves/registrations cannot acquire a new reference mid-delete.
    let mut conn = state.db.lock().map_err(|err| err.to_string())?;
    let mut drafts = state.binding_drafts.lock().map_err(|err| err.to_string())?;
    if let Some(profile) =
        profile_manager::profile_by_id(&conn, &profile_id).map_err(|err| err.to_string())?
    {
        let profiles = profile_manager::list_profiles(&mut conn).map_err(|err| err.to_string())?;
        let others = profiles
            .iter()
            .chain(drafts.values())
            .filter(|item| item.id != profile_id);
        let paths = profile_manager::runtime_paths::cleanup_plan(
            &profile,
            others,
            &state.app_dir,
            &state.cache_dir,
        )?;
        let encoded = profile_manager::cleanup_queue::encode_for_profile(
            &paths,
            &state.app_dir,
            &state.cache_dir,
            &profile,
        )
        .map_err(|err| err.to_string())?;
        profile_manager::delete_profile(&mut conn, &profile_id, Some(&encoded))
            .map_err(|err| err.to_string())?;
        drafts.remove(&profile_id);
    }
    let active_drafts: Vec<_> = drafts.values().cloned().collect();
    profile_manager::cleanup_queue::retry_pending(
        &mut conn,
        &active_drafts,
        &state.app_dir,
        &state.cache_dir,
        Some(&profile_id),
    )?;
    Ok(())
}

fn platform_label(platform: &str) -> &'static str {
    match platform {
        "wecom" => "企业微信",
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        _ => "账号",
    }
}

#[tauri::command]
pub fn release_profile_draft(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    crate::profile_manager::drafts::release(&state, &profile_id)
}
