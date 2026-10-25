#[cfg(all(test, unix))]
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::storage::AppState;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncJobMode {
    Incremental,
    FullResync,
    RetryAnalysis,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub profile_id: String,
    pub profile_label: String,
    pub phase: String,
    pub message: String,
    pub current: i64,
    pub total: i64,
    pub should_refresh_dashboard: bool,
}

impl SyncProgress {
    pub fn new(
        profile_id: String,
        profile_label: String,
        phase: impl Into<String>,
        message: String,
        current: i64,
        total: i64,
    ) -> Self {
        let phase = phase.into();
        Self {
            should_refresh_dashboard: should_refresh_dashboard_for_phase(&phase),
            profile_id,
            profile_label,
            phase,
            message,
            current,
            total,
        }
    }
}

fn should_refresh_dashboard_for_phase(phase: &str) -> bool {
    matches!(
        phase,
        "clear_cache_done"
            | "history_done"
            | "fetch_messages_done"
            | "analysis_done"
            | "summary_done"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_progress_marks_only_dashboard_refresh_phases() {
        let refresh_progress = SyncProgress::new(
            "aggregate".to_owned(),
            "全平台".to_owned(),
            "analysis_done",
            "已完成分析".to_owned(),
            1,
            1,
        );
        let passive_progress = SyncProgress::new(
            "aggregate".to_owned(),
            "全平台".to_owned(),
            "analysis",
            "正在分析".to_owned(),
            1,
            2,
        );

        assert!(refresh_progress.should_refresh_dashboard);
        assert!(!passive_progress.should_refresh_dashboard);
    }
}

pub fn terminate_tracked_sync_bridges(state: &State<'_, AppState>) -> Result<bool, String> {
    state.sync_bridges.cancel_all()
}

#[cfg(all(test, unix))]
fn process_group_exists(pid: u32) -> bool {
    crate::process_group::exists(pid).expect("fixture process-group query failed")
}

pub fn spawn_auto_sync_task(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut first_run = true;
        let mut retry_delay_seconds: Option<u64> = None;
        let mut consecutive_failures = 0_u32;
        loop {
            let state = app.state::<AppState>();
            let minutes = state
                .auto_sync_frequency_minutes
                .load(Ordering::SeqCst)
                .max(1);
            let delay = if first_run {
                0
            } else {
                retry_delay_seconds.unwrap_or_else(|| minutes.saturating_mul(60))
            };
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
                _ = state.auto_sync_schedule_changed.notified() => {
                    retry_delay_seconds = None;
                    consecutive_failures = 0;
                    continue;
                },
            }

            first_run = false;
            let state = app.state::<AppState>();
            if state.sync_job_running.load(Ordering::SeqCst) {
                retry_delay_seconds = Some(60);
                continue;
            }

            let result = crate::sync::orchestrator::run_sync_job(
                app.clone(),
                state,
                "aggregate".to_owned(),
                SyncJobMode::Incremental,
            )
            .await;
            let failure = match result {
                Err(error) => Some(error),
                Ok(result) if matches!(result.sync_status.as_str(), "failed" | "partial") => Some(
                    result
                        .warnings
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "所有账号均未同步成功。".to_owned()),
                ),
                Ok(_) => None,
            };
            if let Some(error) = failure {
                consecutive_failures = consecutive_failures.saturating_add(1);
                retry_delay_seconds = Some(
                    60_u64
                        .saturating_mul(2_u64.saturating_pow(consecutive_failures.min(4)))
                        .min(minutes.saturating_mul(60)),
                );
                crate::diagnostics::record_error_event(
                    &app.state::<AppState>(),
                    crate::diagnostics::DiagnosticErrorEvent {
                        source: "sync",
                        category: "background_auto_sync",
                        severity: "warning",
                        profile_id: None,
                        platform: None,
                        operation: "background_auto_sync".to_owned(),
                        user_message: "后台自动同步失败，已等待下一轮自动同步。".to_owned(),
                        raw_detail: serde_json::json!({ "error": error }),
                        context: serde_json::json!({ "profileId": "aggregate" }),
                    },
                );
            } else {
                consecutive_failures = 0;
                retry_delay_seconds = None;
            }
        }
    });
}

#[cfg(all(test, unix))]
mod process_tree_tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    #[test]
    fn owned_cancellation_disarms_repeated_calls_and_old_registration() {
        let mut command = Command::new("/bin/sleep");
        command.arg("30").process_group(0);
        let child = command.spawn().unwrap();
        let owner = crate::process_group::ProcessOwner::from_blocking(child);
        let pid = owner.pid();
        let registry = crate::bridge_registry::BridgeRegistry::default();
        let registration = registry.register(owner.clone());
        assert!(registry.cancel_all().unwrap());
        assert!(!registry.cancel_all().unwrap());
        assert!(registration.retire());
        assert!(owner.confirm_exit().unwrap());
        owner.terminate().unwrap();
        assert!(!process_group_exists(pid));
    }
    #[test]
    fn exited_leader_is_not_reaped_before_registry_finishes_the_group() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 7"]).process_group(0);
        let owner = crate::process_group::ProcessOwner::from_blocking(command.spawn().unwrap());
        assert_eq!(owner.wait_blocking().unwrap().code(), Some(7));
        assert_eq!(unsafe { libc::kill(owner.pid() as i32, 0) }, 0);
        let registry = crate::bridge_registry::BridgeRegistry::default();
        let _registration = registry.register(owner.clone());
        assert!(registry.cancel_all().unwrap());
        assert!(owner.confirm_exit().unwrap());
        assert_eq!(owner.wait_blocking().unwrap().code(), Some(7));
    }
}
