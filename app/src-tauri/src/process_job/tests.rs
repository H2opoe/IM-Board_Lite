use super::*;
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::mpsc;
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

fn wait_leader(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "fixture leader did not exit");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn suspended_child_is_assigned_and_resumes() {
    let mut command = Command::new("cmd.exe");
    command.args(["/d", "/c", "exit /b 7"]);
    let (mut child, job) = ProcessJob::spawn(command).unwrap();
    assert_eq!(wait_leader(&mut child).code(), Some(7));
    job.terminate().unwrap();
    assert_eq!(job.active_processes().unwrap(), 0);
}

fn tree<T>(
    parent_exits: bool,
    spawn: impl FnOnce(Command) -> (Child, T),
) -> (Child, T, OwnedHandle) {
    let tail = if parent_exits {
        "exit 0"
    } else {
        "Start-Sleep -Seconds 60"
    };
    let script = format!("$p = Start-Process -FilePath $env:ComSpec -ArgumentList '/d /c ping -n 60 127.0.0.1 >NUL' -NoNewWindow -PassThru; [Console]::WriteLine($p.Id); {tail}");
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (mut child, job) = spawn(command);
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = tx.send(result);
    });
    let line = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    let pid = line.trim().parse().unwrap();
    let handle = owned(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.unwrap());
    (child, job, handle)
}

fn assert_exited(handle: &OwnedHandle) {
    assert_eq!(
        unsafe { WaitForSingleObject(HANDLE(handle.as_raw_handle()), 5000) },
        WAIT_OBJECT_0
    );
}

#[test]
fn termination_cleans_descendants_after_parent_exit() {
    let (mut child, job, descendant) = tree(true, |command| ProcessJob::spawn(command).unwrap());
    assert!(wait_leader(&mut child).success());
    assert!(job.active_processes().unwrap() > 0);
    job.terminate().unwrap();
    assert_eq!(job.active_processes().unwrap(), 0);
    assert_exited(&descendant);
    job.terminate().unwrap();
}

#[test]
fn dropping_job_terminates_parent_and_descendants() {
    let (mut child, job, descendant) = tree(false, |command| ProcessJob::spawn(command).unwrap());
    drop(job);
    wait_leader(&mut child);
    assert_exited(&descendant);
}

#[test]
fn supervisor_observation_cleans_job_after_leader_exit() {
    let supervisor = crate::local_runtime_supervisor::LocalRuntimeSupervisor::new().unwrap();
    let (mut child, generation, descendant) =
        tree(true, |command| supervisor.spawn(command).unwrap());
    let status = wait_leader(&mut child);
    assert!(status.success());
    assert!(supervisor
        .observe_wait(child.id(), generation, &Ok(status))
        .unwrap());
    assert!(!supervisor.has_owned_process());
    assert_exited(&descendant);
}

#[test]
fn supervisor_stop_cleans_owned_job() {
    let supervisor = crate::local_runtime_supervisor::LocalRuntimeSupervisor::new().unwrap();
    let (mut child, generation, descendant) =
        tree(false, |command| supervisor.spawn(command).unwrap());
    assert!(supervisor.stop().unwrap());
    assert!(!supervisor.has_owned_process());
    assert_exited(&descendant);
    let status = wait_leader(&mut child);
    assert!(!supervisor
        .observe_wait(child.id(), generation, &Ok(status))
        .unwrap());
    assert!(!supervisor.stop().unwrap());
}
