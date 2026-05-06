use std::collections::HashMap;

use rusqlite::params;
use tauri::State;

use crate::daily_cache;
use crate::messages::normalizer::DailyMessage;
use crate::storage::models::ImProfile;
use crate::storage::AppState;

pub(crate) fn reset_ai_generated_cache(
    state: &State<'_, AppState>,
    day: &daily_cache::DashboardDay,
    target_profiles: &[ImProfile],
) -> anyhow::Result<()> {
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    reset_ai_generated_cache_conn(&conn, day, target_profiles)
}

pub(crate) fn reset_ai_generated_cache_conn(
    conn: &rusqlite::Connection,
    day: &daily_cache::DashboardDay,
    target_profiles: &[ImProfile],
) -> anyhow::Result<()> {
    let tx = conn.unchecked_transaction()?;

    tx.execute(
        "delete from daily_stats where day = ?1 and profile_id = 'aggregate' and metric in ('topics', 'keywords')",
        params![day.day],
    )?;

    for profile in target_profiles {
        tx.execute(
            "update daily_messages set analyzed_at = null, topic_summarized_at = null where day = ?1 and profile_id = ?2",
            params![day.day, profile.id],
        )?;
        delete_regenerable_action_items_for_profile(&tx, profile.id.as_str(), day)?;
        tx.execute(
            "delete from daily_stats where day = ?1 and profile_id = ?2 and metric in ('topics', 'keywords')",
            params![day.day, profile.id],
        )?;
        tx.execute(
            "delete from daily_topics where day = ?1 and profile_id = ?2",
            params![day.day, profile.id],
        )?;
        tx.execute(
            "delete from ai_analysis_runs where day = ?1 and profile_id = ?2",
            params![day.day, profile.id],
        )?;
        tx.execute(
            "update sync_state set last_analysis_at = null, updated_at = datetime('now') where day = ?1 and profile_id = ?2",
            params![day.day, profile.id],
        )?;
    }

    tx.commit()?;
    Ok(())
}

fn delete_regenerable_action_items_for_profile(
    conn: &rusqlite::Connection,
    profile_id: &str,
    day: &daily_cache::DashboardDay,
) -> anyhow::Result<()> {
    // 历史未完成事项按来源消息发生时间判断；旧数据找不到来源消息时才退回 first_detected_at。
    conn.execute(
        "delete from action_items
         where profile_id = ?1
           and not (
             status = 'open'
             and type in ('reply', 'task')
             and carry_over = 1
             and coalesce((
               select min(daily_messages.timestamp)
               from daily_messages
               where daily_messages.id in (
                 select value from json_each(action_items.source_message_ids)
               )
             ), cast(strftime('%s', first_detected_at) as integer), 0) < ?2
           )",
        params![profile_id, day.day_start_timestamp],
    )?;
    Ok(())
}

pub(crate) fn delete_regenerable_action_items(
    conn: &rusqlite::Connection,
    day: &daily_cache::DashboardDay,
) -> anyhow::Result<()> {
    // 全量重新同步会重建今日消息与分析；历史口径仍以来源消息发生时间为准。
    conn.execute(
        "delete from action_items
         where not (
           status = 'open'
           and type in ('reply', 'task')
           and carry_over = 1
           and coalesce((
             select min(daily_messages.timestamp)
             from daily_messages
             where daily_messages.id in (
               select value from json_each(action_items.source_message_ids)
             )
           ), cast(strftime('%s', first_detected_at) as integer), 0) < ?1
         )",
        params![day.day_start_timestamp],
    )?;
    Ok(())
}

pub(crate) fn clear_dashboard_cache(
    state: &State<'_, AppState>,
    day: &daily_cache::DashboardDay,
) -> anyhow::Result<()> {
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    clear_dashboard_cache_conn(&conn, day)
}

