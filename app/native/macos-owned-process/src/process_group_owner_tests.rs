use super::*;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

fn fixture(script: &str) -> (Arc<ProcessOwner>, Option<std::process::ChildStdout>) {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", script])
        .process_group(0)
        .stdout(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take();
    (ProcessOwner::from_blocking(child), stdout)
}
#[test]
fn observation_keeps_zombie_anchor_and_final_child_status() {
    for exit in [0, 7] {
        let (owner, _) = fixture(&format!("exit {exit}"));
        assert_eq!(owner.wait_blocking().unwrap().code(), Some(exit));
        assert_eq!(unsafe { libc::kill(owner.pid() as i32, 0) }, 0);
        assert!(only_owned_leader(owner.pid()).unwrap());
        assert!(owner.confirm_exit().unwrap());
        assert_eq!(owner.wait_blocking().unwrap().code(), Some(exit));
        owner.terminate().unwrap();
        assert!(owner.state.lock().unwrap().child.is_none());
    }
}
#[test]
fn live_descendant_blocks_reaping_and_term_escalates_without_losing_anchor() {
    let (owner, stdout) = fixture("sh -c 'trap \"\" TERM; echo ready; exec sleep 30' & exit 7");
    let mut line = String::new();
    BufReader::new(stdout.unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "ready");
    assert_eq!(owner.wait_blocking().unwrap().code(), Some(7));
    assert!(!owner.confirm_exit().unwrap());
    assert_eq!(unsafe { libc::kill(owner.pid() as i32, 0) }, 0);
    owner.terminate().unwrap();
    assert_eq!(owner.wait_blocking().unwrap().code(), Some(7));
}
#[test]
fn truncated_large_group_never_confirms_exit_but_can_be_terminated() {
    let (owner, stdout) =
        fixture("i=0; while [ $i -lt 96 ]; do sleep 30 & i=$((i+1)); done; echo ready; wait");
    let mut line = String::new();
    BufReader::new(stdout.unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "ready");
    assert!(!only_owned_leader(owner.pid()).unwrap());
    assert!(!owner.confirm_exit().unwrap());
    owner.terminate().unwrap();
    assert!(owner.confirm_exit().unwrap());
}
#[test]
fn concurrent_forks_cannot_create_a_sole_leader_false_positive() {
    let (owner, stdout) = fixture("echo ready; i=0; while [ $i -lt 80 ]; do ( sleep 0.04 & wait ) & i=$((i+1)); sleep 0.005; done; wait");
    let mut line = String::new();
    BufReader::new(stdout.unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "ready");
    for _ in 0..30 {
        assert!(!owner.confirm_exit().unwrap());
        std::thread::sleep(Duration::from_millis(2));
    }
    owner.terminate().unwrap();
    assert!(owner.confirm_exit().unwrap());
}
#[tokio::test]
async fn external_reap_disarms_signals_and_never_drops_a_tokio_kill_guard() {
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        .args(["-c", "exit 7"])
        .process_group(0)
        .kill_on_drop(true);
    let mut owner = ProcessOwner::from_async(command.spawn().unwrap());
    assert_eq!(owner.wait_async().await.unwrap().code(), Some(7));
    let mut status = 0;
    // Reap this test-owned zombie to simulate an unsupported external reaper.
    assert_eq!(
        unsafe { libc::waitpid(owner.pid() as i32, &mut status, 0) },
        owner.pid() as i32
    );
    assert!(owner.terminate().is_err());
    assert!(owner.state.lock().unwrap().identity_lost);
    let mut replacement = Command::new("/bin/sleep");
    replacement.arg("30").process_group(0);
    let mut replacement = replacement.spawn().unwrap();
    // Substitute an owned fixture number; no actual system ID reuse is forced.
    Arc::get_mut(&mut owner).unwrap().pid = replacement.id();
    assert!(owner.signal(libc::SIGKILL).is_err());
    assert!(owner.confirm_exit().is_err());
    drop(owner); // the invalid Tokio guard is forgotten, never auto-signalled
    assert!(replacement.try_wait().unwrap().is_none());
    replacement.kill().unwrap();
    replacement.wait().unwrap();
}
#[test]
fn observer_does_not_hold_cancellation_lock_until_exit() {
    let (owner, _) = fixture("exec sleep 30");
    let observing = owner.clone();
    let waiter = std::thread::spawn(move || observing.wait_blocking().unwrap());
    let started = std::time::Instant::now();
    owner.terminate().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!waiter.join().unwrap().success());
    assert!(owner.confirm_exit().unwrap());
}

#[test]
fn preserves_first_exit_observation_when_child_finally_reaps() {
    use std::os::unix::process::ExitStatusExt;
    const FIXTURE: &str = "IMBOARD_AUDIT_EXTENDED_EXIT";
    if std::env::var_os(FIXTURE).is_some() {
        unsafe { libc::_exit(0x0100_0007) };
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    let test_name = format!(
        "{}::preserves_first_exit_observation_when_child_finally_reaps",
        module_path!().split_once("::").unwrap().1
    );
    command
        .args(["--exact", &test_name])
        .env(FIXTURE, "1")
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let owner = ProcessOwner::from_blocking(command.spawn().unwrap());
    let observed = owner.wait_blocking().unwrap();
    assert_eq!(observed.code(), Some(7));
    assert!(owner.confirm_exit().unwrap());
    assert_eq!(
        owner.wait_blocking().unwrap().into_raw(),
        observed.into_raw()
    );
}

#[test]
fn preserves_term_and_kill_exit_status_after_final_reap() {
    use std::os::unix::process::ExitStatusExt;
    for signal in [libc::SIGTERM, libc::SIGKILL] {
        let (owner, _) = fixture("exec sleep 30");
        owner.signal(signal).unwrap();
        assert_eq!(owner.wait_blocking().unwrap().signal(), Some(signal));
        assert!(owner.confirm_exit().unwrap());
        assert_eq!(owner.wait_blocking().unwrap().signal(), Some(signal));
    }
}
