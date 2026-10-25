use std::sync::Arc;
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

/// Account readers hold a lease for their entire operation, including subprocesses
/// and final database writes. Deletion must acquire exclusivity before DB locks.
#[derive(Default)]
pub struct ProfileActivity {
    gate: Arc<RwLock<()>>,
    pending: Arc<PendingExits>,
}

impl ProfileActivity {
    pub fn acquire(&self) -> Result<OwnedRwLockReadGuard<()>, String> {
        self.pending.refresh();
        if !self
            .pending
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
        {
            return Err("桥接进程仍在清理或未确认退出，请稍后重试。".to_owned());
        }
        self.gate
            .clone()
            .try_read_owned()
            .map_err(|_| "正在删除账号，请稍后再试。".to_owned())
    }

    pub fn bridge_scope(&self) -> Result<BridgeActivityScope, String> {
        Ok(BridgeActivityScope(BridgeActivityContext {
            lease: Arc::new(self.acquire()?),
            pending: self.pending.clone(),
        }))
    }

    pub fn begin_delete(&self) -> Result<OwnedRwLockWriteGuard<()>, String> {
        {
            self.pending.refresh();
            if !self
                .pending
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
            {
                return Err("桥接进程仍在清理或未确认退出，请稍后重试删除账号。".to_owned());
            }
        }
        self.gate
            .clone()
            .try_write_owned()
            .map_err(|_| "账号任务正在运行，请等待任务结束后再删除账号。".to_owned())
    }
}

// This scope follows the bridge future. Cleanup guards clone its lease
// before cancellation can drop the scope, keeping account deletion excluded.
pub struct BridgeActivityScope(BridgeActivityContext);

#[derive(Clone)]
pub(crate) struct BridgeActivityContext {
    lease: Arc<OwnedRwLockReadGuard<()>>,
    pending: Arc<PendingExits>,
}

tokio::task_local! {
    static BRIDGE_ACTIVITY: BridgeActivityContext;
}

impl BridgeActivityScope {
    pub fn run<F: std::future::Future>(
        self,
        future: F,
    ) -> impl std::future::Future<Output = F::Output> {
        // Keep large bridge state machines out of the Tauri IPC future. On
        // Windows the IPC/spawn layers otherwise copy them on a 1 MiB UI stack.
        // TaskLocalFuture still owns the lease and preserves cancellation cleanup.
        Box::pin(BRIDGE_ACTIVITY.scope(self.0, future))
    }
}

pub(crate) fn current_bridge_activity() -> Option<BridgeActivityContext> {
    BRIDGE_ACTIVITY.try_with(Clone::clone).ok()
}

#[derive(Default)]
struct PendingExits {
    next_id: std::sync::atomic::AtomicU64,
    entries: std::sync::Mutex<Vec<PendingExit>>,
}

struct PendingExit {
    id: u64,
    target: ExitTarget,
    in_progress: bool,
    _lease: Arc<OwnedRwLockReadGuard<()>>,
}

pub(crate) struct PendingExitTicket {
    id: u64,
    pending: Arc<PendingExits>,
}

impl BridgeActivityContext {
    pub(crate) fn retain_until_exit(self, target: ExitTarget) -> PendingExitTicket {
        let id = self
            .pending
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.pending
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(PendingExit {
                id,
                target,
                in_progress: true,
                _lease: self.lease,
            });
        PendingExitTicket {
            id,
            pending: self.pending,
        }
    }
}

impl PendingExitTicket {
    pub(crate) fn confirm_exit(self) {
        self.pending
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|entry| entry.id != self.id);
    }
}

impl Drop for PendingExitTicket {
    fn drop(&mut self) {
        if let Some(entry) = self
            .pending
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter_mut()
            .find(|entry| entry.id == self.id)
        {
            // Failure or a worker that never started retains the lease. A later
            // read-only query may release it once absence is actually proven.
            entry.in_progress = false;
        }
    }
}

#[cfg(unix)]
type ExitTarget = Arc<crate::process_group::ProcessOwner>;
#[cfg(windows)]
type ExitTarget = Arc<crate::process_job::ProcessJob>;

impl PendingExits {
    fn refresh(&self) {
        #[cfg(unix)]
        self.refresh_with(|owner| owner.confirm_exit().map(|exited| !exited));
        #[cfg(windows)]
        self.refresh_with(|job| job.confirm_exit().map(|exited| !exited));
    }

