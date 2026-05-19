use rusqlite::params;

use crate::storage::models::{DashboardMetric, SourceStat};

use super::sources::{is_aggregate, source_stat};

pub fn load_metrics(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    open_replies: i64,
    open_tasks: i64,
) -> anyhow::Result<Vec<DashboardMetric>> {
    let messages = count_daily(conn, day, profile_id)?;
    let chats = count_chats(conn, day, profile_id)?;
    Ok(vec![
        DashboardMetric {
            key: "messages".to_owned(),
            label: "今天消息数".to_owned(),
            value: messages,
            sources: daily_source_counts(conn, day, profile_id).unwrap_or_default(),
        },
        DashboardMetric {
            key: "replies".to_owned(),
            label: "待我回复".to_owned(),
            value: open_replies,
            sources: action_source_counts(conn, "reply", profile_id).unwrap_or_default(),
        },
        DashboardMetric {
            key: "tasks".to_owned(),
            label: "待办事项".to_owned(),
            value: open_tasks,
            sources: action_source_counts(conn, "task", profile_id).unwrap_or_default(),
        },
        DashboardMetric {
            key: "chats".to_owned(),
            label: "单聊/群聊数".to_owned(),
            value: chats,
            sources: chat_source_counts(conn, day, profile_id).unwrap_or_default(),
        },
    ])
}

fn count_daily(conn: &rusqlite::Connection, day: &str, profile_id: &str) -> anyhow::Result<i64> {
    if is_aggregate(profile_id) {
        Ok(conn.query_row(
            "select count(*) from daily_messages where day = ?1",
            params![day],
            |row| row.get(0),
        )?)
    } else {
        Ok(conn.query_row(
            "select count(*) from daily_messages where day = ?1 and profile_id = ?2",
            params![day, profile_id],
            |row| row.get(0),
        )?)
    }
}

fn count_chats(conn: &rusqlite::Connection, day: &str, profile_id: &str) -> anyhow::Result<i64> {
    if is_aggregate(profile_id) {
        Ok(conn.query_row(
            "select count(distinct chat_id) from daily_messages where day = ?1",
            params![day],
            |row| row.get(0),
        )?)
    } else {
        Ok(conn.query_row(
            "select count(distinct chat_id) from daily_messages where day = ?1 and profile_id = ?2",
            params![day, profile_id],
            |row| row.get(0),
        )?)
    }
}

