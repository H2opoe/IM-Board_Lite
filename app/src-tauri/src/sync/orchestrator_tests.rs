use super::*;

#[test]
fn aggregate_status_reports_partial_and_failed_truthfully() {
    assert_eq!(aggregate_sync_status([true, true]), "synced");
    assert_eq!(aggregate_sync_status([true, false]), "partial");
    assert_eq!(aggregate_sync_status([false, false]), "failed");
}
use chrono::NaiveDate;

use crate::analysis::orchestrator::apply_context_evidence_conn;
use crate::messages::repository::{delete_regenerable_action_items, reset_ai_generated_cache_conn};
use crate::storage::models::ImProfile;

#[test]
fn reset_ai_generated_cache_clears_generated_state_and_requeues_messages() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    let profile = test_profile("profile-1");
    conn.execute(
        "insert into daily_messages(
           id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
           timestamp, time_text, msg_type, content, content_hash, analyzed_at
         )
         values('msg-1', '2026-05-01', 'profile-1', 'feishu', 'chat-1', '测试群', 1,
                'u-1', '用户', cast(strftime('%s', '2026-05-01 09:00:00') as integer), '09:00',
                'text', '需要重新生成', 'hash-1', datetime('now'))",
        [],
    )
    .expect("message");
    conn.execute(
        "insert into action_items(
           id, type, status, priority, title, description, profile_id, platform, chat_id, chat_name,
           source_message_ids, evidence_summary, first_detected_at, last_updated_at
         )
         values('act-1', 'task', 'open', 'medium', '旧事项', '旧描述', 'profile-1', 'feishu',
                'chat-1', '测试群', '[\"msg-1\"]', '旧证据', datetime('now'), datetime('now'))",
        [],
    )
    .expect("action item");
    conn.execute(
        "insert into daily_stats(id, day, profile_id, metric, value_json, updated_at)
         values('stat-1', '2026-05-01', 'profile-1', 'topics', '[]', datetime('now')),
               ('stat-2', '2026-05-01', 'profile-1', 'keywords', '[]', datetime('now')),
               ('stat-3', '2026-05-01', 'aggregate', 'topics', '[]', datetime('now'))",
        [],
    )
    .expect("stats");
    conn.execute(
        "insert into daily_topics(id, day, profile_id, title, summary, updated_at)
         values('topic-1', '2026-05-01', 'profile-1', '旧话题', '旧摘要', datetime('now'))",
        [],
    )
    .expect("topic");
    conn.execute(
        "insert into ai_analysis_runs(id, day, profile_id, input_message_ids, status, created_at)
         values('run-1', '2026-05-01', 'profile-1', '[\"msg-1\"]', 'done', datetime('now'))",
        [],
    )
    .expect("run");
    conn.execute(
        "insert into sync_state(profile_id, day, last_analysis_at, updated_at)
         values('profile-1', '2026-05-01', datetime('now'), datetime('now'))",
        [],
    )
    .expect("sync state");

    let day = test_dashboard_day("2026-05-01");
    reset_ai_generated_cache_conn(&conn, &day, &[profile]).expect("reset");

    let analyzed_at: Option<String> = conn
        .query_row(
            "select analyzed_at from daily_messages where id = 'msg-1'",
            [],
            |row| row.get(0),
        )
        .expect("analyzed_at");
    assert!(analyzed_at.is_none());
    assert_eq!(count_rows(&conn, "action_items"), 0);
    assert_eq!(count_rows(&conn, "daily_stats"), 0);
    assert_eq!(count_rows(&conn, "daily_topics"), 0);
    assert_eq!(count_rows(&conn, "ai_analysis_runs"), 0);
    let last_analysis_at: Option<String> = conn
        .query_row(
            "select last_analysis_at from sync_state where profile_id = 'profile-1'",
            [],
            |row| row.get(0),
        )
        .expect("last_analysis_at");
    assert!(last_analysis_at.is_none());
}

