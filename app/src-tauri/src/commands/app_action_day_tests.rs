use super::*;

#[test]
fn diagnostic_day_counts_exclude_legacy_uncertainty_and_report_it_once() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .unwrap();
    conn.execute_batch(include_str!("../../migrations/005_action_detected_day.sql"))
        .unwrap();
    conn.execute_batch("insert into daily_messages(id,day,profile_id,platform,chat_id,chat_name,sender_id,sender_name,timestamp,time_text,msg_type,content,content_hash) values('m29','2026-09-29','p','wecom','c','fixture','u','fixture',1,'time','text','fixture','h29'),('m30','2026-09-30','p','wecom','c','fixture','u','fixture',2,'time','text','fixture','h30');
        insert into action_items(id,type,status,priority,title,description,profile_id,platform,chat_id,chat_name,first_detected_at,last_updated_at,detected_day) values
        ('known','task','done','medium','fixture','','p','wecom','c','fixture','2026-09-29T23:30:00-04:00','now','2026-09-29'),
        ('uncertain','task','done','medium','fixture','','p','wecom','c','fixture','2026-09-30 01:15:00','now',null),
        ('unknown','task','done','medium','fixture','','p','wecom','c','fixture','invalid','now',null);").unwrap();
    let actions = crate::daily_cache::action_day::resolve_action_days(&conn).unwrap();
    let summary = collect_daily_summary(&conn, &actions).unwrap();
    assert_eq!(summary.len(), 2);
    assert_eq!(
        summary
            .iter()
            .find(|row| row["day"] == "2026-09-29")
            .unwrap()["actionCount"],
        1
    );
    assert_eq!(
        summary
            .iter()
            .find(|row| row["day"] == "2026-09-30")
            .unwrap()["actionCount"],
        0
    );
    let uncertainty = crate::daily_cache::action_day::diagnostic_day_summary(&actions);
    assert_eq!(
        uncertainty["dailySummaryActionCountScope"],
        "confirmed-day-only"
    );
    assert_eq!(uncertainty["profiles"].as_array().unwrap().len(), 1);
    assert_eq!(uncertainty["profiles"][0]["confirmedActionCount"], 1);
    assert_eq!(uncertainty["profiles"][0]["uncertainActionCount"], 1);
    assert_eq!(uncertainty["profiles"][0]["unassignedActionCount"], 1);
    assert!(summary
        .iter()
        .all(|row| row.get("uncertainActionCount").is_none()));
}
