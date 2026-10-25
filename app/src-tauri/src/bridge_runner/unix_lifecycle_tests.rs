use super::*;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

fn fixture(script: &str) -> BridgeChild {
    let mut command = official_cli_command(Path::new("/bin/sh"));
    command
        .args(["-c", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spawn_bridge(command).unwrap()
}
async fn ready(child: &mut BridgeChild) {
    use tokio::io::AsyncReadExt;
    let mut bytes = [0; 6];
    tokio::time::timeout(
        Duration::from_secs(3),
        child.stdout.as_mut().unwrap().read_exact(&mut bytes),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(&bytes, b"ready\n");
}
#[tokio::test]
async fn confirmed_registry_cancellation_disarms_later_group_cleanup() {
    let (owner, cleanup, _, _, _) = fixture("exec sleep 30").into_parts();
    let registry = BridgeRegistry::default();
    let cleanup = cleanup.track(Some(&registry));
    assert!(registry.cancel_all().unwrap());
    cleanup.prepare_finish().finish().unwrap();
    assert!(registry.is_empty());
    assert!(owner.confirm_exit().unwrap());
    owner.terminate().unwrap();
}
#[tokio::test]
async fn prepared_cleanup_blocks_retry_before_worker_is_scheduled() {
    let activity = crate::profile_manager::activity::ProfileActivity::default();
    activity
        .bridge_scope()
        .unwrap()
        .run(async {
            let (owner, cleanup, _, _, _) = fixture("exit 0").into_parts();
            owner.wait_async().await.unwrap();
            let cleanup = cleanup.prepare_finish();
            assert!(activity.bridge_scope().is_err());
            cleanup.finish().unwrap();
            assert!(activity.bridge_scope().is_ok());
        })
        .await;
    assert!(activity.begin_delete().is_ok());
}
#[tokio::test]
async fn cancelled_bridge_holds_account_lease_until_tree_exit() {
    let activity = Arc::new(crate::profile_manager::activity::ProfileActivity::default());
    let registry = Arc::new(BridgeRegistry::default());
    let worker_registry = registry.clone();
    let scope = activity.bridge_scope().unwrap();
    let mut child = fixture("sh -c 'trap \"\" TERM; echo ready; exec sleep 30' & wait");
    ready(&mut child).await;
    let owner = child
        .cleanup
        .as_ref()
        .unwrap()
        .owner
        .as_ref()
        .unwrap()
        .clone();
    let task = tokio::spawn(scope.run(async move {
        wait_for_official_cli_output(child, Some(&worker_registry), Duration::from_secs(30)).await
    }));
    tokio::time::timeout(Duration::from_secs(3), async {
        while registry.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(deletion) = activity.begin_delete() {
                assert!(owner.confirm_exit().unwrap());
                drop(deletion);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(registry.is_empty());
}
#[tokio::test]
async fn completed_output_reaps_descendants_even_after_leader_exit() {
    for exit in [0, 7] {
        let child = fixture(&format!(
            "sh -c 'trap \"\" TERM; echo ready; exec sleep 30 >/dev/null 2>&1' & exit {exit}"
        ));
        let owner = child
            .cleanup
            .as_ref()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .clone();
        let registry = BridgeRegistry::default();
        let output = wait_for_official_cli_output(child, Some(&registry), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(exit));
        assert_eq!(output.stdout, b"ready\n");
        assert!(owner.confirm_exit().unwrap());
        assert!(registry.is_empty());
    }
}
#[tokio::test]
async fn timeout_and_output_error_confirm_exit_before_returning() {
    for limit in [0, 64] {
        let mut child = fixture("sh -c 'trap \"\" TERM; echo ready; exec sleep 30' & wait");
        use tokio::io::AsyncReadExt;
        let mut byte = [0];
        child
            .stdout
            .as_mut()
            .unwrap()
            .read_exact(&mut byte)
            .await
            .unwrap();
        let owner = child
            .cleanup
            .as_ref()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .clone();
        let activity = crate::profile_manager::activity::ProfileActivity::default();
        let registry = BridgeRegistry::default();
        let result = activity
            .bridge_scope()
            .unwrap()
            .run(wait_for_output_with_limits(
                child,
                Some(&registry),
                Duration::from_millis(30),
                None,
                limit,
                64,
            ))
            .await;
        if limit == 0 {
            assert_eq!(
                result.unwrap().unwrap_err().kind(),
                std::io::ErrorKind::InvalidData
            );
        } else {
            assert!(
                result.is_err()
                    || matches!(result, Ok(Err(ref error)) if error.kind() == std::io::ErrorKind::TimedOut)
            );
        }
        assert!(owner.confirm_exit().unwrap());
        assert!(activity.begin_delete().is_ok());
        assert!(registry.is_empty());
    }
}
#[tokio::test]
async fn dropping_child_before_wait_still_reaps_and_releases_account_lease() {
    let activity = crate::profile_manager::activity::ProfileActivity::default();
    let owner = activity
        .bridge_scope()
        .unwrap()
        .run(async {
            let child = fixture("exec sleep 30");
            let owner = child
                .cleanup
                .as_ref()
                .unwrap()
                .owner
                .as_ref()
                .unwrap()
                .clone();
            drop(child);
            owner
        })
        .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(deletion) = activity.begin_delete() {
                assert!(owner.confirm_exit().unwrap());
                drop(deletion);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn cleanup_failure_preserves_error_context() {
    let io_failure = Ok(Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "output limit",
    )));
    let error = bridge_cleanup_error(&io_failure, std::io::Error::other("cleanup failed"));
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("output limit"));
    assert!(error.to_string().contains("cleanup failed"));
    let timeout = tokio::time::timeout(
        Duration::ZERO,
        std::future::pending::<std::io::Result<std::process::Output>>(),
    )
    .await;
    assert_eq!(
        bridge_cleanup_error(&timeout, std::io::Error::other("fixture")).kind(),
        std::io::ErrorKind::TimedOut
    );
}
