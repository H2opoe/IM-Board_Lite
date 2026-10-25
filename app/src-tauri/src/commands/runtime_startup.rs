use crate::local_runtime_supervisor::LocalRuntimeSupervisor;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) async fn await_step<T>(
    cancel: Option<&AtomicBool>,
    future: impl Future<Output = T>,
) -> Result<T, String> {
    tokio::pin!(future);
    loop {
        if cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            return Err("同步已终止。".to_owned());
        }
        tokio::select! {
            result = &mut future => return Ok(result),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)), if cancel.is_some() => {}
        }
    }
}

/// Created while holding the supervisor startup lock. Cancellation attempts to
/// stop the launched process; failed stops retain ownership for later recovery.
pub(super) struct StartupProcessCleanup<'a> {
    supervisor: &'a LocalRuntimeSupervisor,
    armed: bool,
}
impl<'a> StartupProcessCleanup<'a> {
    pub(super) fn new(supervisor: &'a LocalRuntimeSupervisor) -> Self {
        Self {
            supervisor,
            armed: false,
        }
    }
    pub(super) fn arm(&mut self) {
        self.armed = true;
    }
    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for StartupProcessCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.supervisor.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, time::Duration};
    #[tokio::test]
    async fn cancellation_interrupts_pending_preparation_and_lock_waits() {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let trigger = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            flag.store(true, Ordering::SeqCst);
        });
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            await_step(Some(&cancel), std::future::pending::<()>()),
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap_err(), "同步已终止。");
        trigger.await.unwrap();
        assert_eq!(await_step(None, async { 42 }).await.unwrap(), 42);
        let supervisor = LocalRuntimeSupervisor::new().unwrap();
        let _busy = supervisor.startup_guard().await;
        assert!(await_step(Some(&cancel), supervisor.startup_guard())
            .await
            .is_err());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_health_wait_reaps_only_the_test_process() {
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("30");
        let supervisor = Arc::new(LocalRuntimeSupervisor::new().unwrap());
        let (mut child, generation) = supervisor.spawn(command).unwrap();
        let pid = child.id();
        let observed = supervisor.clone();
        let reaper = std::thread::spawn(move || {
            let result = child.observe_exit();
            observed
                .observe_wait(pid, generation, &result)
                .expect("fixture exit observation");
            result.unwrap()
        });
        {
            let mut cleanup = StartupProcessCleanup::new(&supervisor);
            cleanup.arm();
            let cancelled = AtomicBool::new(true);
            assert!(await_step(
                Some(&cancelled),
                supervisor.wait_until_ready("fixture", Duration::from_secs(90))
            )
            .await
            .is_err());
        }
        reaper.join().unwrap();
        assert!(!supervisor.has_owned_process());
    }
}
