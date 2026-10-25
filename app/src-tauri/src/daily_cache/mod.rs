pub(crate) mod action_day;
mod day;
use day::current_dashboard_day_info;
pub use day::DashboardDay;

use rusqlite::{params, Connection, OptionalExtension};

pub const DEFAULT_CACHE_CLEAR_MINUTES: i64 = 0;
const CACHE_CLEAR_MINUTES_KEY: &str = "cache_clear_minutes";

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolloverResult {
    pub rolled_over: bool,
    pub previous_day: String,
    pub current_day: String,
}

pub fn detect_day_rollover(conn: &Connection) -> anyhow::Result<(RolloverResult, DashboardDay)> {
    rollover_with(conn, current_dashboard_day_info)
}

fn rollover_with(
    conn: &Connection,
    resolve_day: impl FnOnce(&Connection) -> anyhow::Result<DashboardDay>,
) -> anyhow::Result<(RolloverResult, DashboardDay)> {
    let tx = conn.unchecked_transaction()?;
    let snapshot = resolve_day(&tx)?;
    let today = snapshot.day.clone();
    let stored: String = tx.query_row(
        "select value from app_meta where key = 'current_day'",
        [],
        |row| row.get(0),
    )?;

    if stored == today {
        return Ok((
            RolloverResult {
                rolled_over: false,
                previous_day: stored,
                current_day: today,
            },
            snapshot,
        ));
    }

    clear_expired_daily_cache(&tx, &today)?;
    reset_daily_sync_state(&tx, &today)?;
    tx.execute(
        "insert into app_meta(key, value, updated_at) values('current_day', ?1, datetime('now'))
         on conflict(key) do update set value = excluded.value, updated_at = excluded.updated_at",
        params![today],
    )?;

    tx.commit()?;

    Ok((
        RolloverResult {
            rolled_over: true,
            previous_day: stored,
            current_day: today,
        },
        snapshot,
    ))
}

// Call while holding the database mutex, and keep it held through the write.
// A completed rollover invalidates work that was started for another day.
pub(crate) fn ensure_current_day(conn: &Connection, expected: &str) -> anyhow::Result<()> {
    let current: String = conn.query_row(
        "select value from app_meta where key = 'current_day'",
        [],
        |row| row.get(0),
    )?;
    anyhow::ensure!(
        current == expected,
        "看板日期已切换，请重新同步今天的数据。"
    );
    Ok(())
}

// Require the caller's transaction so cleanup cannot be accidentally split
// across independently committed statements.
pub(crate) fn clear_expired_daily_cache(
    conn: &rusqlite::Transaction<'_>,
    today: &str,
) -> anyhow::Result<()> {
    clear_expired_action_items(conn, today)?;
    conn.execute("delete from daily_messages where day <> ?1", params![today])?;
    conn.execute("delete from daily_stats where day <> ?1", params![today])?;
    conn.execute("delete from daily_topics where day <> ?1", params![today])?;
    conn.execute(
        "delete from ai_analysis_runs where day <> ?1 and status in ('pending', 'running')",
        params![today],
    )?;
    Ok(())
}

fn clear_expired_action_items(conn: &Connection, today: &str) -> anyhow::Result<()> {
    // 每日清理只延续历史未完成的待回复/待办；已完成、忽略和其他类型随过期看板数据一起移除。
    for action in action_day::resolve_action_days(conn)? {
        if action.day.expired_on(today) {
            conn.execute(
                "delete from action_items where id = ?1
                 and not (status = 'open' and type in ('reply', 'task'))",
                params![action.id],
            )?;
        }
    }
    Ok(())
}

pub fn reset_daily_sync_state(conn: &Connection, today: &str) -> anyhow::Result<()> {
    conn.execute("delete from sync_state where day <> ?1", params![today])?;
    Ok(())
}

pub fn cache_clear_minutes(conn: &Connection) -> anyhow::Result<i64> {
    let stored: Option<String> = conn
        .query_row(
            "select value from app_meta where key = ?1",
            params![CACHE_CLEAR_MINUTES_KEY],
            |row| row.get(0),
        )
        .optional()?;
    Ok(stored
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|minutes| (0..1440).contains(minutes))
        .unwrap_or(DEFAULT_CACHE_CLEAR_MINUTES))
}

