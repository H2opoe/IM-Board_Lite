use super::*;

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .unwrap();
    conn.execute_batch(include_str!("../../migrations/005_action_detected_day.sql"))
        .unwrap();
    conn
}

fn add_action(conn: &Connection, id: &str, detected: Option<&str>, first: &str) {
    conn.execute(
        "insert into action_items(id,type,status,priority,title,description,profile_id,
         platform,chat_id,chat_name,first_detected_at,last_updated_at,detected_day)
         values(?1,'task','done','medium','fixture','','p','wecom','c','fixture',?2,?2,?3)",
        rusqlite::params![id, first, detected],
    )
    .unwrap();
}

#[test]
fn resolved_source_day_precedes_first_detection_day() {
    let conn = fixture();
    add_action(&conn, "a", Some("2026-09-29"), "invalid");
    conn.execute("insert into daily_messages(id,day,profile_id,platform,chat_id,chat_name,sender_id,sender_name,timestamp,time_text,msg_type,content,content_hash) values('m','2026-09-30','p','wecom','c','fixture','u','fixture',1,'time','text','fixture','hash')", []).unwrap();
    conn.execute(
        "update action_items set source_message_ids='[\"m\"]' where id='a'",
        [],
    )
    .unwrap();
    assert_eq!(
        resolve_action_days(&conn).unwrap()[0].day,
        ActionDay::Known("2026-09-30".to_owned())
    );
}

#[test]
fn stored_detection_day_survives_clear_time_changes() {
    let conn = fixture();
    add_action(&conn, "a", Some("2026-09-29"), "2026-09-30T02:30:00+00:00");
    for minutes in [0, 180, 1380] {
        super::super::set_cache_clear_minutes(&conn, minutes).unwrap();
        let actions = resolve_action_days(&conn).unwrap();
        assert_eq!(actions[0].day, ActionDay::Known("2026-09-29".to_owned()));
        assert!(actions[0].day.expired_on("2026-09-30"));
    }
}

#[test]
fn invalid_dates_use_legacy_candidates_or_remain_unassigned() {
    let conn = fixture();
    add_action(&conn, "invalid-time", None, "not-a-time");
    add_action(
        &conn,
        "invalid-day",
        Some("2026-02-30"),
        "2026-09-30T02:30:00Z",
    );
    let actions = resolve_action_days(&conn).unwrap();
    assert_eq!(
        actions
            .iter()
            .filter(|action| action.day == ActionDay::Unknown)
            .count(),
        1
    );
    assert_eq!(
        actions
            .iter()
            .filter(|action| matches!(action.day, ActionDay::LegacyCandidates(_)))
            .count(),
        1
    );
    assert!(actions
        .iter()
        .all(|action| !action.day.expired_on("2026-09-30")));
    let summary = diagnostic_day_summary(&actions);
    assert_eq!(summary["profiles"].as_array().unwrap().len(), 1);
    assert_eq!(summary["profiles"][0]["unassignedActionCount"], 1);
    assert_eq!(summary["profiles"][0]["uncertainActionCount"], 1);
}

#[test]
fn invalid_source_day_does_not_mask_a_valid_detection_day() {
    let conn = fixture();
    add_action(&conn, "a", Some("2026-09-29"), "invalid");
    conn.execute("insert into daily_messages(id,day,profile_id,platform,chat_id,chat_name,sender_id,sender_name,timestamp,time_text,msg_type,content,content_hash) values('m','invalid','p','wecom','c','fixture','u','fixture',1,'time','text','fixture','hash')", []).unwrap();
    conn.execute(
        "update action_items set source_message_ids='[\"m\"]' where id='a'",
        [],
    )
    .unwrap();
    let actions = resolve_action_days(&conn).unwrap();
    assert_eq!(actions[0].day, ActionDay::Known("2026-09-29".to_owned()));
    assert!(!actions[0].day.expired_on("2026-09-29"));
    assert!(actions[0].day.expired_on("2026-09-30"));
    let summary = diagnostic_day_summary(&actions);
    assert_eq!(summary["profiles"][0]["confirmedActionCount"], 1);
    assert_eq!(summary["profiles"][0]["unassignedActionCount"], 0);
}

#[test]
fn invalid_detection_day_uses_old_timestamp_conservatively() {
    let conn = fixture();
    add_action(&conn, "a", Some("2026-02-30"), "2026-09-30 12:00:00");
    let actions = resolve_action_days(&conn).unwrap();
    let ActionDay::LegacyCandidates(days) = &actions[0].day else {
        panic!("invalid stored day must fall back to the old timestamp")
    };
    assert!(days.contains("2026-09-29") && days.contains("2026-09-30"));
    assert!(!actions[0].day.expired_on("2026-09-30"));
    assert!(actions[0].day.expired_on("2026-10-01"));
    let summary = diagnostic_day_summary(&actions);
    assert_eq!(summary["profiles"][0]["confirmedActionCount"], 0);
    assert_eq!(summary["profiles"][0]["uncertainActionCount"], 1);
    assert_eq!(summary["profiles"][0]["unassignedActionCount"], 0);
}

#[test]
fn malformed_source_json_is_an_observable_sql_error() {
    let conn = fixture();
    add_action(&conn, "a", Some("2026-09-29"), "invalid");
    conn.execute("update action_items set source_message_ids='broken'", [])
        .unwrap();
    assert!(resolve_action_days(&conn).is_err());
}

#[test]
fn canonical_days_reject_non_dates_and_non_canonical_input() {
    for value in ["2026-02-30", "2026-9-30", "2026-09-30Z", ""] {
        assert!(canonical_day(value).is_none(), "{value}");
    }
    assert_eq!(canonical_day("2024-02-29"), Some("2024-02-29".to_owned()));
}

#[cfg(unix)]
#[test]
fn legacy_fold_gap_and_business_day_candidates_use_isolated_timezone() {
    const MARKER: &str = "IM_BOARD_ACTION_DAY_ZONE_FIXTURE";
    if std::env::var(MARKER).as_deref() == Ok("1") {
        let conn = fixture();
        super::super::set_cache_clear_minutes(&conn, 90).unwrap();
        add_action(&conn, "fold", None, "2026-11-01 01:15:00");
        let actions = resolve_action_days(&conn).unwrap();
        let ActionDay::LegacyCandidates(days) = &actions[0].day else {
            panic!("fold must retain both candidates")
        };
        assert!(days.contains("2026-10-31") && days.contains("2026-11-01"));
        assert!(!actions[0].day.expired_on("2026-11-01"));
        assert!(actions[0].day.expired_on("2026-11-02"));
        conn.execute("delete from action_items", []).unwrap();
        add_action(&conn, "gap", None, "2026-03-08 02:15:00");
        assert_eq!(
            resolve_action_days(&conn).unwrap()[0].day,
            ActionDay::Unknown
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "daily_cache::action_day::tests::legacy_fold_gap_and_business_day_candidates_use_isolated_timezone",
            "--nocapture",
        ])
        .env("TZ", "America/New_York")
        .env(MARKER, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
