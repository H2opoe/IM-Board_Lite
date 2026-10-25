//! Exit cleanup errors remain failures; reporting never claims tree exit.
use crate::diagnostics::DiagnosticErrorEvent;
use crate::storage::AppState;

#[derive(Clone, Copy, Debug)]
pub(crate) enum ExitPhase {
    Requested,
    Exiting,
}
impl ExitPhase {
    fn name(self) -> &'static str {
        match self {
            Self::Requested => "exit_requested",
            Self::Exiting => "exit",
        }
    }
}
#[derive(Debug)]
pub(crate) struct CleanupFailure {
    source: &'static str,
    operation: &'static str,
    error: String,
}

pub(crate) fn collect_failures(
    bridges: impl FnOnce() -> Result<bool, String>,
    runtime: impl FnOnce() -> Result<bool, String>,
) -> Vec<CleanupFailure> {
    let results = [
        ("sync", "sync_bridge_cleanup", bridges()),
        ("local_ai_runtime", "local_runtime_cleanup", runtime()),
    ];
    results
        .into_iter()
        .filter_map(|(source, operation, result)| {
            result.err().map(|error| CleanupFailure {
                source,
                operation,
                error,
            })
        })
        .collect()
}

fn error_event(phase: ExitPhase, failure: &CleanupFailure) -> DiagnosticErrorEvent {
    DiagnosticErrorEvent {
        source: failure.source,
        category: "process_cleanup",
        severity: "error",
        profile_id: None,
        platform: None,
        operation: failure.operation.to_owned(),
        user_message: "应用退出时无法确认后台进程已退出。".to_owned(),
        raw_detail: serde_json::json!({ "cleanup_error": failure.error }),
        context: serde_json::json!({ "lifecyclePhase": phase.name(), "cleanupConfirmed": false }),
    }
}

pub(crate) fn report_failures(state: &AppState, phase: ExitPhase, failures: Vec<CleanupFailure>) {
    report_failures_with(
        phase,
        failures,
        |event| crate::diagnostics::try_record_error_event(state, event),
        |line| eprintln!("{line}"),
    );
}

fn report_failures_with(
    phase: ExitPhase,
    failures: Vec<CleanupFailure>,
    mut record: impl FnMut(DiagnosticErrorEvent) -> Result<(), String>,
    mut stderr: impl FnMut(&str),
) {
    for failure in failures {
        let detail = crate::security::sanitize_log(&format!(
            "应用退出清理未确认 [{}:{}]：{}",
            phase.name(),
            failure.operation,
            failure.error
        ));
        stderr(&detail);
        if let Err(error) = record(error_event(phase, &failure)) {
            let error = crate::security::sanitize_log(&format!(
                "退出清理失败诊断未能写入 [{}:{}]：{}",
                phase.name(),
                failure.operation,
                error
            ));
            stderr(&error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn first_failure_does_not_skip_runtime_cleanup() {
        let attempts = RefCell::new(Vec::new());
        let failures = collect_failures(
            || {
                attempts.borrow_mut().push("bridge");
                Err("bridge fixture failed".into())
            },
            || {
                attempts.borrow_mut().push("runtime");
                Ok(true)
            },
        );
        assert_eq!(*attempts.borrow(), ["bridge", "runtime"]);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].operation, "sync_bridge_cleanup");
        assert_eq!(failures[0].error, "bridge fixture failed");
    }

    #[test]
    fn both_failures_keep_separate_causes_and_phase() {
        let failures = collect_failures(
            || Err("bridge fixture failed".into()),
            || Err("runtime fixture failed".into()),
        );
        assert_eq!(failures.len(), 2);
        for phase in [ExitPhase::Requested, ExitPhase::Exiting] {
            let first = error_event(phase, &failures[0]);
            let second = error_event(phase, &failures[1]);
            assert_eq!(first.source, "sync");
            assert_eq!(second.source, "local_ai_runtime");
            assert_eq!(first.category, "process_cleanup");
            assert_eq!(first.severity, "error");
            assert!(first.user_message.contains("无法确认"));
            assert_eq!(first.context["lifecyclePhase"], phase.name());
            assert_eq!(first.context["cleanupConfirmed"], false);
            assert_eq!(first.raw_detail["cleanup_error"], "bridge fixture failed");
            assert_eq!(second.raw_detail["cleanup_error"], "runtime fixture failed");
            assert_eq!(second.operation, "local_runtime_cleanup");
        }
    }

    #[test]
    fn success_and_no_owned_process_are_not_errors() {
        for bridge in [false, true] {
            for runtime in [false, true] {
                assert!(collect_failures(|| Ok(bridge), || Ok(runtime)).is_empty());
            }
        }
    }
    #[test]
    fn rejected_diagnostic_write_reports_cleanup_and_persistence_errors() {
        let failures = collect_failures(
            || Err("bridge fixture failed token-fixturePrivateSecret".into()),
            || Err("runtime fixture failed".into()),
        );
        let mut attempted = Vec::new();
        let mut stderr = Vec::new();
        report_failures_with(
            ExitPhase::Exiting,
            failures,
            |event| {
                attempted.push(event.operation);
                Err("database fixture is busy token-fixturePrivateSecret".into())
            },
            |line| stderr.push(line.to_owned()),
        );
        assert_eq!(attempted, ["sync_bridge_cleanup", "local_runtime_cleanup"]);
        assert_eq!(stderr.len(), 4);
        assert!(stderr[0].contains("bridge fixture failed"));
        assert!(stderr[1].contains("诊断未能写入"));
        assert!(stderr[2].contains("runtime fixture failed"));
        assert!(stderr[3].contains("database fixture is busy"));
        assert!(stderr.iter().all(|line| line.contains("exit:")));
        assert!(stderr
            .iter()
            .all(|line| !line.contains("fixturePrivateSecret")));
    }
}