pub fn set_cache_clear_minutes(conn: &Connection, minutes: i64) -> anyhow::Result<()> {
    let minutes = minutes.clamp(0, 1439);
    conn.execute(
        "insert into app_meta(key, value, updated_at) values(?1, ?2, datetime('now'))
         on conflict(key) do update set value = excluded.value, updated_at = excluded.updated_at",
        params![CACHE_CLEAR_MINUTES_KEY, minutes.to_string()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_daily_cache_clears_finished_history_and_keeps_open_items() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute_batch(include_str!("../../migrations/005_action_detected_day.sql"))
            .expect("action detection schema");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id,
               sender_name, timestamp, time_text, msg_type, content, content_hash
             )
             values('msg-old-open', '2026-05-08', 'profile-1', 'feishu', 'chat-1', '旧群', 1,
                    'u-1', '用户', 1, '09:00', 'text', '旧未完成事项', 'hash-old-open'),
                   ('msg-old-done', '2026-05-08', 'profile-1', 'feishu', 'chat-1', '旧群', 1,
                    'u-1', '用户', 2, '10:00', 'text', '旧已完成事项', 'hash-old-done'),
                   ('msg-today-done', '2026-05-09', 'profile-1', 'feishu', 'chat-1', '今日群', 1,
                    'u-1', '用户', 3, '11:00', 'text', '今日已完成事项', 'hash-today-done')",
            [],
        )
        .expect("messages");
        conn.execute(
            "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id,
               chat_name, source_message_ids, evidence_summary, carry_over, first_detected_at, last_updated_at,
               completed_at
             )
             values('act-old-open', 'task', 'open', 'medium', '旧未完成', '旧未完成描述',
                    'profile-1', 'feishu', 'chat-1', '旧群', '[\"msg-old-open\"]',
                    '旧未完成证据', 1, '2026-05-08 09:00:00', '2026-05-08 09:00:00', null),
                   ('act-old-done', 'task', 'done', 'medium', '旧已完成', '旧已完成描述',
                    'profile-1', 'feishu', 'chat-1', '旧群', '[\"msg-old-done\"]',
                    '旧已完成证据', 1, '2026-05-08 10:00:00', '2026-05-08 10:30:00',
                    '2026-05-08 10:30:00'),
                   ('act-old-ignored', 'reply', 'ignored', 'low', '旧已忽略', '旧已忽略描述',
                    'profile-1', 'feishu', 'chat-1', '旧群', '[\"msg-old-done\"]',
                    '旧已忽略证据', 1, '2026-05-08 12:00:00', '2026-05-08 12:00:00', null),
                   ('act-today-done', 'reply', 'done', 'high', '今日已完成', '今日已完成描述',
                    'profile-1', 'feishu', 'chat-1', '今日群', '[\"msg-today-done\"]',
                    '今日已完成证据', 1, '2026-05-09 11:00:00', '2026-05-09 11:30:00',
                    '2026-05-09 11:30:00'),
                   ('act-legacy-open', 'reply', 'open', 'high', '旧版未完成', '旧版未完成描述',
                    'profile-1', 'feishu', 'chat-1', '旧群', '[]',
                    '旧版未完成证据', 0, '2026-05-07 08:00:00', '2026-05-07 08:00:00', null),
                   ('act-legacy-done', 'reply', 'done', 'high', '旧版已完成', '旧版已完成描述',
                    'profile-1', 'feishu', 'chat-1', '旧群', '[]',
                    '旧版已完成证据', 1, '2026-05-07 08:00:00', '2026-05-07 08:30:00',
                    '2026-05-07 08:30:00')",
            [],
        )
        .expect("action items");

        let tx = conn.unchecked_transaction().unwrap();
        clear_expired_daily_cache(&tx, "2026-05-09").expect("clear expired cache");
        tx.commit().unwrap();

        assert_eq!(
            action_item_ids(&conn),
            vec![
                "act-legacy-open".to_owned(),
                "act-old-open".to_owned(),
                "act-today-done".to_owned()
            ]
        );
        assert_eq!(daily_message_ids(&conn), vec!["msg-today-done".to_owned()]);
    }

    fn action_item_ids(conn: &Connection) -> Vec<String> {
        ids_from_query(conn, "select id from action_items order by id")
    }

    fn daily_message_ids(conn: &Connection) -> Vec<String> {
        ids_from_query(conn, "select id from daily_messages order by id")
    }

    fn ids_from_query(conn: &Connection, sql: &str) -> Vec<String> {
        let mut stmt = conn.prepare(sql).expect("prepare ids query");
        stmt.query_map([], |row| row.get::<_, String>(0))
            .expect("query ids")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect ids")
    }
}

#[cfg(test)]
mod rollover_tests;

#[cfg(test)]
#[path = "action_day_rollover_tests.rs"]
mod action_day_rollover_tests;
