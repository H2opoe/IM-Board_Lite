use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use tauri::State;

use crate::profile_manager;
use crate::storage::models::ImProfile;
use crate::storage::AppState;

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Result<Vec<ImProfile>, String> {
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    profile_manager::list_profiles(&conn).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn upsert_profile(state: State<'_, AppState>, profile: ImProfile) -> Result<ImProfile, String> {
    if profile.platform == "wechat" {
        return Err("微信功能仅限付费用户使用，请联系开发者开通。".to_owned());
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

fn cleanup_profile_runtime_files(profile: &ImProfile, app_dir: &Path, cache_dir: &Path) -> Result<(), String> {
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
        if let Some(path) = profile.config_json.get(key).and_then(|value| value.as_str()) {
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
    let first = relative.components().find_map(|component| match component {
        Component::Normal(value) => Some(value),
        _ => None,
    })?;
    Some(managed_root.join(first))
}