#[test]
fn reset_ai_generated_cache_preserves_historical_open_reply_and_task() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    let profile = test_profile("profile-1");
    conn.execute(
        "insert into daily_messages(
           id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
           timestamp, time_text, msg_type, content, content_hash, analyzed_at
         )
         values('msg-today', '2026-05-01', 'profile-1', 'feishu', 'chat-1', '测试群', 1,
                'u-1', '用户', cast(strftime('%s', '2026-05-01 09:00:00') as integer), '09:00',
                'text', '今天需要重新生成', 'hash-today', datetime('now'))",
        [],
    )
    .expect("message");
    conn.execute(
        "insert into action_items(
           id, type, status, priority, title, description, profile_id, platform, chat_id,
           chat_name, source_message_ids, evidence_summary, carry_over, first_detected_at, last_updated_at
         )
         values('act-today', 'task', 'open', 'medium', '今日事项', '今日描述',
                'profile-1', 'feishu', 'chat-1', '测试群', '[\"msg-today\"]',
                '今日证据', 1, '2026-05-01 09:00:00', '2026-05-01 09:00:00'),
               ('act-history-reply', 'reply', 'open', 'high', '历史待回复', '历史描述',
                'profile-1', 'feishu', 'chat-old', '旧聊天', '[\"msg-old\"]',
                '历史证据', 1, '2026-04-30 09:00:00', '2026-04-30 09:00:00'),
               ('act-history-done', 'task', 'done', 'low', '已完成历史事项', '已完成描述',
                'profile-1', 'feishu', 'chat-old', '旧聊天', '[\"msg-old-done\"]',
                '已完成证据', 1, '2026-04-30 09:00:00', '2026-04-30 09:00:00')",
        [],
    )
    .expect("action items");

    let day = test_dashboard_day("2026-05-01");
    reset_ai_generated_cache_conn(&conn, &day, &[profile]).expect("reset");

    assert_eq!(action_item_ids(&conn), vec!["act-history-reply".to_owned()]);
}

#[test]
fn reset_ai_generated_cache_preserves_historical_item_by_chat_time() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    let profile = test_profile("profile-1");
    conn.execute(
        "insert into daily_messages(
           id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
           timestamp, time_text, msg_type, content, content_hash, analyzed_at
         )
         values('msg-yesterday', '2026-05-05', 'profile-1', 'feishu', 'chat-1', '客户群', 1,
                'u-1', '客户', cast(strftime('%s', '2026-05-05 18:30:00') as integer), '18:30',
                'text', '昨天产生但今天才进入看板的待办', 'hash-yesterday', datetime('now')),
               ('msg-today', '2026-05-06', 'profile-1', 'feishu', 'chat-1', '客户群', 1,
                'u-1', '客户', cast(strftime('%s', '2026-05-06 09:30:00') as integer), '09:30',
                'text', '今天产生的待办', 'hash-today', datetime('now'))",
        [],
    )
    .expect("messages");
    conn.execute(
        "insert into action_items(
           id, type, status, priority, title, description, profile_id, platform, chat_id,
           chat_name, source_message_ids, evidence_summary, carry_over, first_detected_at, last_updated_at
         )
         values('act-yesterday', 'task', 'open', 'medium', '昨天聊天待办', '今天才进入看板',
                'profile-1', 'feishu', 'chat-1', '客户群', '[\"msg-yesterday\"]',
                '昨天证据', 1, '2026-05-06 10:00:00', '2026-05-06 10:00:00'),
               ('act-today', 'reply', 'open', 'high', '今天待回复', '今天聊天产生',
                'profile-1', 'feishu', 'chat-1', '客户群', '[\"msg-today\"]',
                '今天证据', 1, '2026-05-06 10:00:00', '2026-05-06 10:00:00')",
        [],
    )
    .expect("action items");

    let day = test_dashboard_day("2026-05-06");
    reset_ai_generated_cache_conn(&conn, &day, &[profile]).expect("reset");

    assert_eq!(action_item_ids(&conn), vec!["act-yesterday".to_owned()]);
}

#[test]
fn full_cache_clear_preserves_historical_open_reply_and_task() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into daily_messages(
           id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id,
           sender_name, timestamp, time_text, msg_type, content, content_hash
         )
         values('msg-today', '2026-05-01', 'profile-1', 'feishu', 'chat-1', '测试聊天', 1,
                'u-1', '用户', cast(strftime('%s', '2026-05-01 09:00:00') as integer),
                '09:00', 'text', '今日待回复', 'hash-today')",
        [],
    )
    .expect("message");
    conn.execute(
        "insert into action_items(
           id, type, status, priority, title, description, profile_id, platform, chat_id,
           chat_name, source_message_ids, evidence_summary, carry_over, first_detected_at, last_updated_at
         )
         values('act-history-task', 'task', 'open', 'medium', '历史待办', '历史描述',
                'profile-1', 'feishu', 'chat-old', '旧聊天', '[\"msg-old\"]',
                '历史证据', 1, '2026-04-30 09:00:00', '2026-04-30 09:00:00'),
               ('act-today-reply', 'reply', 'open', 'medium', '今日待回复', '今日描述',
                'profile-1', 'feishu', 'chat-1', '测试聊天', '[\"msg-today\"]',
                '今日证据', 1, '2026-05-01 09:00:00', '2026-05-01 09:00:00'),
               ('act-history-attention', 'attention', 'open', 'low', '历史关注', '历史描述',
                'profile-1', 'feishu', 'chat-old', '旧聊天', '[\"msg-old-attention\"]',
                '历史关注证据', 1, '2026-04-30 09:00:00', '2026-04-30 09:00:00')",
        [],
    )
    .expect("action items");

    let day = test_dashboard_day("2026-05-01");
    delete_regenerable_action_items(&conn, &day).expect("delete regenerable items");

    assert_eq!(action_item_ids(&conn), vec!["act-history-task".to_owned()]);
}

