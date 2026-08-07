use std::sync::atomic::Ordering;

use tauri::{Emitter, State};

use crate::messages::normalizer::{platform_label, profile_remark};
use crate::storage::models::ImProfile;
use crate::storage::AppState;
use crate::sync::job::SyncProgress;

const SYNC_CANCELLED_MESSAGE: &str = "同步已终止。";

pub(crate) fn ensure_sync_not_cancelled(state: &State<'_, AppState>) -> Result<(), String> {
    if state.sync_cancel_requested.load(Ordering::SeqCst) {
        Err(SYNC_CANCELLED_MESSAGE.to_owned())
    } else {
        Ok(())
    }
}

pub(crate) fn is_sync_cancelled_message(message: &str) -> bool {
    message == SYNC_CANCELLED_MESSAGE
}

pub(crate) fn emit_analysis_progress_for_scope(
    app: &tauri::AppHandle,
    target_profiles: &[ImProfile],
    batch_index: usize,
    total_batches: usize,
) {
    if target_profiles.len() == 1 {
        emit_analysis_progress(app, &target_profiles[0], batch_index, total_batches);
        return;
    }
    emit_aggregate_progress(
        app,
        "analysis",
        format!(
            "正在识别全平台待回复和待办事项第{}/{}批…",
            batch_index, total_batches
        ),
        batch_index as i64,
        total_batches as i64,
    );
}

pub(crate) fn emit_analysis_done_for_scope(
    app: &tauri::AppHandle,
    target_profiles: &[ImProfile],
    batch_index: usize,
    total_batches: usize,
) {
    if target_profiles.len() == 1 {
        emit_sync_progress(
            app,
            &target_profiles[0],
            "analysis_done",
            format!(
                "已完成【{} · {}】待回复和待办事项识别第{}/{}批，正在更新看板…",
                platform_label(&target_profiles[0].platform),
                profile_remark(&target_profiles[0]),
                batch_index,
                total_batches
            ),
            batch_index as i64,
            total_batches as i64,
        );
        return;
    }
    emit_aggregate_progress(
        app,
        "analysis_done",
        format!(
            "已完成全平台待回复和待办事项识别第{}/{}批，正在更新看板…",
            batch_index, total_batches
        ),
        batch_index as i64,
        total_batches as i64,
    );
}

pub(crate) fn emit_summary_progress_for_scope(
    app: &tauri::AppHandle,
    target_profiles: &[ImProfile],
    phase: &str,
    message: String,
    current: i64,
    total: i64,
) {
    if target_profiles.len() == 1 {
        emit_sync_progress(app, &target_profiles[0], phase, message, current, total);
        return;
    }
    emit_aggregate_progress(app, phase, message, current, total);
}

pub(crate) fn emit_sync_progress(
    app: &tauri::AppHandle,
    profile: &ImProfile,
    phase: &str,
    message: String,
    current: i64,
    total: i64,
) {
    let _ = app.emit(
        "sync-progress",
        SyncProgress::new(
            profile.id.clone(),
            profile.label.clone(),
            phase,
            message,
            current,
            total,
        ),
    );
}

fn emit_analysis_progress(
    app: &tauri::AppHandle,
    profile: &ImProfile,
    batch_index: usize,
    total_batches: usize,
) {
    emit_sync_progress(
        app,
        profile,
        "analysis",
        format!(
            "正在识别【{} · {}】待回复和待办事项第{}/{}批…",
            platform_label(&profile.platform),
            profile_remark(profile),
            batch_index,
            total_batches
        ),
        batch_index as i64,
        total_batches as i64,
    );
}

fn emit_aggregate_progress(
    app: &tauri::AppHandle,
    phase: &str,
    message: String,
    current: i64,
    total: i64,
) {
    let _ = app.emit(
        "sync-progress",
        SyncProgress::new(
            "aggregate".to_owned(),
            "全平台".to_owned(),
            phase,
            message,
            current,
            total,
        ),
    );
}
