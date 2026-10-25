use super::job::{SyncJobMode, SyncProgress};
use crate::storage::{models::SyncResult, AppState};
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager, State};

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncJobSnapshot {
    pub revision: u64,
    pub job_id: String,
    pub profile_id: String,
    pub mode: String,
    pub running: bool,
    pub busy: bool,
    pub progress: Option<SyncProgress>,
    pub result: Option<SyncResult>,
    pub error: Option<String>,
}
impl SyncJobSnapshot {
    fn begin(&mut self, profile_id: String, mode: SyncJobMode) {
        *self = Self {
            revision: self.revision + 1,
            job_id: uuid::Uuid::new_v4().to_string(),
            profile_id,
            mode: match mode {
                SyncJobMode::Incremental => "incremental",
                SyncJobMode::FullResync => "full_resync",
                SyncJobMode::RetryAnalysis => "retry_analysis",
            }
            .into(),
            running: true,
            busy: true,
            ..Self::default()
        };
    }
    fn finish(&mut self, job_id: &str, outcome: &Result<SyncResult, String>) -> bool {
        if !self.running || self.job_id != job_id {
            return false;
        }
        self.revision += 1;
        self.running = false;
        self.busy = false;
        match outcome {
            Ok(result) => self.result = Some(result.clone()),
            Err(error) => self.error = Some(error.clone()),
        }
        true
    }
}

pub fn current(state: &AppState) -> Result<SyncJobSnapshot, String> {
    let mut snapshot = state
        .sync_job_snapshot
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    snapshot.busy = state.sync_job_running.load(Ordering::SeqCst);
    Ok(snapshot)
}

pub fn emit_progress(app: &tauri::AppHandle, progress: SyncProgress) {
    let _ = app.emit("sync-progress", &progress);
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let snapshot = {
        let Ok(mut snapshot) = state.sync_job_snapshot.lock() else {
            return;
        };
        if !snapshot.running {
            return;
        }
        snapshot.revision += 1;
        snapshot.progress = Some(progress);
        snapshot.clone()
    };
    let _ = app.emit("sync-job-state", snapshot);
}

pub struct JobRunGuard<'a> {
    state: State<'a, AppState>,
    app: tauri::AppHandle,
    job_id: String,
    finished: bool,
    _profile_activity: tokio::sync::OwnedRwLockReadGuard<()>,
}
impl<'a> JobRunGuard<'a> {
    pub fn acquire(
        app: tauri::AppHandle,
        state: State<'a, AppState>,
        profile_id: String,
        mode: SyncJobMode,
    ) -> Result<Self, String> {
        let activity = state.profile_activity.acquire()?;
        state
            .sync_job_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "已有同步任务正在运行，请稍后再试。".to_owned())?;
        if let Err(error) = state.sync_bridges.begin_job() {
            state.sync_job_running.store(false, Ordering::SeqCst);
            return Err(error);
        }
        let snapshot = match state.sync_job_snapshot.lock() {
            Ok(mut snapshot) => {
                snapshot.begin(profile_id, mode);
                snapshot.clone()
            }
            Err(error) => {
                state.sync_job_running.store(false, Ordering::SeqCst);
                return Err(error.to_string());
            }
        };
        let job_id = snapshot.job_id.clone();
        let _ = app.emit("sync-job-state", snapshot);
        Ok(Self {
            state,
            app,
            job_id,
            finished: false,
            _profile_activity: activity,
        })
    }
    pub fn finish(&mut self, outcome: &Result<SyncResult, String>) {
        if self.finished {
            return;
        }
        let snapshot = {
            let Ok(mut snapshot) = self.state.sync_job_snapshot.lock() else {
                self.state.sync_job_running.store(false, Ordering::SeqCst);
                self.finished = true;
                return;
            };
            if !snapshot.finish(&self.job_id, outcome) {
                return;
            }
            self.state.sync_job_running.store(false, Ordering::SeqCst);
            snapshot.clone()
        };
        self.finished = true;
        let _ = self.app.emit("sync-job-state", snapshot);
    }
}
impl Drop for JobRunGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(&Err("同步任务已中断。".into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revisions_and_job_identity_reject_old_completion() {
        let mut snapshot = SyncJobSnapshot::default();
        snapshot.begin("first".into(), SyncJobMode::Incremental);
        let old = snapshot.job_id.clone();
        assert!(snapshot.finish(&old, &Err("cancelled".into())));
        let last = snapshot.revision;
        snapshot.begin("second".into(), SyncJobMode::RetryAnalysis);
        assert!(snapshot.revision > last);
        assert!(snapshot.running);
        assert!(snapshot.error.is_none());
        assert!(!snapshot.finish(&old, &Err("late error".into())));
        assert!(snapshot.running);
        assert!(snapshot.error.is_none());
        let id = snapshot.job_id.clone();
        assert!(snapshot.finish(&id, &Err("fixture".into())));
        assert!(!snapshot.running);
        assert!(!snapshot.busy);
        assert_eq!(snapshot.error.as_deref(), Some("fixture"));
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["jobId"], id);
        assert!(json["progress"].is_null());
    }
}
