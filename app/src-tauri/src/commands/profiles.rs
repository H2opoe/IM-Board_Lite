use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

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
    let id = format!("{platform}_{}", now.timestamp_millis());
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
    Ok(ImProfile {
        id,
        platform: platform.clone(),
        label: platform_label(&platform).to_owned(),
        enabled: true,
        config_json: config,
        status: "normal".to_owned(),
        sort_order,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    })
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
    profile_manager::upsert_profile(&conn, profile).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    let deleted_profile = {
        let mut conn = state.db.lock().map_err(|err| err.to_string())?;
        profile_manager::delete_profile(&mut conn, &profile_id).map_err(|err| err.to_string())?
    };
    if let Some(profile) = deleted_profile {
        cleanup_profile_runtime_files(&profile, &state.app_dir, &state.cache_dir)?;
    }
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

fn cleanup_profile_runtime_files(
    profile: &ImProfile,
    app_dir: &Path,
    cache_dir: &Path,
) -> Result<(), String> {
    let mut targets = BTreeSet::new();
    let profile_root = app_dir.join("Profiles");
    for path in profile_runtime_path_candidates(profile, cache_dir) {
        if let Some(target) = managed_profile_root(&profile_root, &path) {
            targets.insert(target);
        }
        if let Some(target) = managed_profile_root(cache_dir, &path) {
            targets.insert(target);
        }
    }
    targets.insert(profile_root.join(&profile.id));
    targets.insert(cache_dir.join(&profile.id));

    for target in targets {
        if target.exists() {
            fs::remove_dir_all(&target)
                .map_err(|err| format!("删除账号本地缓存 {} 失败：{err}", target.display()))?;
        }
    }
    Ok(())
}

fn profile_runtime_path_candidates(profile: &ImProfile, cache_dir: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for key in [
        "profileDir",
        "configDir",
        "configPath",
        "keysPath",
        "cacheDir",
        "tmpDir",
    ] {
        if let Some(path) = profile
            .config_json
            .get(key)
            .and_then(|value| value.as_str())
        {
            candidates.push(expand_tilde_path(path));
        }
    }
    candidates.push(cache_dir.join(&profile.id));
    candidates
}

fn expand_tilde_path(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn managed_profile_root(managed_root: &Path, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(managed_root).ok()?;
    let first = relative
        .components()
        .find_map(|component| match component {
            Component::Normal(value) => Some(value),
            _ => None,
        })?;
    Some(managed_root.join(first))
}
