use tauri::State;

use crate::domain::dashboard;
use crate::storage::models::DashboardData;
use crate::storage::AppState;

#[tauri::command]
pub fn get_dashboard(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<DashboardData, String> {
    dashboard::get_dashboard(&state, profile_id).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn mark_action_item(
    state: State<'_, AppState>,
    action_id: String,
    status: String,
) -> Result<(), String> {
    dashboard::mark_action_item(&state, &action_id, &status).map_err(|err| err.to_string())
}
