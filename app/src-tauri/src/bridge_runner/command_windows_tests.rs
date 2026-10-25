use super::spawn_bridge as spawn;
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0},
    System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
};

#[tokio::test]
async fn assigned_process_resumes_and_preserves_exit_code() {
    let mut command = tokio::process::Command::new("cmd.exe");
    command.args(["/d", "/c", "exit /b 7"]);
    let mut child = spawn(command).unwrap();
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(7));
}

async fn tree(parent_exits: bool) -> (super::BridgeChild, Vec<OwnedHandle>) {
    let mut command = tokio::process::Command::new("powershell.exe");
    let tail = if parent_exits {
        "exit 0"
    } else {
        "Start-Sleep -Seconds 60"
    };
    let script = format!("$p = Start-Process -FilePath $env:ComSpec -ArgumentList '/d /c ping -n 60 127.0.0.1 >NUL' -NoNewWindow -PassThru; [Console]::WriteLine($p.Id); {tail}");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(command).unwrap();
    let parent = child.id().unwrap();
    let mut line = String::new();
    let mut output = BufReader::new(child.stdout().take().unwrap());
    tokio::time::timeout(Duration::from_secs(10), output.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    let descendant: u32 = line.trim().parse().unwrap();
    let handles = [parent, descendant]
        .into_iter()
        .map(|pid| {
            // Keep a handle to the exact fixture process, avoiding PID-reuse polling.
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.unwrap();
            unsafe { OwnedHandle::from_raw_handle(handle.0) }
        })
        .collect();
    (child, handles)
}

fn assert_exited(handles: Vec<OwnedHandle>) {
    for handle in handles {
        let status = unsafe { WaitForSingleObject(HANDLE(handle.as_raw_handle()), 5000) };
        assert_eq!(
            status, WAIT_OBJECT_0,
            "fixture process survived closing its job"
        );
    }
}

#[tokio::test]
async fn timeout_drops_job_and_terminates_parent_and_descendant() {
    let (mut child, handles) = tree(false).await;
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        async move { child.wait().await },
    )
    .await;
    assert!(result.is_err());
    assert_exited(handles);
}

#[tokio::test]
async fn cancelled_task_drops_job_and_terminates_descendant() {
    let (mut child, handles) = tree(false).await;
    let task = tokio::spawn(async move { child.wait().await });
    tokio::task::yield_now().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_exited(handles);
}

fn assert_exited_now(handles: &[OwnedHandle]) {
    for handle in handles {
        assert_eq!(
            unsafe { WaitForSingleObject(HANDLE(handle.as_raw_handle()), 0) },
            WAIT_OBJECT_0,
            "bridge returned before its process tree exited"
        );
    }
}

#[tokio::test]
async fn completed_bridge_cleans_descendants_after_leader_exit() {
    let (child, handles) = tree(true).await;
    let output = super::wait_for_official_cli_output(child, None, Duration::from_secs(10))
        .await
        .unwrap();
    assert!(output.status.success());
    assert_exited_now(&handles);
}

#[tokio::test]
async fn timed_out_bridge_confirms_job_exit_before_returning() {
    let (child, handles) = tree(false).await;
    assert!(
        super::wait_for_official_cli_output(child, None, Duration::from_millis(30))
            .await
            .is_err()
    );
    assert_exited_now(&handles);
}

#[tokio::test]
async fn cancelled_bridge_keeps_account_protection_until_job_exit() {
    let activity =
        std::sync::Arc::new(crate::profile_manager::activity::ProfileActivity::default());
    let scope = activity.bridge_scope().unwrap();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(scope.run(async move {
        let (child, handles) = tree(false).await;
        ready_tx.send(handles).unwrap();
        super::wait_for_official_cli_output(child, None, Duration::from_secs(60)).await
    }));
    let handles = tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .unwrap()
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(_deletion) = activity.begin_delete() {
                assert_exited_now(&handles);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn registry_cancellation_terminates_the_owned_job() {
    let (child, handles) = tree(false).await;
    let registry = std::sync::Arc::new(crate::bridge_registry::BridgeRegistry::default());
    let waiting_registry = registry.clone();
    let waiting = tokio::spawn(async move {
        super::wait_for_official_cli_output(child, Some(&waiting_registry), Duration::from_secs(60))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while registry.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let cancelling_registry = registry.clone();
    assert!(
        tokio::task::spawn_blocking(move || cancelling_registry.cancel_all())
            .await
            .unwrap()
            .unwrap()
    );
    assert_exited_now(&handles);
    tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(registry.is_empty());
    assert!(!registry.cancel_all().unwrap());
}
