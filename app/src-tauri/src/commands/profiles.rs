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
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    profile_manager::upsert_profile(&conn, profile).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    profile_manager::delete_profile(&conn, &profile_id).map_err(|err| err.to_string())
}