#[test]
fn context_evidence_backfill_marks_action_complete_and_merges_summary() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into action_items(
           id, type, status, priority, title, description, profile_id, platform, chat_id,
           chat_name, source_message_ids, evidence_summary, context_incomplete,
           first_detected_at, last_updated_at
         )
         values('act-context', 'task', 'open', 'medium', '需要补证据', '描述',
                'profile-1', 'feishu', 'chat-1', '测试群', '[\"msg-1\"]',
                '原始证据', 1, '2026-05-01 09:00:00', '2026-05-01 09:00:00')",
        [],
    )
    .expect("action item");

    apply_context_evidence_conn(
        &conn,
        "act-context",
        &[
            "[2026-04-30 09:10] 张三: 昨天已经确认方案".to_owned(),
            "[2026-04-29 18:20] 李四: 等今天同步后执行".to_owned(),
        ],
    )
    .expect("apply context evidence");

    let (evidence_summary, context_incomplete): (String, i64) = conn
        .query_row(
            "select evidence_summary, context_incomplete from action_items where id = 'act-context'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("context evidence");
    assert!(evidence_summary.contains("原始证据"));
    assert!(evidence_summary.contains("上下文补读"));
    assert!(evidence_summary.contains("昨天已经确认方案"));
    assert_eq!(context_incomplete, 0);
}

#[test]
fn expired_daily_cache_clear_keeps_today_and_finished_history_only() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute_batch(include_str!("../../migrations/005_action_detected_day.sql"))
        .expect("action day schema");
    conn.execute(
        "insert into daily_messages(
           id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id,
           sender_name, timestamp, time_text, msg_type, content, content_hash
         )
         values('msg-old', '2026-04-30', 'profile-1', 'feishu', 'chat-old', '旧群', 1,
                'u-1', '用户', 1, '09:00', 'text', '旧消息', 'hash-old'),
               ('msg-today', '2026-05-01', 'profile-1', 'feishu', 'chat-1', '今日群', 1,
                'u-1', '用户', 2, '10:00', 'text', '今日消息', 'hash-today')",
        [],
    )
    .expect("messages");
    conn.execute(
        "insert into daily_stats(id, day, profile_id, metric, value_json, updated_at)
         values('stat-old', '2026-04-30', 'profile-1', 'keywords', '[]', datetime('now')),
               ('stat-today', '2026-05-01', 'profile-1', 'keywords', '[]', datetime('now'))",
        [],
    )
    .expect("stats");
    conn.execute(
        "insert into daily_topics(id, day, profile_id, title, summary, updated_at)
         values('topic-old', '2026-04-30', 'profile-1', '旧话题', '旧摘要', datetime('now')),
               ('topic-today', '2026-05-01', 'profile-1', '今日话题', '今日摘要', datetime('now'))",
        [],
    )
    .expect("topics");
    conn.execute(
        "insert into ai_analysis_runs(id, day, profile_id, input_message_ids, status, created_at)
         values('run-old-pending', '2026-04-30', 'profile-1', '[]', 'pending', datetime('now')),
               ('run-old-running', '2026-04-30', 'profile-1', '[]', 'running', datetime('now')),
               ('run-old-done', '2026-04-30', 'profile-1', '[]', 'done', datetime('now')),
               ('run-today-running', '2026-05-01', 'profile-1', '[]', 'running', datetime('now'))",
        [],
    )
    .expect("runs");
    conn.execute(
        "insert into sync_state(profile_id, day, updated_at)
         values('profile-1', '2026-04-30', datetime('now')),
               ('profile-1', '2026-05-01', datetime('now'))",
        [],
    )
    .expect("sync state");

    let tx = conn.unchecked_transaction().unwrap();
    daily_cache::clear_expired_daily_cache(&tx, "2026-05-01").expect("clear cache");
    tx.commit().unwrap();
    daily_cache::reset_daily_sync_state(&conn, "2026-05-01").expect("reset sync state");

    assert_eq!(daily_message_ids(&conn), vec!["msg-today".to_owned()]);
    assert_eq!(daily_stat_ids(&conn), vec!["stat-today".to_owned()]);
    assert_eq!(daily_topic_ids(&conn), vec!["topic-today".to_owned()]);
    assert_eq!(
        ai_run_ids(&conn),
        vec!["run-old-done".to_owned(), "run-today-running".to_owned()]
    );
    assert_eq!(sync_state_days(&conn), vec!["2026-05-01".to_owned()]);
}