pub(crate) fn clear_dashboard_cache_conn(
    conn: &rusqlite::Connection,
    day: &daily_cache::DashboardDay,
) -> anyhow::Result<()> {
    let tx = conn.unchecked_transaction()?;

    // 先用来源消息时间判断哪些待办可重建，再清空消息表；否则昨天聊天、今天入看板的未完成项会误按入看板时间删除。
    delete_regenerable_action_items(&tx, day)?;
    tx.execute("delete from daily_messages", [])?;
    tx.execute("delete from daily_stats", [])?;
    tx.execute("delete from daily_topics", [])?;
    tx.execute("delete from ai_analysis_runs", [])?;
    tx.execute("delete from sync_state", [])?;

    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn clear_dashboard_cache_preserves_open_item_by_source_message_time() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values('msg-yesterday', '2026-05-05', 'profile-1', 'wechat', 'chat-1', '客户群', 1,
                    'u-1', '客户', cast(strftime('%s', '2026-05-05 18:30:00') as integer),
                    '18:30', 'text', '昨天提出但今天才被分析进看板的待办', 'hash-yesterday'),
                   ('msg-today', '2026-05-06', 'profile-1', 'wechat', 'chat-1', '客户群', 1,
                    'u-1', '客户', cast(strftime('%s', '2026-05-06 09:30:00') as integer),
                    '09:30', 'text', '今天新产生的待办', 'hash-today')",
            [],
        )
        .expect("messages");
        conn.execute(
            "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id,
               chat_name, source_message_ids, evidence_summary, carry_over, first_detected_at, last_updated_at
             )
             values('act-yesterday', 'task', 'open', 'medium', '昨天聊天待办', '昨天聊天产生，今天才进入看板',
                    'profile-1', 'wechat', 'chat-1', '客户群', '[\"msg-yesterday\"]',
                    '昨天证据', 1, '2026-05-06 10:00:00', '2026-05-06 10:00:00'),
                   ('act-today', 'task', 'open', 'medium', '今天聊天待办', '今天聊天产生',
                    'profile-1', 'wechat', 'chat-1', '客户群', '[\"msg-today\"]',
                    '今天证据', 1, '2026-05-06 10:00:00', '2026-05-06 10:00:00')",
            [],
        )
        .expect("action items");

        let day = test_dashboard_day("2026-05-06");
        clear_dashboard_cache_conn(&conn, &day).expect("clear cache");

        let ids = action_item_ids(&conn);
        assert_eq!(ids, vec!["act-yesterday".to_owned()]);
        assert_eq!(count_rows(&conn, "daily_messages"), 0);
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

    fn count_rows(conn: &rusqlite::Connection, table: &str) -> i64 {
        conn.query_row(&format!("select count(*) from {table}"), [], |row| {
            row.get(0)
        })
        .expect("count")
    }
}

fn insert_daily_message(
    conn: &rusqlite::Connection,
    message: &DailyMessage,
) -> anyhow::Result<i64> {
    Ok(conn.execute(
        "insert into daily_messages(
           id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
           timestamp, time_text, msg_type, content, raw_type, local_id, raw_json, content_hash, partial
         )
         values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
         on conflict(id) do update set
           chat_name = excluded.chat_name,
           sender_name = excluded.sender_name,
           time_text = excluded.time_text,
           msg_type = excluded.msg_type,
           raw_type = excluded.raw_type,
           raw_json = excluded.raw_json,
           partial = excluded.partial",
        params![
            message.id,
            message.day,
            message.profile_id,
            message.platform,
            message.chat_id,
            message.chat_name,
            i64::from(message.is_group),
            message.sender_id,
            message.sender_name,
            message.timestamp,
            message.time_text,
            message.msg_type,
            message.content,
            message.raw_type,
            message.local_id,
            message.raw_json,
            message.content_hash,
            i64::from(message.partial),
        ],
    )? as i64)
}

pub(crate) fn insert_messages(
    state: &State<'_, AppState>,
    messages: &[DailyMessage],
) -> anyhow::Result<i64> {
    if messages.is_empty() {
        return Ok(0);
    }
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let tx = conn.unchecked_transaction()?;
    let mut inserted = 0;
    for message in messages {
        inserted += insert_daily_message(&tx, message)?;
    }
    tx.commit()?;
    Ok(inserted)
}

pub(crate) fn latest_saved_message_timestamps(
    state: &State<'_, AppState>,
    profile: &ImProfile,
    day: &str,
) -> anyhow::Result<HashMap<String, i64>> {
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let mut stmt = conn.prepare(
        "select chat_id, max(timestamp)
         from daily_messages
         where day = ?1 and profile_id = ?2 and platform = ?3
         group by chat_id",
    )?;
    let rows = stmt.query_map(params![day, profile.id, profile.platform], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut timestamps = HashMap::new();
    for row in rows {
        let (chat_id, timestamp) = row?;
        timestamps.insert(chat_id, timestamp);
    }
    Ok(timestamps)
}

pub(crate) fn resolve_target_profiles(
    conn: &rusqlite::Connection,
    profile_id: &str,
) -> anyhow::Result<Vec<ImProfile>> {
    let mut profiles = Vec::new();
    if profile_id == "aggregate" {
        let mut stmt = conn.prepare(
            "select id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at
             from profiles where enabled = 1 order by sort_order",
        )?;
        let rows = stmt.query_map([], map_profile)?;
        for row in rows {
            profiles.push(row?);
        }
    } else {
        profiles.push(conn.query_row(
            "select id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at
             from profiles where id = ?1",
            params![profile_id],
            map_profile,
        )?);
    }

    Ok(profiles)
}

pub(crate) fn resolve_all_profiles(conn: &rusqlite::Connection) -> anyhow::Result<Vec<ImProfile>> {
    let mut profiles = Vec::new();
    let mut stmt = conn.prepare(
        "select id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at
         from profiles order by sort_order",
    )?;
    let rows = stmt.query_map([], map_profile)?;
    for row in rows {
        profiles.push(row?);
    }
    Ok(profiles)
}

fn map_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<ImProfile> {
    let config_json: String = row.get(4)?;
    Ok(ImProfile {
        id: row.get(0)?,
        platform: row.get(1)?,
        label: row.get(2)?,
        enabled: row.get::<_, i64>(3)? == 1,
        config_json: serde_json::from_str(&config_json).unwrap_or_else(|_| serde_json::json!({})),
        status: row.get(5)?,
        sort_order: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}
