use super::*;

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .unwrap();
    conn.execute_batch(include_str!("../../migrations/005_action_detected_day.sql"))
        .expect("action detection schema");
    conn.execute_batch("update app_meta set value='2000-01-01' where key='current_day';
        insert into daily_messages(id,day,profile_id,platform,chat_id,chat_name,sender_id,sender_name,timestamp,time_text,msg_type,content,content_hash)
        values('m','2000-01-01','p','feishu','c','fixture','u','fixture',1,'00:00','text','fixture','hash');
        insert into action_items(id,type,status,priority,title,description,profile_id,platform,chat_id,chat_name,source_message_ids,first_detected_at,last_updated_at)
        values('done','task','done','low','fixture','','p','feishu','c','fixture','[\"m\"]','2000-01-01','2000-01-01'),
              ('open','task','open','low','fixture','','p','feishu','c','fixture','[\"m\"]','2000-01-01','2000-01-01');
        insert into daily_stats(id,day,profile_id,metric,value_json,updated_at) values('s','2000-01-01','p','fixture','{}','fixture');
        insert into daily_topics(id,day,profile_id,title,summary,updated_at) values('t','2000-01-01','p','fixture','','fixture');
        insert into sync_state(profile_id,day,updated_at) values('p','2000-01-01','fixture');
        insert into ai_analysis_runs(id,day,profile_id,status,created_at) values('pending','2000-01-01','p','pending','fixture'),('finished','2000-01-01','p','done','fixture');").unwrap();
    conn
}

fn counts(conn: &Connection) -> Vec<i64> {
    [
        "action_items",
        "daily_messages",
        "daily_stats",
        "daily_topics",
        "sync_state",
        "ai_analysis_runs",
    ]
    .into_iter()
    .map(|table| {
        conn.query_row(&format!("select count(*) from {table}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect()
}

#[test]
fn rollover_rolls_back_every_stage_and_can_retry() {
    for trigger in [
        "create trigger injected before delete on daily_topics begin select raise(abort,'fixture'); end;",
        "create trigger injected before delete on sync_state begin select raise(abort,'fixture'); end;",
        "create trigger injected before update on app_meta when old.key='current_day' begin select raise(abort,'fixture'); end;",
    ] {
        let conn = fixture();
        let before = counts(&conn);
        conn.execute_batch(trigger).unwrap();
        assert!(rollover_with(&conn, |_| Ok(snapshot("2026-09-30"))).is_err());
        assert_eq!(counts(&conn), before, "partial cleanup survived: {trigger}");
        assert_eq!(conn.query_row("select value from app_meta where key='current_day'", [], |r| r.get::<_,String>(0)).unwrap(), "2000-01-01");
        assert!(conn.is_autocommit());
        conn.execute_batch("drop trigger injected").unwrap();
        let (result, _) = rollover_with(&conn, |_| Ok(snapshot("2026-09-30"))).unwrap();
        assert!(result.rolled_over);
        assert_eq!(result.previous_day, "2000-01-01");
        assert_eq!(counts(&conn), vec![1,0,0,0,0,1]);
        assert_eq!(conn.query_row("select id from action_items", [], |r| r.get::<_,String>(0)).unwrap(), "open");
        assert!(!rollover_with(&conn, |_| Ok(snapshot("2026-09-30"))).unwrap().0.rolled_over);
        assert_eq!(counts(&conn), vec![1,0,0,0,0,1]);
    }
}

#[test]
fn standalone_cleanup_also_rolls_back_a_late_failure() {
    let conn = fixture();
    let before = counts(&conn);
    conn.execute_batch("create trigger injected before delete on daily_topics begin select raise(abort,'fixture'); end;").unwrap();
    {
        let tx = conn.unchecked_transaction().unwrap();
        assert!(clear_expired_daily_cache(&tx, "2026-09-30").is_err());
    }
    assert_eq!(counts(&conn), before);
    assert!(conn.is_autocommit());
}

fn snapshot(day: &str) -> DashboardDay {
    DashboardDay {
        day: day.into(),
        day_start_timestamp: 10,
        day_start_text: format!("{day} 00:00:00"),
        sync_end_timestamp: 20,
        sync_end_text: format!("{day} 00:00:10"),
    }
}

#[test]
fn rollover_returns_the_same_snapshot_for_cleanup_and_the_caller() {
    let conn = fixture();
    // The first request is before the threshold; the next is after it.
    // Each request must use its own single snapshot for both cleanup and reads.
    for (day, expected_rollover) in [("2000-01-01", false), ("2000-01-02", true)] {
        let calls = std::cell::Cell::new(0);
        let (result, actual) = rollover_with(&conn, |_| {
            calls.set(calls.get() + 1);
            Ok(snapshot(day))
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(result.rolled_over, expected_rollover);
        assert_eq!(result.current_day, actual.day);
        assert_eq!(actual.day, day);
        assert_eq!(actual.day_start_timestamp, 10);
        assert_eq!(actual.sync_end_timestamp, 20);
        assert_eq!(actual.day_start_text, format!("{day} 00:00:00"));
        assert_eq!(actual.sync_end_text, format!("{day} 00:00:10"));
        let stored: String = conn
            .query_row(
                "select value from app_meta where key='current_day'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, actual.day);
        assert!(conn.is_autocommit());
    }
}
