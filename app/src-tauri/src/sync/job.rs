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
    let pids = state
        .sync_bridge_pids
        .lock()
        .map_err(|err| err.to_string())?
        .clone();
    for pid in &pids {
        terminate_process_tree(*pid)?;
    }
    Ok(!pids.is_empty())
}

#[cfg(windows)]
fn terminate_process_tree(pid: u32) -> Result<(), String> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .map_err(|error| format!("终止同步进程失败：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("终止同步进程失败：{status}"))
    }
}

#[cfg(not(windows))]
fn terminate_process_tree(pid: u32) -> Result<(), String> {
    let process_group = format!("-{pid}");
    let terminated_group = Command::new("kill")
        .args(["-TERM", "--", &process_group])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !terminated_group {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
    for _ in 0..20 {
        if !process_group_exists(pid) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = Command::new("kill")
        .args(["-KILL", "--", &process_group])
        .status();
    let _ = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status();
    for _ in 0..20 {
        if !process_group_exists(pid) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if process_group_exists(pid) {
        Err(format!("同步进程 {pid} 未能在取消后退出。"))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn process_group_exists(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", "--", &format!("-{pid}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn spawn_auto_sync_task(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut retry_delay_seconds: Option<u64> = None;
        let mut consecutive_failures = 0_u32;
        loop {
            let state = app.state::<AppState>();
            let minutes = state
                .auto_sync_frequency_minutes
                .load(Ordering::SeqCst)
                .max(1);
            let delay = retry_delay_seconds.unwrap_or_else(|| minutes.saturating_mul(60));
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
                _ = state.auto_sync_schedule_changed.notified() => {
                    retry_delay_seconds = None;
                    consecutive_failures = 0;
                    continue;
                },
            }

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
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    #[test]
    fn escalates_when_leader_exits_but_descendant_ignores_term() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "sh -c 'trap \"\" TERM; echo ready; exec sleep 30' & wait",
        ]);
        command.process_group(0).stdout(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim(), "ready");
        let reaper = std::thread::spawn(move || child.wait().unwrap());
        let result = terminate_process_tree(pid);
        // Cleanup is unconditional, including a failing assertion on the implementation.
        let alive = process_group_exists(pid);
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .status();
        reaper.join().unwrap();
        assert!(result.is_ok(), "{result:?}");
        assert!(!alive, "a descendant survived cancellation");
    }
}
