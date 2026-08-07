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
pub fn initialize_auto_sync_frequency_minutes(
    state: State<'_, AppState>,
    legacy_minutes: u64,
) -> Result<u64, String> {
    let minutes = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        crate::storage::initialize_auto_sync_frequency_minutes(&conn, legacy_minutes)
            .map_err(|err| err.to_string())?
    };
    state
        .auto_sync_frequency_minutes
        .store(minutes, Ordering::SeqCst);
    state.auto_sync_schedule_changed.notify_one();
    Ok(minutes)
}

#[tauri::command]
pub fn set_auto_sync_frequency_minutes(
    state: State<'_, AppState>,
    minutes: u64,
) -> Result<u64, String> {
    let normalized_minutes = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        crate::storage::persist_auto_sync_frequency_minutes(&conn, minutes)
            .map_err(|err| err.to_string())?
    };
    state
        .auto_sync_frequency_minutes
        .store(normalized_minutes, Ordering::SeqCst);
    state.auto_sync_schedule_changed.notify_one();
    Ok(normalized_minutes)
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
