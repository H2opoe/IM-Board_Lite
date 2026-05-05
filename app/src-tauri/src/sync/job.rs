use std::process::Command;

use tauri::State;

use crate::storage::AppState;

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