fn test_profile(id: &str) -> ImProfile {
    ImProfile {
        id: id.to_owned(),
        platform: "feishu".to_owned(),
        label: "飞书".to_owned(),
        enabled: true,
        config_json: serde_json::json!({}),
        status: "normal".to_owned(),
        sort_order: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn count_rows(conn: &rusqlite::Connection, table: &str) -> i64 {
    conn.query_row(&format!("select count(*) from {table}"), [], |row| {
        row.get(0)
    })
    .expect("count")
}

fn test_dashboard_day(day: &str) -> daily_cache::DashboardDay {
    let day_start_timestamp = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .expect("day")
        .and_hms_opt(0, 0, 0)
        .expect("day start")
        .and_utc()
        .timestamp();
    daily_cache::DashboardDay {
        day: day.to_owned(),
        day_start_timestamp,
        day_start_text: format!("{day} 00:00:00"),
        sync_end_timestamp: day_start_timestamp + 86_399,
        sync_end_text: format!("{day} 23:59:59"),
    }
}

fn action_item_ids(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("select id from action_items order by id")
        .expect("prepare action ids");
    stmt.query_map([], |row| row.get::<_, String>(0))
        .expect("query action ids")
        .collect::<Result<Vec<_>, _>>()
        .expect("action ids")
}

fn daily_message_ids(conn: &rusqlite::Connection) -> Vec<String> {
    ids_from_query(conn, "select id from daily_messages order by id")
}

fn daily_stat_ids(conn: &rusqlite::Connection) -> Vec<String> {
    ids_from_query(conn, "select id from daily_stats order by id")
}

fn daily_topic_ids(conn: &rusqlite::Connection) -> Vec<String> {
    ids_from_query(conn, "select id from daily_topics order by id")
}

fn ai_run_ids(conn: &rusqlite::Connection) -> Vec<String> {
    ids_from_query(conn, "select id from ai_analysis_runs order by id")
}

fn sync_state_days(conn: &rusqlite::Connection) -> Vec<String> {
    ids_from_query(conn, "select day from sync_state order by day")
}

fn ids_from_query(conn: &rusqlite::Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).expect("prepare ids");
    stmt.query_map([], |row| row.get::<_, String>(0))
        .expect("query ids")
        .collect::<Result<Vec<_>, _>>()
        .expect("ids")
}

#[test]
fn no_accounts_is_idle_instead_of_a_failed_sync() {
    assert_eq!(aggregate_sync_status([]), "idle");
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .unwrap();
    assert!(resolve_target_profiles(&conn, "aggregate")
        .unwrap()
        .is_empty());
    let mut profile = test_profile("disabled");
    profile.enabled = false;
    conn.execute("insert into profiles(id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at) values(?1, ?2, ?3, 0, '{}', 'normal', 0, 'fixture', 'fixture')", rusqlite::params![profile.id, profile.platform, profile.label]).unwrap();
    assert!(resolve_target_profiles(&conn, "aggregate")
        .unwrap()
        .is_empty());
    conn.execute("update profiles set enabled = 1", []).unwrap();
    assert_eq!(
        resolve_target_profiles(&conn, "aggregate").unwrap().len(),
        1
    );
    for ai_status in ["not_configured", "ready"] {
        let result = empty_sync_result("aggregate".into(), ai_status);
        assert_eq!(result.sync_status, "idle");
        assert_eq!(result.ai_status, ai_status);
        assert_eq!(result.inserted_messages, 0);
        assert_eq!(result.analyzed_messages, 0);
        assert!(result.warnings.is_empty());
    }
}
