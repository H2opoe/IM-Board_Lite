use super::*;

fn message(id: &str, day: &str) -> DailyMessage {
    DailyMessage {
        id: id.into(),
        day: day.into(),
        profile_id: "p".into(),
        platform: "feishu".into(),
        chat_id: "c".into(),
        chat_name: "fixture".into(),
        is_group: false,
        sender_id: "u".into(),
        sender_name: "fixture".into(),
        timestamp: 1,
        time_text: "00:00:00".into(),
        msg_type: "text".into(),
        content: "fixture".into(),
        raw_type: None,
        local_id: None,
        raw_json: "{}".into(),
        content_hash: id.into(),
        partial: false,
    }
}

#[test]
fn late_messages_cannot_reappear_after_rollover() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .unwrap();
    conn.execute_batch(include_str!("../../migrations/005_action_detected_day.sql"))
        .expect("action day schema");
    conn.execute(
        "update app_meta set value='1900-01-01' where key='current_day'",
        [],
    )
    .unwrap();
    assert_eq!(
        insert_messages_conn(&conn, &[message("old", "1900-01-01")]).unwrap(),
        1
    );
    let (_, day) = daily_cache::detect_day_rollover(&conn).unwrap();
    assert_ne!(day.day, "1900-01-01");
    assert!(insert_messages_conn(&conn, &[message("late", "1900-01-01")]).is_err());
    let count: i64 = conn
        .query_row("select count(*) from daily_messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        insert_messages_conn(&conn, &[message("current", &day.day)]).unwrap(),
        1
    );
}

#[test]
fn mixed_day_batch_rolls_back_before_any_new_rows_survive() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .unwrap();
    conn.execute(
        "update app_meta set value='2000-01-02' where key='current_day'",
        [],
    )
    .unwrap();
    assert!(insert_messages_conn(
        &conn,
        &[
            message("current", "2000-01-02"),
            message("stale", "2000-01-01")
        ]
    )
    .is_err());
    let count: i64 = conn
        .query_row("select count(*) from daily_messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