fn daily_source_counts(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SourceStat>> {
    source_counts(
        conn,
        &format!(
            "select daily_messages.profile_id, daily_messages.platform,
                    coalesce(json_extract(profiles.config_json, '$.remark'), ''), count(*)
             from daily_messages
             left join profiles on profiles.id = daily_messages.profile_id
             where daily_messages.day = ?1{}
             group by daily_messages.profile_id, daily_messages.platform
             order by count(*) desc",
            if is_aggregate(profile_id) {
                ""
            } else {
                " and daily_messages.profile_id = ?2"
            }
        ),
        day,
        profile_id,
    )
}

fn action_source_counts(
    conn: &rusqlite::Connection,
    item_type: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SourceStat>> {
    let sql = format!(
        "select action_items.profile_id, action_items.platform,
                coalesce(json_extract(profiles.config_json, '$.remark'), ''), count(*)
         from action_items
         left join profiles on profiles.id = action_items.profile_id
         where action_items.status = 'open' and action_items.type = ?1{}
         group by action_items.profile_id, action_items.platform
         order by count(*) desc",
        if is_aggregate(profile_id) {
            ""
        } else {
            " and action_items.profile_id = ?2"
        }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = if is_aggregate(profile_id) {
        stmt.query_map(params![item_type], map_source_count)?
    } else {
        stmt.query_map(params![item_type, profile_id], map_source_count)?
    };
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn chat_source_counts(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SourceStat>> {
    source_counts(
        conn,
        &format!(
            "select daily_messages.profile_id, daily_messages.platform,
                    coalesce(json_extract(profiles.config_json, '$.remark'), ''),
                    count(distinct daily_messages.chat_id)
             from daily_messages
             left join profiles on profiles.id = daily_messages.profile_id
             where daily_messages.day = ?1{}
             group by daily_messages.profile_id, daily_messages.platform
             order by count(distinct daily_messages.chat_id) desc",
            if is_aggregate(profile_id) {
                ""
            } else {
                " and daily_messages.profile_id = ?2"
            }
        ),
        day,
        profile_id,
    )
}

fn source_counts(
    conn: &rusqlite::Connection,
    sql: &str,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SourceStat>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = if is_aggregate(profile_id) {
        stmt.query_map(params![day], map_source_count)?
    } else {
        stmt.query_map(params![day, profile_id], map_source_count)?
    };
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn map_source_count(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceStat> {
    let profile_id: String = row.get(0)?;
    let platform: String = row.get(1)?;
    let remark: String = row.get(2)?;
    let count: i64 = row.get(3)?;
    Ok(source_stat(profile_id, platform, remark, count, Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into profiles(id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at)
             values('wechat-work', 'wechat', '微信', 1, '{\"remark\":\"工作号\"}', 'normal', 0, datetime('now'), datetime('now')),
                   ('feishu-main', 'feishu', '飞书', 1, '{\"remark\":\"研发\"}', 'normal', 1, datetime('now'), datetime('now'))",
            [],
        )
        .expect("profiles");
        conn
    }

    fn insert_message(
        conn: &rusqlite::Connection,
        id: &str,
        profile_id: &str,
        platform: &str,
        chat_id: &str,
        chat_name: &str,
        timestamp: i64,
    ) {
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-06', ?2, ?3, ?4, ?5, 0, 'u-1', '用户', ?6, '10:00', 'text', ?7, ?8)",
            rusqlite::params![
                id,
                profile_id,
                platform,
                chat_id,
                chat_name,
                timestamp,
                format!("消息 {id}"),
                format!("hash-{id}")
            ],
        )
        .expect("message");
    }

    fn insert_action(
        conn: &rusqlite::Connection,
        id: &str,
        item_type: &str,
        status: &str,
        profile_id: &str,
        platform: &str,
    ) {
        conn.execute(
            "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id,
               chat_name, source_message_ids, evidence_summary, first_detected_at, last_updated_at
             )
             values(?1, ?2, ?3, 'medium', ?4, '描述', ?5, ?6, 'chat-action',
                    '行动群', '[]', '证据', datetime('now'), datetime('now'))",
            rusqlite::params![id, item_type, status, id, profile_id, platform],
        )
        .expect("action item");
    }

    fn metric_value(metrics: &[DashboardMetric], key: &str) -> i64 {
        metrics
            .iter()
            .find(|metric| metric.key == key)
            .map(|metric| metric.value)
            .expect("metric")
    }

    #[test]
    fn aggregate_metrics_keep_message_chat_and_open_action_sources() {
        let conn = setup_conn();
        insert_message(
            &conn,
            "msg-1",
            "wechat-work",
            "wechat",
            "chat-a",
            "客户 A",
            1,
        );
        insert_message(
            &conn,
            "msg-2",
            "wechat-work",
            "wechat",
            "chat-b",
            "客户 B",
            2,
        );
        insert_message(
            &conn,
            "msg-3",
            "feishu-main",
            "feishu",
            "chat-c",
            "项目群",
            3,
        );
        insert_action(
            &conn,
            "reply-open",
            "reply",
            "open",
            "wechat-work",
            "wechat",
        );
        insert_action(
            &conn,
            "reply-done",
            "reply",
            "done",
            "wechat-work",
            "wechat",
        );
        insert_action(&conn, "task-open", "task", "open", "feishu-main", "feishu");

        let metrics = load_metrics(&conn, "2026-05-06", "aggregate", 1, 1).expect("metrics");

        assert_eq!(metric_value(&metrics, "messages"), 3);
        assert_eq!(metric_value(&metrics, "chats"), 3);
        assert_eq!(metric_value(&metrics, "replies"), 1);
        assert_eq!(metric_value(&metrics, "tasks"), 1);

        let message_sources = &metrics
            .iter()
            .find(|metric| metric.key == "messages")
            .expect("messages metric")
            .sources;
        assert_eq!(message_sources[0].label, "微信（工作号）");
        assert_eq!(message_sources[0].count, 2);
        assert_eq!(message_sources[1].label, "飞书（研发）");
        assert_eq!(message_sources[1].count, 1);

        let reply_sources = &metrics
            .iter()
            .find(|metric| metric.key == "replies")
            .expect("replies metric")
            .sources;
        assert_eq!(reply_sources.len(), 1);
        assert_eq!(reply_sources[0].label, "微信（工作号）");
        assert_eq!(reply_sources[0].count, 1);
    }

    #[test]
    fn profile_metrics_only_include_selected_profile_sources() {
        let conn = setup_conn();
        insert_message(
            &conn,
            "msg-1",
            "wechat-work",
            "wechat",
            "chat-a",
            "客户 A",
            1,
        );
        insert_message(
            &conn,
            "msg-2",
            "wechat-work",
            "wechat",
            "chat-b",
            "客户 B",
            2,
        );
        insert_message(
            &conn,
            "msg-3",
            "feishu-main",
            "feishu",
            "chat-c",
            "项目群",
            3,
        );
        insert_action(
            &conn,
            "reply-open",
            "reply",
            "open",
            "wechat-work",
            "wechat",
        );
        insert_action(&conn, "task-open", "task", "open", "feishu-main", "feishu");

        let metrics = load_metrics(&conn, "2026-05-06", "wechat-work", 1, 0).expect("metrics");

        assert_eq!(metric_value(&metrics, "messages"), 2);
        assert_eq!(metric_value(&metrics, "chats"), 2);
        assert_eq!(metric_value(&metrics, "replies"), 1);
        assert_eq!(metric_value(&metrics, "tasks"), 0);

        for metric in metrics {
            for source in metric.sources {
                assert_eq!(source.profile_id, "wechat-work");
            }
        }
    }
}
