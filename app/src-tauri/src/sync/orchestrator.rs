use std::fs;
use std::sync::atomic::Ordering;

use crate::analysis::orchestrator::{emit_sync_progress, ensure_sync_not_cancelled};
use rusqlite::params;
use tauri::{Manager, State};

use crate::analysis::orchestrator::analyze_pending_messages;
use crate::bridge_runner::{self, BridgeRequest};
use crate::daily_cache;
use crate::messages::repository::{
    clear_dashboard_cache, reset_ai_generated_cache, resolve_all_profiles, resolve_target_profiles,
};
use crate::storage::models::SyncResult;
use crate::storage::AppState;
use crate::sync::fetch::{sync_target_profiles_messages, MessageImportWindow};
use crate::sync::job::SyncJobMode;

pub async fn run_sync_job(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    mode: SyncJobMode,
) -> Result<SyncResult, String> {
    let _guard = SyncJobRunGuard::acquire(state.clone())?;
    match mode {
        SyncJobMode::Incremental => run_manual_sync_inner(app, state, profile_id, true).await,
        SyncJobMode::FullResync => run_full_resync(app, state, profile_id).await,
        SyncJobMode::RetryAnalysis => retry_ai_analysis(app, state, profile_id).await,
    }
}

struct SyncJobRunGuard<'a> {
    state: State<'a, AppState>,
}

impl<'a> SyncJobRunGuard<'a> {
    fn acquire(state: State<'a, AppState>) -> Result<Self, String> {
        state
            .sync_job_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "已有同步任务正在运行，请稍后再试。".to_owned())?;
        Ok(Self { state })
    }
}

impl Drop for SyncJobRunGuard<'_> {
    fn drop(&mut self) {
        self.state.sync_job_running.store(false, Ordering::SeqCst);
    }
}

async fn run_manual_sync_inner(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    reset_cancel_flag: bool,
) -> Result<SyncResult, String> {
    if reset_cancel_flag {
        state.sync_cancel_requested.store(false, Ordering::SeqCst);
    }
    let started_at = chrono::Local::now().to_rfc3339();
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    let cache_dir = state.cache_dir.clone();
    prepare_sync_storage_access(&state);

    let (target_profiles, dashboard_day) = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        daily_cache::detect_day_rollover(&conn).map_err(|err| err.to_string())?;
        let dashboard_day =
            daily_cache::current_dashboard_day_info(&conn).map_err(|err| err.to_string())?;
        let target_profiles =
            resolve_target_profiles(&conn, &profile_id).map_err(|err| err.to_string())?;
        (target_profiles, dashboard_day)
    };
    let sync_window = MessageImportWindow::from_dashboard_day(&dashboard_day);
    let day = sync_window.day.clone();
    let day_start = sync_window.start_timestamp;
    let day_start_text = dashboard_day.day_start_text;
    let sync_end_text = dashboard_day.sync_end_text;

    let mut warnings = Vec::new();
    let mut inserted_messages = 0;

    for outcome in sync_target_profiles_messages(
        &app,
        &state,
        &target_profiles,
        &sync_window,
        day_start,
        &day_start_text,
        &sync_end_text,
        resource_dir.clone(),
        cache_dir.clone(),
    )
    .await?
    {
        inserted_messages += outcome.inserted_messages;
        warnings.extend(outcome.warnings);
    }

    {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|err| err.to_string())?;

        for profile in &target_profiles {
            tx.execute(
                "insert into sync_state(profile_id, day, last_sync_at, cursor_json, updated_at)
                 values(?1, ?2, datetime('now'), '{}', datetime('now'))
                 on conflict(profile_id, day) do update set
                   last_sync_at = excluded.last_sync_at,
                   updated_at = excluded.updated_at",
                params![profile.id, day],
            )
            .map_err(|err| err.to_string())?;
        }

        tx.commit().map_err(|err| err.to_string())?;
    }

    let (ai_status, analyzed_messages) = analyze_pending_messages(
        &app,
        &state,
        &day,
        &target_profiles,
        resource_dir.clone(),
        cache_dir.clone(),
        &mut warnings,
    )
    .await?;

    Ok(SyncResult {
        profile_id,
        sync_status: "synced".to_owned(),
        ai_status,
        inserted_messages,
        analyzed_messages,
        warnings,
        started_at,
        finished_at: chrono::Local::now().to_rfc3339(),
    })
}

