//! Cancellation snapshots own controls, not bare reusable process identifiers.
use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(unix)]
pub(crate) type BridgeTarget = Arc<crate::process_group::ProcessOwner>;
#[cfg(windows)]
pub(crate) type BridgeTarget = Arc<crate::process_job::ProcessJob>;

pub(crate) type BridgeRegistry = Registry<BridgeTarget>;
pub(crate) type BridgeRegistration = Registration<BridgeTarget>;

pub(crate) struct Registry<T> {
    inner: Arc<RegistryInner<T>>,
}

struct RegistryInner<T> {
    cancelled: Arc<AtomicBool>,
    next_id: AtomicU64,
    entries: Mutex<BTreeMap<u64, Arc<Control<T>>>>,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Self::with_cancel_flag(Arc::new(AtomicBool::new(false)))
    }
}

impl<T> Clone for Registry<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

struct Control<T> {
    state: Mutex<ControlState<T>>,
}

struct ControlState<T> {
    target: Option<T>,
    exit_confirmed: bool,
}

impl<T> Control<T> {
    fn terminate_with(&self, terminate: impl FnOnce(&T) -> io::Result<()>) -> io::Result<bool> {
        // Serialize retirement with cancellation, so a delayed snapshot cannot
        // issue termination after cleanup has taken ownership of this instance.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(active) = state.target.as_ref() else {
            return Ok(false);
        };
        terminate(active)?;
        state.target = None;
        state.exit_confirmed = true;
        Ok(true)
    }

    fn retire(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.target = None;
        state.exit_confirmed
    }
}

pub(crate) struct Registration<T> {
    registry: Registry<T>,
    id: u64,
    control: Arc<Control<T>>,
}

impl<T> Registration<T> {
    pub(crate) fn cancellation_requested(&self) -> bool {
        self.registry.inner.cancelled.load(Ordering::SeqCst)
    }
    /// Retire this cancellation control and return whether its owned target
    /// already exited. Cleanup must not signal a reusable ID after this proof.
    pub(crate) fn retire(&self) -> bool {
        self.control.retire()
    }
}

