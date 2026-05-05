use std::sync::atomic::Ordering;

use tauri::State;

use crate::daily_cache::{self, RolloverResult};
use crate::storage::models::SyncResult;
use crate::storage::AppState;

#[tauri::command]
pub fn detect_day_rollover(state: State<'_, AppState>) -> Result<RolloverResult, String> {
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    daily_cache::detect_day_rollover(&conn).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn cancel_sync(state: State<'_, AppState>) -> Result<bool, String> {
    state.sync_cancel_requested.store(true, Ordering::SeqCst);
    crate::sync::job::terminate_tracked_sync_bridges(&state)
}

#[tauri::command]
pub async fn run_manual_sync(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<SyncResult, String> {
    crate::sync::orchestrator::run_manual_sync(app, state, profile_id).await
}

#[tauri::command]
pub async fn retry_ai_analysis(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<SyncResult, String> {
    crate::sync::orchestrator::retry_ai_analysis(app, state, profile_id).await
}

#[tauri::command]
pub async fn run_full_resync(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<SyncResult, String> {
    crate::sync::orchestrator::run_full_resync(app, state, profile_id).await
}
