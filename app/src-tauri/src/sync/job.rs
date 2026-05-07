use std::process::Command;

use serde::{Deserialize, Serialize};
use tauri::State;

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
    let pids = state
        .sync_bridge_pids
        .lock()
        .map_err(|err| err.to_string())?
        .clone();
    for pid in &pids {
        terminate_process(*pid);
    }
    Ok(!pids.is_empty())
}

#[cfg(windows)]
fn terminate_process(pid: u32) {
    let _ = Command::new("taskkill")
        .arg("/PID")
        .arg(pid.to_string())
        .arg("/T")
        .arg("/F")
        .status();
}

#[cfg(not(windows))]
fn terminate_process(pid: u32) {
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
}
