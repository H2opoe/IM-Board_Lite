use super::*;

fn database(old_day: &str) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .unwrap();
    conn.execute_batch(include_str!("../../migrations/005_action_detected_day.sql"))
        .unwrap();
    conn.execute(
        "update app_meta set value=?1 where key='current_day'",
        [old_day],
    )
    .unwrap();
    conn
}

fn action(conn: &Connection, id: &str, status: &str, detected: Option<&str>, first: &str) {
    conn.execute("insert into action_items(id,type,status,priority,title,description,profile_id,platform,chat_id,chat_name,first_detected_at,last_updated_at,detected_day) values(?1,'reply',?2,'medium','fixture','','p','wecom','c','fixture',?3,?3,?4)",params![id,status,first,detected]).unwrap();
}

fn ids(conn: &Connection) -> Vec<String> {
    conn.prepare("select id from action_items order by id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn known_detection_days_and_same_day_reads_use_isolated_timezones() {
    const MARKER: &str = "IM_BOARD_ACTION_ROLLOVER_ZONE";
    if std::env::var_os(MARKER).is_some() {
        use chrono::{Local, NaiveDateTime, TimeZone};
        for (minutes, before, after) in [
            (0, "2026-09-29 23:30:00", "2026-09-30 00:01:00"),
            (180, "2026-09-30 02:30:00", "2026-09-30 03:01:00"),
            (1380, "2026-09-30 22:30:00", "2026-09-30 23:01:00"),
        ] {
            let at = |value: &str| {
                Local
                    .from_local_datetime(
                        &NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").unwrap(),
                    )
                    .single()
                    .unwrap()
            };
            let before = at(before);
            let after = at(after);
            let old = day::dashboard_day_at(before, minutes).unwrap().day;
            let current = day::dashboard_day_at(after, minutes).unwrap().day;
            assert_ne!(old, current);
            let conn = database(&old);
            set_cache_clear_minutes(&conn, minutes).unwrap();
            action(&conn, "old", "done", Some(&old), &before.to_rfc3339());
            action(
                &conn,
                "current",
                "done",
                Some(&current),
                &after.to_rfc3339(),
            );
            action(&conn, "open", "open", Some(&old), &before.to_rfc3339());
            action(&conn, "unknown", "done", None, "invalid");
            let result = rollover_with(&conn, |_| day::dashboard_day_at(after, minutes)).unwrap();
            assert!(result.0.rolled_over);
            assert_eq!(ids(&conn), vec!["current", "open", "unknown"]);
            conn.execute_batch("create trigger no_same_day_cleanup before delete on action_items begin select raise(abort,'same-day cleanup'); end;").unwrap();
            assert!(
                !rollover_with(&conn, |_| day::dashboard_day_at(after, minutes))
                    .unwrap()
                    .0
                    .rolled_over
            );
            assert_eq!(ids(&conn), vec!["current", "open", "unknown"]);
        }
        return;
    }
    for zone in [
        "Asia/Shanghai",
        "America/New_York",
        "UTC",
        "Pacific/Kiritimati",
    ] {
        let output=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","daily_cache::action_day_rollover_tests::known_detection_days_and_same_day_reads_use_isolated_timezones","--nocapture"])
            .env("TZ",zone).env(MARKER,"1").output().unwrap();
        assert!(
            output.status.success(),
            "{zone}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn invalid_day_fields_share_cleanup_and_diagnostic_fallback_rules() {
    let conn = database("2026-09-29");
    action(&conn, "valid-stored", "done", Some("2026-09-29"), "invalid");
    action(
        &conn,
        "legacy",
        "done",
        Some("2026-02-30"),
        "2026-09-30 12:00:00",
    );
    conn.execute("insert into daily_messages(id,day,profile_id,platform,chat_id,chat_name,sender_id,sender_name,timestamp,time_text,msg_type,content,content_hash) values('m','invalid','p','wecom','c','fixture','u','fixture',1,'time','text','fixture','hash')", []).unwrap();
    conn.execute(
        "update action_items set source_message_ids='[\"m\"]' where id='valid-stored'",
        [],
    )
    .unwrap();
    let before =
        action_day::diagnostic_day_summary(&action_day::resolve_action_days(&conn).unwrap());
    assert_eq!(before["profiles"][0]["confirmedActionCount"], 1);
    assert_eq!(before["profiles"][0]["uncertainActionCount"], 1);
    rollover_with(&conn, |_| {
        Ok(DashboardDay {
            day: "2026-09-30".to_owned(),
            day_start_timestamp: 0,
            day_start_text: String::new(),
            sync_end_timestamp: 1,
            sync_end_text: String::new(),
        })
    })
    .unwrap();
    assert_eq!(ids(&conn), vec!["legacy"]);
    let after =
        action_day::diagnostic_day_summary(&action_day::resolve_action_days(&conn).unwrap());
    assert_eq!(after["profiles"][0]["confirmedActionCount"], 0);
    assert_eq!(after["profiles"][0]["uncertainActionCount"], 1);
    assert_eq!(after["profiles"][0]["unassignedActionCount"], 0);
}

#[test]
fn failed_action_delete_rolls_back_and_can_retry() {
    let conn = database("2026-09-29");
    action(
        &conn,
        "old",
        "done",
        Some("2026-09-29"),
        "2026-09-30T02:30:00Z",
    );
    conn.execute_batch("create trigger fail_action before delete on action_items begin select raise(abort,'fixture'); end;").unwrap();
    let snapshot = || DashboardDay {
        day: "2026-09-30".to_owned(),
        day_start_timestamp: 0,
        day_start_text: String::new(),
        sync_end_timestamp: 1,
        sync_end_text: String::new(),
    };
    assert!(rollover_with(&conn, |_| Ok(snapshot())).is_err());
    assert_eq!(ids(&conn), vec!["old"]);
    assert_eq!(
        conn.query_row(
            "select value from app_meta where key='current_day'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "2026-09-29"
    );
    assert!(conn.is_autocommit());
    conn.execute_batch("drop trigger fail_action").unwrap();
    rollover_with(&conn, |_| Ok(snapshot())).unwrap();
    assert!(ids(&conn).is_empty());
}