    fn refresh_with(&self, mut exists: impl FnMut(&ExitTarget) -> std::io::Result<bool>) {
        // The retained owner/Job refers to the original instance. A reused
        // numeric PID cannot prolong or release this account's cleanup lease.
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|entry| entry.in_progress || !matches!(exists(&entry.target), Ok(false)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn new_account_operations_wait_for_pending_cleanup() {
        let activity = ProfileActivity::default();
        let scope = activity.bridge_scope().unwrap();
        let ticket = scope
            .0
            .clone()
            .retain_until_exit(crate::process_group::ProcessOwner::unconfirmed_fixture());
        drop(scope);
        // The in-progress ticket suppresses OS queries for this synthetic PID.
        assert!(activity.acquire().is_err());
        ticket.confirm_exit();
        assert!(activity.acquire().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn uncertain_cleanup_holds_lease_until_absence_is_proven() {
        let activity = ProfileActivity::default();
        let scope = activity.bridge_scope().unwrap();
        let ticket = scope
            .0
            .clone()
            .retain_until_exit(crate::process_group::ProcessOwner::unconfirmed_fixture());
        drop(scope);
        activity
            .pending
            .refresh_with(|_| panic!("worker still owns cleanup"));
        assert!(activity.gate.clone().try_write_owned().is_err());
        drop(ticket);
        activity
            .pending
            .refresh_with(|_| Err(std::io::Error::from_raw_os_error(libc::EPERM)));
        assert!(activity.gate.clone().try_write_owned().is_err());
        activity.pending.refresh_with(|_| Ok(true));
        assert!(activity.gate.clone().try_write_owned().is_err());
        activity.pending.refresh_with(|_| Ok(false));
        assert!(activity.gate.clone().try_write_owned().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_confirmation_releases_only_its_own_lease() {
        let activity = ProfileActivity::default();
        let scope = activity.bridge_scope().unwrap();
        let first = scope
            .0
            .clone()
            .retain_until_exit(crate::process_group::ProcessOwner::unconfirmed_fixture());
        let second = scope
            .0
            .clone()
            .retain_until_exit(crate::process_group::ProcessOwner::unconfirmed_fixture());
        drop(scope);
        first.confirm_exit();
        assert!(activity.gate.clone().try_write_owned().is_err());
        second.confirm_exit();
        assert!(activity.gate.clone().try_write_owned().is_ok());
    }

    #[test]
    fn deletion_waits_for_every_existing_reader() {
        let activity = ProfileActivity::default();
        let first = activity.acquire().unwrap();
        let second = activity.acquire().unwrap();
        assert!(activity.begin_delete().is_err());
        drop(first);
        assert!(activity.begin_delete().is_err());
        drop(second);
        assert!(activity.begin_delete().is_ok());
    }

    #[test]
    fn deletion_blocks_new_readers_and_other_deletions() {
        let activity = ProfileActivity::default();
        let deletion = activity.begin_delete().unwrap();
        assert!(activity.acquire().is_err());
        assert!(activity.begin_delete().is_err());
        drop(deletion);
        assert!(activity.acquire().is_ok());
    }

    #[test]
    fn failed_operation_releases_lease() {
        fn fail(activity: &ProfileActivity) -> Result<(), String> {
            let _lease = activity.acquire()?;
            Err("fixture".into())
        }
        let activity = ProfileActivity::default();
        assert!(fail(&activity).is_err());
        assert!(activity.begin_delete().is_ok());
    }

    #[test]
    fn competing_reader_and_deletion_cannot_both_enter() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Barrier,
        };
        let activity = Arc::new(ProfileActivity::default());
        let barrier = Arc::new(Barrier::new(2));
        let entered = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            let (worker_activity, worker_barrier, worker_entered) =
                (activity.clone(), barrier.clone(), entered.clone());
            scope.spawn(move || {
                worker_barrier.wait();
                let lease = worker_activity.acquire();
                if lease.is_ok() {
                    worker_entered.fetch_add(1, Ordering::SeqCst);
                }
                worker_barrier.wait();
                assert_eq!(worker_entered.load(Ordering::SeqCst), 1);
                worker_barrier.wait();
                drop(lease);
            });
            barrier.wait();
            let deletion = activity.begin_delete();
            if deletion.is_ok() {
                entered.fetch_add(1, Ordering::SeqCst);
            }
            barrier.wait();
            assert_eq!(entered.load(Ordering::SeqCst), 1);
            barrier.wait();
            drop(deletion);
        });
    }
}

#[cfg(test)]
mod future_layout_tests {
    use super::*;
    #[test]
    fn scoped_large_task_has_pointer_sized_dispatch_state() {
        let activity = ProfileActivity::default();
        let payload = [7_u8; 128 * 1024];
        let task = activity.bridge_scope().unwrap().run(async move {
            std::future::pending::<()>().await;
            std::hint::black_box(payload);
        });
        assert!(
            std::mem::size_of_val(&task) <= 64,
            "bridge dispatch must not embed the large task on the UI stack"
        );
        assert!(activity.begin_delete().is_err());
        drop(task);
        assert!(activity.begin_delete().is_ok());
    }
}
