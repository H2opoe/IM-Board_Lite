use super::*;

fn database() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .unwrap();
    conn.execute_batch(include_str!(
        "../../../migrations/005_action_detected_day.sql"
    ))
    .unwrap();
    conn.execute("insert into daily_messages(id,day,profile_id,platform,chat_id,chat_name,sender_id,sender_name,timestamp,time_text,msg_type,content,content_hash) values('message','2026-09-29','p','wecom','c','fixture','me','me',1,'time','text','fixture','hash')",[]).unwrap();
    conn
}

fn analysis(existing: Option<&str>, status: &str) -> AiAnalysis {
    serde_json::from_value(serde_json::json!({"actionItems":[{
        "type":"reply","status":status,"priority":"medium","title":"fixture",
        "description":"fixture","profileId":"p","chatId":"c",
        "existingActionItemId":existing,"sourceMessageIds":["missing-message"]
    }]}))
    .unwrap()
}

#[test]
fn first_detection_day_is_saved_and_not_replaced_by_later_analysis() {
    let conn = database();
    persist_analysis_across_profiles(&conn, "2026-09-29", &[], analysis(None, "open")).unwrap();
    let (id, first, day): (String, String, String) = conn
        .query_row(
            "select id,first_detected_at,detected_day from action_items",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(day, "2026-09-29");
    chrono::DateTime::parse_from_rfc3339(&first).unwrap();
    conn.execute("update daily_messages set day='2026-09-30'", [])
        .unwrap();
    persist_analysis_across_profiles(&conn, "2026-09-30", &[], analysis(Some(&id), "done"))
        .unwrap();
    let (updated_first, updated_day, status): (String, String, String) = conn
        .query_row(
            "select first_detected_at,detected_day,status from action_items",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(updated_first, first);
    assert_eq!(updated_day, day);
    assert_eq!(status, "done");
}

#[test]
fn updating_legacy_item_does_not_invent_its_original_day() {
    let conn = database();
    persist_analysis_across_profiles(&conn, "2026-09-29", &[], analysis(None, "open")).unwrap();
    conn.execute("update action_items set detected_day=null", [])
        .unwrap();
    let id: String = conn
        .query_row("select id from action_items", [], |r| r.get(0))
        .unwrap();
    persist_analysis_across_profiles(&conn, "2026-09-29", &[], analysis(Some(&id), "done"))
        .unwrap();
    assert_eq!(
        conn.query_row("select detected_day from action_items", [], |r| r
            .get::<_, Option<String>>(0))
            .unwrap(),
        None
    );
}

#[test]
fn invalid_analysis_day_and_failed_insert_leave_no_partial_state() {
    let conn = database();
    assert!(
        persist_analysis_across_profiles(&conn, "2026-02-30", &[], analysis(None, "open")).is_err()
    );
    assert_eq!(
        conn.query_row("select count(*) from action_items", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    conn.execute_batch("create trigger fail_action before insert on action_items begin select raise(abort,'fixture'); end;").unwrap();
    assert!(
        persist_analysis_across_profiles(&conn, "2026-09-29", &[], analysis(None, "open")).is_err()
    );
    assert_eq!(
        conn.query_row("select count(*) from action_items", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("select count(*) from sync_state", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(conn.is_autocommit());
}