impl<T> Drop for Registration<T> {
    fn drop(&mut self) {
        self.retire();
        self.registry
            .inner
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

impl<T> Registry<T> {
    pub(crate) fn with_cancel_flag(cancelled: Arc<AtomicBool>) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                cancelled,
                next_id: AtomicU64::new(0),
                entries: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub(crate) fn begin_job(&self) -> Result<(), String> {
        let entries = self.inner.entries.lock().unwrap_or_else(|e| e.into_inner());
        if !entries.is_empty() {
            return Err("上一轮桥接仍在清理，请稍后再试。".to_owned());
        }
        self.inner.cancelled.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub(crate) fn register(&self, target: T) -> Registration<T> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let control = Arc::new(Control {
            state: Mutex::new(ControlState {
                target: Some(target),
                exit_confirmed: false,
            }),
        });
        self.inner
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, control.clone());
        Registration {
            registry: self.clone(),
            id,
            control,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.inner
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    #[cfg(test)]
    fn snapshot(&self) -> Vec<Arc<Control<T>>> {
        self.inner
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    fn cancel_all_with(&self, terminate: impl Fn(&T) -> io::Result<()>) -> Result<bool, String> {
        let mut cancelled = false;
        let mut errors = Vec::new();
        let controls: Vec<_> = {
            let entries = self.inner.entries.lock().unwrap_or_else(|e| e.into_inner());
            self.inner.cancelled.store(true, Ordering::SeqCst);
            entries.values().cloned().collect()
        };
        for control in controls {
            match control.terminate_with(&terminate) {
                Ok(changed) => cancelled |= changed,
                Err(error) => errors.push(error.to_string()),
            }
        }
        if errors.is_empty() {
            Ok(cancelled)
        } else {
            Err(format!("部分桥接进程未能终止：{}", errors.join("；")))
        }
    }
}

impl BridgeRegistry {
    pub(crate) fn cancel_all(&self) -> Result<bool, String> {
        self.cancel_all_with(|target| {
            #[cfg(unix)]
            {
                target.terminate()
            }
            #[cfg(windows)]
            {
                target.terminate()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn cancellation_latches_for_late_registration_and_resets_only_when_empty() {
        let registry = Registry::<u32>::default();
        assert!(!registry
            .cancel_all_with(|_| panic!("no target yet"))
            .unwrap());
        let late = registry.register(101);
        assert!(late.cancellation_requested());
        assert!(registry.begin_job().is_err());
        drop(late);
        registry.begin_job().unwrap();
        let next = registry.register(101);
        assert!(!next.cancellation_requested());
    }

    #[test]
    fn retirement_preserves_confirmed_exit_and_invalidates_old_snapshots() {
        let registry = Registry::<u32>::default();
        let registration = registry.register(101);
        let old = registry.snapshot();
        assert!(registry.cancel_all_with(|_| Ok(())).unwrap());
        assert!(registration.retire());
        assert!(registration.retire());
        assert!(!old[0]
            .terminate_with(|_| panic!("target already exited"))
            .unwrap());
    }

    #[test]
    fn failed_cancellation_does_not_claim_confirmed_exit_at_retirement() {
        let registry = Registry::<u32>::default();
        let registration = registry.register(101);
        assert!(registry
            .cancel_all_with(|_| Err(io::Error::other("uncertain exit")))
            .is_err());
        assert!(!registration.retire());
        assert!(!registry
            .cancel_all_with(|_| panic!("cleanup owns the retired target"))
            .unwrap());
    }

    #[test]
    fn early_failure_removes_only_its_registration() {
        let registry = Registry::<u32>::default();
        let first = registry.register(101);
        {
            let _second = registry.register(202);
        }
        assert_eq!(registry.snapshot().len(), 1);
        drop(first);
        assert!(registry.is_empty());
    }

    #[test]
    fn same_numeric_target_does_not_merge_distinct_registrations() {
        let registry = Registry::<u32>::default();
        let first = registry.register(101);
        let second = registry.register(101);
        drop(first);
        assert_eq!(registry.snapshot().len(), 1);
        drop(second);
        assert!(registry.is_empty());
    }

    #[test]
    fn old_snapshot_cannot_cancel_replacement_with_same_numeric_target() {
        let registry = Registry::<u32>::default();
        let first = registry.register(101);
        let old = registry.snapshot();
        drop(first);
        let _replacement = registry.register(101);
        assert!(!old[0]
            .terminate_with(|_| panic!("stale snapshot signaled replacement"))
            .unwrap());
        let calls = AtomicUsize::new(0);
        assert!(registry
            .cancel_all_with(|_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn batch_attempts_every_target_and_retries_only_failures() {
        let registry = Registry::<u32>::default();
        let _first = registry.register(101);
        let _second = registry.register(202);
        let calls = Mutex::new(Vec::new());
        assert!(registry
            .cancel_all_with(|target| {
                calls.lock().unwrap().push(*target);
                if *target == 101 {
                    Err(io::Error::other("fixture failure"))
                } else {
                    Ok(())
                }
            })
            .is_err());
        assert_eq!(*calls.lock().unwrap(), [101, 202]);
        calls.lock().unwrap().clear();
        assert!(registry
            .cancel_all_with(|target| {
                calls.lock().unwrap().push(*target);
                Ok(())
            })
            .unwrap());
        assert_eq!(*calls.lock().unwrap(), [101]);
        assert!(!registry
            .cancel_all_with(|_| panic!("already cancelled"))
            .unwrap());
    }

    #[test]
    fn retirement_waits_for_inflight_cancellation() {
        use std::sync::mpsc;
        let registry = Registry::<u32>::default();
        let registration = registry.register(101);
        let control = registry.snapshot().pop().unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let cancellation = std::thread::spawn(move || {
            control.terminate_with(|_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
        });
        entered_rx.recv().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let retirement = std::thread::spawn(move || {
            drop(registration);
            done_tx.send(()).unwrap();
        });
        let early = done_rx.recv_timeout(std::time::Duration::from_millis(50));
        release_tx.send(()).unwrap();
        assert!(cancellation.join().unwrap().unwrap());
        retirement.join().unwrap();
        assert!(
            early.is_err(),
            "retired while a cancellation still owned the target"
        );
        assert!(registry.is_empty());
    }
}
