use rusqlite::params;

use crate::storage::models::ActionItem;

use super::sources::{is_aggregate, platform_label, source_label};

pub fn list_actions(
    conn: &rusqlite::Connection,
    item_type: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<ActionItem>> {
    let sql = if is_aggregate(profile_id) {
        "select action_items.id, type, action_items.status, priority, title, description, suggested_reply,
                profile_id, action_items.platform, chat_id, chat_name, evidence_summary,
                context_incomplete, carry_over, last_updated_at, completed_at,
                coalesce(json_extract(profiles.config_json, '$.remark'), ''),
                coalesce((
                    select datetime(max(daily_messages.timestamp), 'unixepoch', 'localtime')
                    from daily_messages
                    where daily_messages.id in (
                        select value from json_each(action_items.source_message_ids)
                    )
                ), datetime(first_detected_at), first_detected_at, last_updated_at) as source_message_at
         from action_items
         left join profiles on profiles.id = action_items.profile_id
         where type = ?1
         order by case action_items.status when 'open' then 0 when 'done' then 1 else 2 end,
                  case when action_items.status = 'open' then source_message_at end asc,
                  case when action_items.status = 'done' then coalesce(completed_at, last_updated_at) end desc,
                  case when action_items.status not in ('open', 'done') then source_message_at end desc"
    } else {
        "select action_items.id, type, action_items.status, priority, title, description, suggested_reply,
                profile_id, action_items.platform, chat_id, chat_name, evidence_summary,
                context_incomplete, carry_over, last_updated_at, completed_at,
                coalesce(json_extract(profiles.config_json, '$.remark'), ''),
                coalesce((
                    select datetime(max(daily_messages.timestamp), 'unixepoch', 'localtime')
                    from daily_messages
                    where daily_messages.id in (
                        select value from json_each(action_items.source_message_ids)
                    )
                ), datetime(first_detected_at), first_detected_at, last_updated_at) as source_message_at
         from action_items
         left join profiles on profiles.id = action_items.profile_id
         where type = ?1 and profile_id = ?2
         order by case action_items.status when 'open' then 0 when 'done' then 1 else 2 end,
                  case when action_items.status = 'open' then source_message_at end asc,
                  case when action_items.status = 'done' then coalesce(completed_at, last_updated_at) end desc,
                  case when action_items.status not in ('open', 'done') then source_message_at end desc"
    };

    let mut stmt = conn.prepare(sql)?;
    if is_aggregate(profile_id) {
        let rows = stmt.query_map(params![item_type], map_action)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    } else {
        let rows = stmt.query_map(params![item_type, profile_id], map_action)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn map_action(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActionItem> {
    let platform: String = row.get(8)?;
    let platform_remark: String = row.get(16)?;
    let platform_label = platform_label(&platform).to_owned();
    let source_label = source_label(&platform, &platform_remark);
    Ok(ActionItem {
        id: row.get(0)?,
        item_type: row.get(1)?,
        status: row.get(2)?,
        priority: row.get(3)?,
        title: row.get(4)?,
        description: row.get(5)?,
        suggested_reply: row.get(6)?,
        profile_id: row.get(7)?,
        platform,
        platform_label,
        platform_remark,
        source_label,
        chat_id: row.get(9)?,
        chat_name: row.get(10)?,
        evidence_summary: row.get(11)?,
        context_incomplete: row.get::<_, i64>(12)? == 1,
        carry_over: row.get::<_, i64>(13)? == 1,
        source_message_at: row.get(17)?,
        last_updated_at: row.get(14)?,
        completed_at: row.get(15)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marking_one_historical_task_done_keeps_other_carry_over_tasks_visible() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into profiles(id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at)
             values('profile-1', 'wechat', '微信', 1, '{\"remark\":\"工作号\"}', 'normal', 0, datetime('now'), datetime('now'))",
            [],
        )
        .expect("profile");
        conn.execute(
            "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id,
               chat_name, source_message_ids, evidence_summary, carry_over, first_detected_at, last_updated_at
             )
             values('act-1', 'task', 'open', 'medium', '历史待办一', '历史描述',
                    'profile-1', 'wechat', 'chat-1', '旧聊天一', '[\"msg-old-1\"]',
                    '历史证据', 1, '2026-05-05 09:00:00', '2026-05-06 09:30:00'),
                   ('act-2', 'task', 'open', 'medium', '历史待办二', '历史描述',
                    'profile-1', 'wechat', 'chat-2', '旧聊天二', '[\"msg-old-2\"]',
                    '历史证据', 1, '2026-05-05 10:00:00', '2026-05-05 10:00:00'),
                   ('act-3', 'task', 'open', 'medium', '历史待办三', '历史描述',
                    'profile-1', 'wechat', 'chat-3', '旧聊天三', '[\"msg-old-3\"]',
                    '历史证据', 1, '2026-05-05 11:00:00', '2026-05-05 11:00:00')",
            [],
        )
        .expect("action items");

        conn.execute(
            "update action_items
             set status = 'done', completed_at = '2026-05-06 12:00:00', last_updated_at = '2026-05-06 12:00:00'
             where id = 'act-2'",
            [],
        )
        .expect("mark done");

        let tasks = list_actions(&conn, "task", "aggregate").expect("tasks");
        let visible = tasks
            .iter()
            .map(|item| (item.id.as_str(), item.status.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            visible,
            vec![("act-1", "open"), ("act-3", "open"), ("act-2", "done")]
        );
        assert_eq!(tasks[0].source_message_at, "2026-05-05 09:00:00");
    }
}
