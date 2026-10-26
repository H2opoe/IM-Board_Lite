pub(super) use super::read_checkpoint::record_profile_sync_outcomes;
use std::fs;

use crate::analysis::orchestrator::{emit_sync_progress, ensure_sync_not_cancelled};
use tauri::{Manager, State};

use crate::analysis::orchestrator::analyze_pending_messages;
use crate::bridge_runner::{self, BridgeRequest};
use crate::daily_cache;
use crate::messages::import_window::MessageImportWindow;
use crate::messages::repository::{
    clear_dashboard_cache, clear_profile_dashboard_cache, reset_ai_generated_cache,
    resolve_all_profiles, resolve_target_profiles,
};
use crate::storage::models::SyncResult;
use crate::storage::AppState;
use crate::sync::fetch::sync_target_profiles_messages;
use crate::sync::job::SyncJobMode;

pub async fn run_sync_job(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    mode: SyncJobMode,
) -> Result<SyncResult, String> {
    // No enabled accounts is a normal first-launch state. Check before
    // publishing a job or entering maintenance paths that clear cached data.
    {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        let targets = resolve_target_profiles(&conn, &profile_id).map_err(|err| err.to_string())?;
        if targets.is_empty() {
            let config = crate::ai::get_config(&conn).map_err(|err| err.to_string())?;
            let ai_status =
                if crate::commands::ai::is_configured_for_current_runtime(&config, &state) {
                    "ready"
                } else {
                    "not_configured"
                };
            return Ok(empty_sync_result(profile_id, ai_status));
        }
    }
    let mut guard = crate::sync::status::JobRunGuard::acquire(
        app.clone(),
        state.clone(),
        profile_id.clone(),
        mode,
    )?;
    let result = match mode {
        SyncJobMode::Incremental => run_manual_sync_inner(app, state, profile_id).await,
        SyncJobMode::FullResync => run_full_resync(app, state, profile_id).await,
        SyncJobMode::RetryAnalysis => retry_ai_analysis(app, state, profile_id).await,
    };
    guard.finish(&result);
    result
}

async fn run_manual_sync_inner(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<SyncResult, String> {
    let started_at = chrono::Local::now().to_rfc3339();
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    let cache_dir = state.cache_dir.clone();
    prepare_sync_storage_access(&state);

    let (target_profiles, dashboard_day) = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        let (_, dashboard_day) =
            daily_cache::detect_day_rollover(&conn).map_err(|err| err.to_string())?;
        let target_profiles =
            resolve_target_profiles(&conn, &profile_id).map_err(|err| err.to_string())?;
        (target_profiles, dashboard_day)
    };
    let sync_window = MessageImportWindow {
        day: dashboard_day.day.clone(),
        start_timestamp: dashboard_day.day_start_timestamp,
        end_timestamp: dashboard_day.sync_end_timestamp,
    };
    let day = sync_window.day.clone();
    let day_start = sync_window.start_timestamp;
    let day_start_text = dashboard_day.day_start_text;
    let sync_end_text = dashboard_day.sync_end_text;

    let mut warnings = Vec::new();
    let mut inserted_messages = 0;

    let profile_outcomes = sync_target_profiles_messages(
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
    .await?;
    for outcome in &profile_outcomes {
        inserted_messages += outcome.inserted_messages;
        warnings.extend(outcome.warnings.clone());
    }

    {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        record_profile_sync_outcomes(&conn, &day, &profile_outcomes)?;
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

    let sync_status =
        aggregate_sync_status(profile_outcomes.iter().map(|outcome| outcome.succeeded));
    let sync_status = if sync_status == "failed" && inserted_messages > 0 {
        "partial"
    } else {
        sync_status
    };

    Ok(SyncResult {
        profile_id,
        sync_status: sync_status.to_owned(),
        ai_status,
        inserted_messages,
        analyzed_messages,
        warnings,
        started_at,
        finished_at: chrono::Local::now().to_rfc3339(),
    })
}

fn empty_sync_result(profile_id: String, ai_status: &str) -> SyncResult {
    let now = chrono::Local::now().to_rfc3339();
    SyncResult {
        profile_id,
        sync_status: "idle".to_owned(),
        ai_status: ai_status.to_owned(),
        inserted_messages: 0,
        analyzed_messages: 0,
        warnings: Vec::new(),
        started_at: now.clone(),
        finished_at: now,
    }
}

fn aggregate_sync_status(results: impl IntoIterator<Item = bool>) -> &'static str {
    let results = results.into_iter().collect::<Vec<_>>();
    if results.is_empty() {
        return "idle";
    }
    let successful = results.iter().filter(|value| **value).count();
    match successful {
        0 => "failed",
        count if count == results.len() => "synced",
        _ => "partial",
    }
}

async fn retry_ai_analysis(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<SyncResult, String> {
    let started_at = chrono::Local::now().to_rfc3339();
    let resource_dir = app.path().resource_dir().map_err(|err| err.to_string())?;
    let cache_dir = state.cache_dir.clone();
    prepare_sync_storage_access(&state);
    let (target_profiles, day, rollover) = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        let (rollover, day) =
            daily_cache::detect_day_rollover(&conn).map_err(|err| err.to_string())?;
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
        return run_manual_sync_inner(app, state, profile_id).await;
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
    let cache_dir = state.cache_dir.clone();
    prepare_sync_storage_access(&state);
    let (target_profiles, all_profiles, day) = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        let (_, day) = daily_cache::detect_day_rollover(&conn).map_err(|err| err.to_string())?;
        (
            resolve_target_profiles(&conn, &profile_id).map_err(|err| err.to_string())?,
            resolve_all_profiles(&conn).map_err(|err| err.to_string())?,
            day,
        )
    };

    let full_aggregate_resync = profile_id == "aggregate";
    if full_aggregate_resync && cache_dir.exists() {
        fs::remove_dir_all(&cache_dir).map_err(|err| err.to_string())?;
    } else {
        for profile in &target_profiles {
            let profile_cache = cache_dir.join(&profile.id);
            if profile_cache.exists() {
                fs::remove_dir_all(&profile_cache).map_err(|err| err.to_string())?;
            }
        }
    }
    fs::create_dir_all(&cache_dir).map_err(|err| err.to_string())?;
    ensure_sync_not_cancelled(&state)?;

    if full_aggregate_resync {
        clear_dashboard_cache(&state, &day).map_err(|err| err.to_string())?;
    } else {
        clear_profile_dashboard_cache(&state, &day, &target_profiles)
            .map_err(|err| err.to_string())?;
    }
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
    run_manual_sync_inner(app, state, profile_id).await
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
    let result = state
        .profile_activity
        .bridge_scope()
        .map_err(anyhow::Error::msg)?
        .run(bridge_runner::run_bridge_tracked(
            request,
            resource_dir.clone(),
            cache_dir.clone(),
            Some(&state.sync_bridges),
        ))
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

#[cfg(test)]
#[path = "orchestrator_tests.rs"]
mod tests;
