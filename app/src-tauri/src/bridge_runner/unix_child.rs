use super::ProcessGroupCleanup;
use crate::process_group::ProcessOwner;
use std::sync::Arc;

type Parts = (
    Arc<ProcessOwner>,
    ProcessGroupCleanup,
    Option<tokio::process::ChildStdin>,
    Option<tokio::process::ChildStdout>,
    Option<tokio::process::ChildStderr>,
);

pub(crate) struct BridgeChild {
    pub(super) stdin: Option<tokio::process::ChildStdin>,
    pub(super) stdout: Option<tokio::process::ChildStdout>,
    pub(super) stderr: Option<tokio::process::ChildStderr>,
    pub(super) cleanup: Option<ProcessGroupCleanup>,
}

impl From<tokio::process::Child> for BridgeChild {
    fn from(mut child: tokio::process::Child) -> Self {
        let (stdin, stdout, stderr) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take());
        Self {
            stdin,
            stdout,
            stderr,
            cleanup: Some(ProcessGroupCleanup::new(ProcessOwner::from_async(child))),
        }
    }
}
impl BridgeChild {
    pub(super) fn into_parts(mut self) -> Parts {
        let cleanup = self.cleanup.take().expect("owned bridge child");
        (
            cleanup.owner.as_ref().expect("owned process").clone(),
            cleanup,
            self.stdin.take(),
            self.stdout.take(),
            self.stderr.take(),
        )
    }
}
