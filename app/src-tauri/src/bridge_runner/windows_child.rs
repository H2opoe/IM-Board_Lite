use crate::process_job::ProcessJob;
use crate::profile_manager::activity::{
    current_bridge_activity, BridgeActivityContext, PendingExitTicket,
};
use std::io;
use std::sync::Arc;

pub(crate) struct BridgeChild {
    child: tokio::process::Child,
    cleanup: Option<JobCleanup>,
}

impl BridgeChild {
    pub(super) fn spawn(command: tokio::process::Command) -> io::Result<Self> {
        let job = Arc::new(ProcessJob::new()?);
        // Create the cleanup owner before assignment/resume, including failures.
        let cleanup = JobCleanup {
            job: Some(job.clone()),
            activity: current_bridge_activity(),
            ticket: None,
            registration: None,
        };
        let child = job.spawn_async(command)?;
        Ok(Self {
            child,
            cleanup: Some(cleanup),
        })
    }

    pub(super) fn into_parts(mut self) -> (tokio::process::Child, JobCleanup) {
        (
            self.child,
            self.cleanup.take().expect("bridge owns cleanup"),
        )
    }

    #[cfg(test)]
    pub(super) fn id(&self) -> Option<u32> {
        self.child.id()
    }

    #[cfg(test)]
    pub(super) fn stdout(&mut self) -> &mut Option<tokio::process::ChildStdout> {
        &mut self.child.stdout
    }

    #[cfg(test)]
    pub(super) async fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        let status = self.child.wait().await?;
        if let Some(cleanup) = self.cleanup.take() {
            let cleanup = cleanup.prepare_finish();
            tokio::task::spawn_blocking(move || cleanup.finish())
                .await
                .map_err(io::Error::other)??;
        }
        Ok(status)
    }
}

pub(super) struct JobCleanup {
    job: Option<Arc<ProcessJob>>,
    activity: Option<BridgeActivityContext>,
    ticket: Option<PendingExitTicket>,
    registration: Option<crate::bridge_registry::BridgeRegistration>,
}

impl JobCleanup {
    pub(super) fn track(
        mut self,
        registry: Option<&crate::bridge_registry::BridgeRegistry>,
    ) -> Self {
        if let (Some(registry), Some(job)) = (registry, self.job.as_ref()) {
            self.registration = Some(registry.register(job.clone()));
        }
        self
    }

    pub(super) fn cancellation_requested(&self) -> bool {
        self.registration
            .as_ref()
            .is_some_and(|entry| entry.cancellation_requested())
    }

    fn retire_registration(&mut self) {
        if self
            .registration
            .as_ref()
            .is_some_and(|registration| registration.retire())
        {
            self.job = None;
            if let Some(ticket) = self.ticket.take() {
                ticket.confirm_exit();
            }
        }
    }

    pub(super) fn prepare_finish(mut self) -> Self {
        self.retire_registration();
        if let (Some(activity), Some(job)) = (self.activity.take(), self.job.as_ref()) {
            self.ticket = Some(activity.retain_until_exit(job.clone()));
        }
        self
    }

    pub(super) fn finish(mut self) -> io::Result<()> {
        if let Some(job) = self.job.take() {
            job.terminate()?;
        }
        if let Some(ticket) = self.ticket.take() {
            ticket.confirm_exit();
        }
        Ok(())
    }
}

impl Drop for JobCleanup {
    fn drop(&mut self) {
        self.retire_registration();
        if let Some(job) = self.job.take() {
            let ticket = self.ticket.take().or_else(|| {
                self.activity
                    .take()
                    .map(|activity| activity.retain_until_exit(job.clone()))
            });
            // Request termination before scheduling, including runtime shutdown.
            let _ = job.request_termination();
            let _ = std::thread::Builder::new()
                .name("bridge-job-cleanup".into())
                .spawn(move || {
                    if job.wait_until_empty().is_ok() {
                        if let Some(ticket) = ticket {
                            ticket.confirm_exit();
                        }
                    }
                });
        }
    }
}
