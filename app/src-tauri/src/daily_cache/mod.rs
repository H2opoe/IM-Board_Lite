use chrono::{Datelike, Duration, Local, TimeZone};
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

#[derive(Debug, Clone)]
pub struct DashboardDay {
    pub day: String,
    pub day_start_timestamp: i64,
    pub day_start_text: String,
    pub sync_end_timestamp: i64,
    pub sync_end_text: String,
}

pub fn detect_day_rollover(conn: &Connection) -> anyhow::Result<RolloverResult> {
    let today = current_dashboard_day(conn)?;
    let stored: String = conn.query_row(
        "select value from app_meta where key = 'current_day'",
        [],
        |row| row.get(0),
    )?;

    if stored == today {
        return Ok(RolloverResult {
            rolled_over: false,
            previous_day: stored,
            current_day: today,
        });
    }

    clear_expired_daily_cache(conn, &today)?;
    reset_daily_sync_state(conn, &today)?;
    conn.execute(
        "insert into app_meta(key, value, updated_at) values('current_day', ?1, datetime('now'))
         on conflict(key) do update set value = excluded.value, updated_at = excluded.updated_at",
        params![today],
    )?;

    Ok(RolloverResult {
        rolled_over: true,
        previous_day: stored,
        current_day: today,
    })
}

pub fn clear_expired_daily_cache(conn: &Connection, today: &str) -> anyhow::Result<()> {
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
    conn.execute(
        "delete from action_items
         where not (
           status = 'open'
           and type in ('reply', 'task')
           and carry_over = 1
         )
         and coalesce((
           select max(daily_messages.day)
           from daily_messages
           where daily_messages.id in (
             select value from json_each(action_items.source_message_ids)
           )
         ), date(first_detected_at)) <> ?1",
        params![today],
    )?;
    Ok(())
}

pub fn reset_daily_sync_state(conn: &Connection, today: &str) -> anyhow::Result<()> {
    conn.execute("delete from sync_state where day <> ?1", params![today])?;
    Ok(())
}

pub fn current_dashboard_day(conn: &Connection) -> anyhow::Result<String> {
    Ok(current_dashboard_day_info(conn)?.day)
}

pub fn current_dashboard_day_info(conn: &Connection) -> anyhow::Result<DashboardDay> {
    let minutes = cache_clear_minutes(conn)?;
    let now = Local::now();
    let day_date = if now < today_clear_at(minutes)? {
        now.date_naive() - Duration::days(1)
    } else {
        now.date_naive()
    };
    let day_start = Local
        .with_ymd_and_hms(day_date.year(), day_date.month(), day_date.day(), 0, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("无法计算看板日期开始时间。"))?;
    let day = day_date.format("%Y-%m-%d").to_string();

    Ok(DashboardDay {
        day: day.clone(),
        day_start_timestamp: day_start.timestamp(),
        day_start_text: format!("{day} 00:00:00"),
        sync_end_timestamp: now.timestamp(),
        sync_end_text: now.format("%Y-%m-%d %H:%M:%S").to_string(),
    })
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

fn today_clear_at(minutes: i64) -> anyhow::Result<chrono::DateTime<Local>> {
    let now = Local::now();
    let midnight = Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("无法计算缓存清理时间。"))?;
    Ok(midnight + Duration::minutes(minutes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_daily_cache_clears_finished_history_and_keeps_open_items() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
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
                    '旧版未完成证据', 1, '2026-05-07 08:00:00', '2026-05-07 08:00:00', null),
                   ('act-legacy-done', 'reply', 'done', 'high', '旧版已完成', '旧版已完成描述',
                    'profile-1', 'feishu', 'chat-1', '旧群', '[]',
                    '旧版已完成证据', 1, '2026-05-07 08:00:00', '2026-05-07 08:30:00',
                    '2026-05-07 08:30:00')",
            [],
        )
        .expect("action items");

        clear_expired_daily_cache(&conn, "2026-05-09").expect("clear expired cache");

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
