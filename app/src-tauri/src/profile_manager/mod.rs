use chrono::Local;
use rusqlite::{params, Connection};

use crate::storage::models::ImProfile;

pub fn list_profiles(conn: &Connection) -> anyhow::Result<Vec<ImProfile>> {
    let mut stmt = conn.prepare(
        "select id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at
         from profiles order by sort_order, platform, label",
    )?;
    let rows = stmt.query_map([], |row| {
        let config: String = row.get(4)?;
        Ok(ImProfile {
            id: row.get(0)?,
            platform: row.get(1)?,
            label: row.get(2)?,
            enabled: row.get::<_, i64>(3)? == 1,
            config_json: serde_json::from_str(&config).unwrap_or_else(|_| serde_json::json!({})),
            status: row.get(5)?,
            sort_order: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn upsert_profile(conn: &Connection, profile: ImProfile) -> anyhow::Result<ImProfile> {
    let now = Local::now().to_rfc3339();
    let created_at = if profile.created_at.is_empty() {
        now.clone()
    } else {
        profile.created_at.clone()
    };
    conn.execute(
        "insert into profiles(id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         on conflict(id) do update set
           platform = excluded.platform,
           label = excluded.label,
           enabled = excluded.enabled,
           config_json = excluded.config_json,
           status = excluded.status,
           sort_order = excluded.sort_order,
           updated_at = excluded.updated_at",
        params![
            profile.id,
            profile.platform,
            profile.label,
            if profile.enabled { 1 } else { 0 },
            serde_json::to_string(&profile.config_json)?,
            profile.status,
            profile.sort_order,
            created_at,
            now
        ],
    )?;
    Ok(profile)
}

pub fn delete_profile(conn: &mut Connection, profile_id: &str) -> anyhow::Result<Option<ImProfile>> {
    let profile = profile_by_id(conn, profile_id)?;
    let tx = conn.transaction()?;
    tx.execute("delete from profiles where id = ?1", params![profile_id])?;
    for table in [
        "sync_state",
        "daily_messages",
        "daily_stats",
        "daily_topics",
        "ai_analysis_runs",
        "action_items",
    ] {
        tx.execute(
            &format!("delete from {table} where profile_id = ?1"),
            params![profile_id],
        )?;
    }
    tx.execute(
        "delete from daily_stats where profile_id = 'aggregate' and metric in ('topics', 'keywords')",
        [],
    )?;
    tx.commit()?;
    Ok(profile)
}

fn profile_by_id(conn: &Connection, profile_id: &str) -> anyhow::Result<Option<ImProfile>> {
    let mut stmt = conn.prepare(
        "select id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at
         from profiles where id = ?1",
    )?;
    let mut rows = stmt.query_map(params![profile_id], |row| {
        let config: String = row.get(4)?;
        Ok(ImProfile {
            id: row.get(0)?,
            platform: row.get(1)?,
            label: row.get(2)?,
            enabled: row.get::<_, i64>(3)? == 1,
            config_json: serde_json::from_str(&config).unwrap_or_else(|_| serde_json::json!({})),
            status: row.get(5)?,
            sort_order: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })?;
    rows.next().transpose().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_profile_removes_profile_owned_runtime_rows() {
        let mut conn = Connection::open_in_memory().expect("open memory db");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("create schema");
        conn.execute(
            "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('wechat-a', 'wechat', '微信', '{}', datetime('now'), datetime('now'))",
            [],
        )
        .expect("insert profile");
        conn.execute(
            "insert into sync_state(profile_id, day, updated_at)
             values('wechat-a', '2026-05-07', datetime('now'))",
            [],
        )
        .expect("insert sync");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             ) values(
               'msg-a', '2026-05-07', 'wechat-a', 'wechat', 'chat-a', '测试群', 'sender-a', '张三',
               1778112000, '09:00', 'text', 'hello', 'hash-a'
             )",
            [],
        )
        .expect("insert message");
        conn.execute(
            "insert into daily_stats(id, day, profile_id, metric, value_json, updated_at)
             values('stat-a', '2026-05-07', 'wechat-a', 'keywords', '[]', datetime('now'))",
            [],
        )
        .expect("insert stat");
        conn.execute(
            "insert into daily_topics(id, day, profile_id, title, summary, updated_at)
             values('topic-a', '2026-05-07', 'wechat-a', '主题', '摘要', datetime('now'))",
            [],
        )
        .expect("insert topic");
        conn.execute(
            "insert into ai_analysis_runs(id, day, profile_id, status, created_at)
             values('run-a', '2026-05-07', 'wechat-a', 'done', datetime('now'))",
            [],
        )
        .expect("insert run");
        conn.execute(
            "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id, chat_name,
               first_detected_at, last_updated_at
             ) values(
               'act-a', 'reply', 'open', 'medium', '回复', '描述', 'wechat-a', 'wechat', 'chat-a', '测试群',
               datetime('now'), datetime('now')
             )",
            [],
        )
        .expect("insert action");

        let deleted = delete_profile(&mut conn, "wechat-a").expect("delete profile");
        assert_eq!(deleted.expect("deleted profile").id, "wechat-a");
        for table in [
            "profiles",
            "sync_state",
            "daily_messages",
            "daily_stats",
            "daily_topics",
            "ai_analysis_runs",
            "action_items",
        ] {
            let count: i64 = conn
                .query_row(&format!("select count(*) from {table}"), [], |row| row.get(0))
                .expect("count rows");
            assert_eq!(count, 0, "{table} should be empty");
        }
    }
}
