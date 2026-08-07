use chrono::Local;
use rusqlite::{params, Connection};
use std::collections::BTreeSet;

use crate::storage::models::ImProfile;

pub fn list_profiles(conn: &mut Connection) -> anyhow::Result<Vec<ImProfile>> {
    let mut stmt = conn.prepare(
        "select id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at
         from profiles where platform in ('wecom', 'feishu', 'dingtalk')
         order by sort_order, platform, label",
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
    Ok(ImProfile {
        updated_at: now,
        ..profile
    })
}

pub fn reorder_profiles(conn: &mut Connection, profile_ids: &[String]) -> anyhow::Result<()> {
    let existing_ids = {
        let mut stmt = conn.prepare("select id from profiles")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    let requested = profile_ids.iter().cloned().collect::<BTreeSet<_>>();
    let existing = existing_ids.iter().cloned().collect::<BTreeSet<_>>();

    if requested.len() != profile_ids.len() {
        anyhow::bail!("账号排序列表包含重复账号，请刷新后重试。");
    }
    if requested != existing {
        anyhow::bail!("账号排序列表与当前账号不一致，请刷新后重试。");
    }

    let now = Local::now().to_rfc3339();
    let tx = conn.transaction()?;
    for (sort_order, profile_id) in profile_ids.iter().enumerate() {
        let updated = tx.execute(
            "update profiles set sort_order = ?1, updated_at = ?2 where id = ?3",
            params![sort_order as i64, now, profile_id],
        )?;
        if updated != 1 {
            anyhow::bail!("账号排序提交失败，请刷新后重试。");
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn delete_profile(
    conn: &mut Connection,
    profile_id: &str,
) -> anyhow::Result<Option<ImProfile>> {
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
             values('dingtalk-a', 'dingtalk', '钉钉', '{}', datetime('now'), datetime('now'))",
            [],
        )
        .expect("insert profile");
        conn.execute(
            "insert into sync_state(profile_id, day, updated_at)
             values('dingtalk-a', '2026-05-07', datetime('now'))",
            [],
        )
        .expect("insert sync");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             ) values(
               'msg-a', '2026-05-07', 'dingtalk-a', 'dingtalk', 'chat-a', '测试群', 'sender-a', '张三',
               1778112000, '09:00', 'text', 'hello', 'hash-a'
             )",
            [],
        )
        .expect("insert message");
        conn.execute(
            "insert into daily_stats(id, day, profile_id, metric, value_json, updated_at)
             values('stat-a', '2026-05-07', 'dingtalk-a', 'keywords', '[]', datetime('now'))",
            [],
        )
        .expect("insert stat");
        conn.execute(
            "insert into daily_topics(id, day, profile_id, title, summary, updated_at)
             values('topic-a', '2026-05-07', 'dingtalk-a', '主题', '摘要', datetime('now'))",
            [],
        )
        .expect("insert topic");
        conn.execute(
            "insert into ai_analysis_runs(id, day, profile_id, status, created_at)
             values('run-a', '2026-05-07', 'dingtalk-a', 'done', datetime('now'))",
            [],
        )
        .expect("insert run");
        conn.execute(
            "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id, chat_name,
               first_detected_at, last_updated_at
             ) values(
               'act-a', 'reply', 'open', 'medium', '回复', '描述', 'dingtalk-a', 'dingtalk', 'chat-a', '测试群',
               datetime('now'), datetime('now')
             )",
            [],
        )
        .expect("insert action");

        let deleted = delete_profile(&mut conn, "dingtalk-a").expect("delete profile");
        assert_eq!(deleted.expect("deleted profile").id, "dingtalk-a");
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
                .query_row(&format!("select count(*) from {table}"), [], |row| {
                    row.get(0)
                })
                .expect("count rows");
            assert_eq!(count, 0, "{table} should be empty");
        }
    }

    #[test]
    fn reorder_profiles_commits_the_complete_order_atomically() {
        let mut conn = Connection::open_in_memory().expect("open memory db");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("create schema");
        for (id, order) in [("dingtalk-a", 0), ("feishu-a", 1)] {
            conn.execute(
                "insert into profiles(id, platform, label, config_json, sort_order, created_at, updated_at)
                 values(?1, 'dingtalk', ?1, '{}', ?2, datetime('now'), datetime('now'))",
                params![id, order],
            )
            .expect("insert profile");
        }

        reorder_profiles(&mut conn, &["feishu-a".to_owned(), "dingtalk-a".to_owned()])
            .expect("reorder profiles");

        let ordered = list_profiles(&mut conn).expect("list profiles");
        assert_eq!(ordered[0].id, "feishu-a");
        assert_eq!(ordered[1].id, "dingtalk-a");
    }

    #[test]
    fn reorder_profiles_rejects_incomplete_lists_without_changes() {
        let mut conn = Connection::open_in_memory().expect("open memory db");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("create schema");
        for (id, order) in [("dingtalk-a", 0), ("feishu-a", 1)] {
            conn.execute(
                "insert into profiles(id, platform, label, config_json, sort_order, created_at, updated_at)
                 values(?1, 'dingtalk', ?1, '{}', ?2, datetime('now'), datetime('now'))",
                params![id, order],
            )
            .expect("insert profile");
        }

        let error = reorder_profiles(&mut conn, &["feishu-a".to_owned()])
            .expect_err("incomplete reorder should fail");
        assert!(error.to_string().contains("不一致"));
        let ordered = list_profiles(&mut conn).expect("list profiles");
        assert_eq!(ordered[0].id, "dingtalk-a");
        assert_eq!(ordered[1].id, "feishu-a");
    }
}