async fn retry_ai_analysis(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<SyncResult, String> {
    state.sync_cancel_requested.store(false, Ordering::SeqCst);
    let started_at = chrono::Local::now().to_rfc3339();
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    let cache_dir = state.cache_dir.clone();
    prepare_sync_storage_access(&state);
    let (target_profiles, day, rollover) = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        let rollover = daily_cache::detect_day_rollover(&conn).map_err(|err| err.to_string())?;
        let day = daily_cache::current_dashboard_day_info(&conn).map_err(|err| err.to_string())?;
        let target_profiles =
            resolve_target_profiles(&conn, &profile_id).map_err(|err| err.to_string())?;
        (target_profiles, day, rollover)
    };
    if rollover.rolled_over {
        if let Some(profile) = target_profiles.first() {
            emit_sync_progress(
                &app,
                profile,
                "history",
                format!(
                    "业务日已从{}切换到{}，正在重新读取今天的消息…",
                    rollover.previous_day, rollover.current_day
                ),
                0,
                0,
            );
        }
        return run_manual_sync_inner(app, state, profile_id, false).await;
    }
    reset_ai_generated_cache(&state, &day, &target_profiles).map_err(|err| err.to_string())?;
    if let Some(profile) = target_profiles.first() {
        emit_sync_progress(
            &app,
            profile,
            "clear_cache_done",
            "AI generated cache was cleared; re-running analysis for today.".to_owned(),
            0,
            0,
        );
    }

    let mut warnings = Vec::new();
    let (ai_status, analyzed_messages) = analyze_pending_messages(
        &app,
        &state,
        &day.day,
        &target_profiles,
        resource_dir.clone(),
        cache_dir.clone(),
        &mut warnings,
    )
    .await?;

    Ok(SyncResult {
        profile_id,
        sync_status: "synced".to_owned(),
        ai_status,
        inserted_messages: 0,
        analyzed_messages,
        warnings,
        started_at,
        finished_at: chrono::Local::now().to_rfc3339(),
    })
}

async fn run_full_resync(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<SyncResult, String> {
    state.sync_cancel_requested.store(false, Ordering::SeqCst);
    let cache_dir = state.cache_dir.clone();
    prepare_sync_storage_access(&state);
    let (target_profiles, all_profiles, day) = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        daily_cache::detect_day_rollover(&conn).map_err(|err| err.to_string())?;
        (
            resolve_target_profiles(&conn, &profile_id).map_err(|err| err.to_string())?,
            resolve_all_profiles(&conn).map_err(|err| err.to_string())?,
            daily_cache::current_dashboard_day_info(&conn).map_err(|err| err.to_string())?,
        )
    };

    if cache_dir.exists() {
        fs::remove_dir_all(&cache_dir).map_err(|err| err.to_string())?;
    }
    fs::create_dir_all(&cache_dir).map_err(|err| err.to_string())?;
    ensure_sync_not_cancelled(&state)?;

    clear_dashboard_cache(&state, &day).map_err(|err| err.to_string())?;
    if let Some(profile) = target_profiles.first().or_else(|| all_profiles.first()) {
        emit_sync_progress(
            &app,
            profile,
            "clear_cache_done",
            "Today dashboard data and cache were cleared; re-reading today messages.".to_owned(),
            0,
            0,
        );
    }
    run_manual_sync_inner(app, state, profile_id, false).await
}

fn prepare_sync_storage_access(_state: &State<'_, AppState>) {}

pub(crate) async fn run_sync_bridge(
    state: &State<'_, AppState>,
    request: BridgeRequest,
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
) -> anyhow::Result<bridge_runner::BridgeEnvelope> {
    ensure_sync_not_cancelled(state).map_err(anyhow::Error::msg)?;
    let diagnostic_request = request.clone();
    let result = bridge_runner::run_bridge_tracked(
        request,
        resource_dir.clone(),
        cache_dir.clone(),
        Some(&state.sync_bridge_pids),
    )
    .await;
    ensure_sync_not_cancelled(state).map_err(anyhow::Error::msg)?;
    match &result {
        Ok(envelope) => {
            crate::diagnostics::record_bridge_envelope(state, &diagnostic_request, envelope);
        }
        Err(error) => {
            crate::diagnostics::record_bridge_failure(
                state,
                &diagnostic_request,
                &error.to_string(),
            );
        }
    }
    result
}
