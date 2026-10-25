//! The single persistence path enforces privacy filtering and bounded retention.
use super::DiagnosticErrorEvent;
use crate::security::diagnostic_metadata;
use crate::storage::AppState;
use rusqlite::{params, Connection};
use uuid::Uuid;

const DIAGNOSTIC_RETENTION_DAYS: i64 = 90;
const DIAGNOSTIC_MAX_EVENTS: i64 = 2_000;

pub(crate) fn record_error_event(state: &AppState, event: DiagnosticErrorEvent) {
    let Ok(conn) = state.db.lock() else {
        return;
    };
    let _ = record_error_event_conn(&conn, event);
}

/// Exit reporting must not wait for a background task's database lock.
pub(crate) fn try_record_error_event(
    state: &AppState,
    event: DiagnosticErrorEvent,
) -> Result<(), String> {
    try_record_error_event_db(&state.db, event)
}

fn try_record_error_event_db(
    db: &std::sync::Mutex<Connection>,
    event: DiagnosticErrorEvent,
) -> Result<(), String> {
    let conn = db.try_lock().map_err(|error| match error {
        std::sync::TryLockError::WouldBlock => "诊断数据库正忙，退出清理失败未能写入。".to_owned(),
        std::sync::TryLockError::Poisoned(_) => {
            "诊断数据库锁已损坏，退出清理失败未能写入。".to_owned()
        }
    })?;
    record_error_event_conn(&conn, event)
        .map_err(|error| format!("写入退出清理失败诊断出错：{error}"))
}

pub(crate) fn record_error_event_conn(
    conn: &Connection,
    event: DiagnosticErrorEvent,
) -> rusqlite::Result<()> {
    let raw_detail_json = serde_json::to_string(&diagnostic_metadata(event.raw_detail))
        .unwrap_or_else(|_| "{}".to_owned());
    let context_json = serde_json::to_string(&diagnostic_metadata(event.context))
        .unwrap_or_else(|_| "{}".to_owned());
    conn.execute(
        "insert into diagnostic_error_events(
           id, source, category, severity, profile_id, platform, operation,
           user_message, raw_detail_json, context_json, created_at
         )
         values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'))",
        params![
            Uuid::new_v4().to_string(),
            event.source,
            event.category,
            event.severity,
            event.profile_id,
            event.platform,
            event.operation,
            event.user_message,
            raw_detail_json,
            context_json
        ],
    )?;
    prune_diagnostic_error_events(conn)?;
    Ok(())
}

fn prune_diagnostic_error_events(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "delete from diagnostic_error_events
         where created_at < datetime('now', ?1)",
        params![format!("-{DIAGNOSTIC_RETENTION_DAYS} days")],
    )?;
    conn.execute(
        "delete from diagnostic_error_events
         where id in (
           select id
           from diagnostic_error_events
           order by created_at desc, id desc
           limit -1 offset ?1
         )",
        params![DIAGNOSTIC_MAX_EVENTS],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exit_fixture_event() -> DiagnosticErrorEvent {
        DiagnosticErrorEvent {
            source: "sync",
            category: "process_cleanup",
            severity: "error",
            profile_id: None,
            platform: None,
            operation: "sync_bridge_cleanup".into(),
            user_message: "无法确认进程退出".into(),
            raw_detail: serde_json::json!({"cleanup_error": "fixture cleanup failed"}),
            context: serde_json::json!({"lifecyclePhase": "exit_requested", "cleanupConfirmed": false}),
        }
    }

    #[test]
    fn exit_diagnostic_try_write_persists_actual_failure() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .unwrap();
        let db = std::sync::Mutex::new(conn);
        try_record_error_event_db(&db, exit_fixture_event()).unwrap();
        let row: (String, String, String, String) = db.lock().unwrap().query_row(
            "select category, severity, raw_detail_json, context_json from diagnostic_error_events",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        assert_eq!(row.0, "process_cleanup");
        assert_eq!(row.1, "error");
        let detail: serde_json::Value = serde_json::from_str(&row.2).unwrap();
        let context: serde_json::Value = serde_json::from_str(&row.3).unwrap();
        assert_eq!(detail, serde_json::json!({}));
        assert!(!row.2.contains("fixture cleanup failed"));
        assert_eq!(context["lifecyclePhase"], "exit_requested");
        assert_eq!(context["cleanupConfirmed"], false);
    }

    #[test]
    fn exit_diagnostic_busy_database_returns_without_waiting() {
        let db = std::sync::Mutex::new(Connection::open_in_memory().unwrap());
        let _held = db.lock().unwrap();
        let started = std::time::Instant::now();
        let error = try_record_error_event_db(&db, exit_fixture_event()).unwrap_err();
        assert!(error.contains("正忙"));
        assert!(started.elapsed() < std::time::Duration::from_millis(200));
    }

    #[test]
    fn exit_diagnostic_sql_failure_does_not_claim_persistence() {
        let db = std::sync::Mutex::new(Connection::open_in_memory().unwrap());
        let error = try_record_error_event_db(&db, exit_fixture_event()).unwrap_err();
        assert!(error.contains("写入退出清理失败诊断出错"));
        assert!(error.contains("diagnostic_error_events"));
    }

    #[test]
    fn diagnostic_retention_removes_expired_and_overflow_events() {
        let conn = Connection::open_in_memory().expect("open memory db");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("create schema");
        conn.execute(
            "insert into diagnostic_error_events(
               id, source, category, severity, operation, user_message, created_at
             ) values('expired', 'sync', 'test', 'warning', 'test', 'expired', datetime('now', '-91 days'))",
            [],
        )
        .expect("insert expired event");
        for index in 0..=DIAGNOSTIC_MAX_EVENTS {
            conn.execute(
                "insert into diagnostic_error_events(
                   id, source, category, severity, operation, user_message, created_at
                 ) values(?1, 'sync', 'test', 'warning', 'test', 'recent', datetime('now', ?2))",
                params![
                    format!("recent-{index:04}"),
                    format!("-{} seconds", DIAGNOSTIC_MAX_EVENTS - index)
                ],
            )
            .expect("insert recent event");
        }

        prune_diagnostic_error_events(&conn).expect("prune diagnostic events");

        let count: i64 = conn
            .query_row("select count(*) from diagnostic_error_events", [], |row| {
                row.get(0)
            })
            .expect("count events");
        let expired_count: i64 = conn
            .query_row(
                "select count(*) from diagnostic_error_events where id = 'expired'",
                [],
                |row| row.get(0),
            )
            .expect("count expired event");
        assert_eq!(count, DIAGNOSTIC_MAX_EVENTS);
        assert_eq!(expired_count, 0);
    }
}
