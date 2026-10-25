use super::*;
use std::os::unix::process::CommandExt;
use std::process::Command;

#[test]
fn rejects_auto_reap_sigchld_policy_before_runtime_spawn() {
    const POLICY: &str = "IMBOARD_AUDIT_SIGCHLD_POLICY";
    const SENTINEL: &str = "IMBOARD_AUDIT_SIGCHLD_SENTINEL";
    if let Some(policy) = std::env::var_os(POLICY) {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = if policy == "ignore" {
            libc::SIG_IGN
        } else {
            libc::SIG_DFL
        };
        action.sa_flags = if policy == "no-cld-wait" {
            libc::SA_NOCLDWAIT
        } else {
            0
        };
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) },
            0
        );
        assert!(check_supported().is_err());
        let supervisor = crate::local_runtime_supervisor::LocalRuntimeSupervisor::new().unwrap();
        let mut command = Command::new("/usr/bin/touch");
        command.arg(std::env::var_os(SENTINEL).unwrap());
        assert!(supervisor.spawn(command).is_err());
        assert!(!supervisor.has_owned_process());
        return;
    }
    for policy in ["ignore", "no-cld-wait"] {
        let sentinel = std::env::temp_dir().join(format!(
            "imboard-sigchld-{}-{policy}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!sentinel.exists());
        let test_name = format!(
            "{}::rejects_auto_reap_sigchld_policy_before_runtime_spawn",
            module_path!().split_once("::").unwrap().1
        );
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &test_name])
            .env(POLICY, policy)
            .env(SENTINEL, &sentinel)
            .process_group(0);
        let owner = ProcessOwner::from_blocking(command.spawn().unwrap());
        assert!(owner.wait_blocking().unwrap().success());
        assert!(owner.confirm_exit().unwrap());
        assert!(
            !sentinel.exists(),
            "unsupported policy must fail before spawn"
        );
    }
}
