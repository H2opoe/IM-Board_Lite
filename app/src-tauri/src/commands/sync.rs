use std::sync::atomic::Ordering;

use tauri::State;

use crate::storage::models::SyncResult;
use crate::storage::AppState;
use crate::sync::job::SyncJobMode;

#[tauri::command]
pub fn cancel_sync(state: State<'_, AppState>) -> Result<bool, String> {
    state.sync_cancel_requested.store(true, Ordering::SeqCst);
    crate::sync::job::terminate_tracked_sync_bridges(&state)
}

#[tauri::command]
pub async fn run_sync_job(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    mode: SyncJobMode,
) -> Result<SyncResult, String> {
    crate::sync::orchestrator::run_sync_job(app, state, profile_id, mode).await
}
