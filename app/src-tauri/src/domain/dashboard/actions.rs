use rusqlite::params;

use crate::storage::models::ActionItem;

use super::sources::{is_aggregate, platform_label, source_label};

pub fn list_actions(
    conn: &rusqlite::Connection,
    item_type: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<ActionItem>> {
    // Source-backed timestamps are local wall time. The stored fallback also accepts
    // RFC3339, so only explicit zones may be converted; naive values are already local.
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
                ), case
                    when upper(substr(trim(first_detected_at), -1)) = 'Z'
                         or trim(first_detected_at) glob '*[+-][0-9][0-9]:[0-9][0-9]'
                    then datetime(first_detected_at, 'localtime')
                    else datetime(first_detected_at)
                  end, first_detected_at, last_updated_at) as source_message_at
         from action_items
         left join profiles on profiles.id = action_items.profile_id
         where type = ?1
         order by case action_items.status when 'open' then 0 when 'done' then 1 else 2 end,
                  case when action_items.status = 'open' then source_message_at end asc,
                  case when action_items.status = 'done' then julianday(coalesce(completed_at, last_updated_at)) end desc,
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
                ), case
                    when upper(substr(trim(first_detected_at), -1)) = 'Z'
                         or trim(first_detected_at) glob '*[+-][0-9][0-9]:[0-9][0-9]'
                    then datetime(first_detected_at, 'localtime')
                    else datetime(first_detected_at)
                  end, first_detected_at, last_updated_at) as source_message_at
         from action_items
         left join profiles on profiles.id = action_items.profile_id
         where type = ?1 and profile_id = ?2
         order by case action_items.status when 'open' then 0 when 'done' then 1 else 2 end,
                  case when action_items.status = 'open' then source_message_at end asc,
                  case when action_items.status = 'done' then julianday(coalesce(completed_at, last_updated_at)) end desc,
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

    fn timezone_fixture(first_detected_at: &str, with_source: bool) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into profiles(id, platform, label, created_at, updated_at)
             values('timezone-profile', 'wecom', '企业微信', '2026-09-30', '2026-09-30')",
            [],
        )
        .expect("profile");
        if with_source {
            conn.execute(
                "insert into daily_messages(
                   id, day, profile_id, platform, chat_id, chat_name, sender_id, sender_name,
                   timestamp, time_text, msg_type, content, content_hash
                 ) values('timezone-message', '2026-09-30', 'timezone-profile', 'wecom',
                          'timezone-chat', '时区测试', 'sender', '发送者', 1790699400,
                          '00:30', 'text', '临时消息', 'timezone-hash')",
                [],
            )
            .expect("source message");
        }
        conn.execute(
            "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform,
               chat_id, chat_name, source_message_ids, first_detected_at, last_updated_at
             ) values('timezone-action', 'task', 'open', 'medium', '时区待办', '临时数据',
                      'timezone-profile', 'wecom', 'timezone-chat', '时区测试',
                      '[\"timezone-message\"]', ?1, '2026-09-30 00:30:00')",
            params![first_detected_at],
        )
        .expect("action");
        conn
    }

    fn action_source_times(conn: &rusqlite::Connection) -> Vec<String> {
        ["aggregate", "timezone-profile"]
            .into_iter()
            .map(|profile| {
                let items = list_actions(conn, "task", profile).expect("actual repository read");
                assert_eq!(items.len(), 1);
                items[0].source_message_at.clone()
            })
            .collect()
    }

    #[test]
    fn missing_source_offset_matches_source_message_local_time() {
        // The same instant crosses a calendar date in positive offsets. Expected
        // values come from chrono rather than repeating the SQL implementation.
        let expected = chrono::DateTime::parse_from_rfc3339("2026-09-30T00:30:00+08:00")
            .expect("RFC3339")
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let sourced = timezone_fixture("unrelated fallback", true);
        assert_eq!(action_source_times(&sourced), vec![expected.clone(); 2]);
        for value in ["2026-09-30T00:30:00+08:00", "2026-09-29T16:30:00Z"] {
            let missing = timezone_fixture(value, false);
            assert_eq!(action_source_times(&missing), vec![expected.clone(); 2]);
        }
    }

    #[test]
    fn missing_source_naive_time_stays_local_without_double_conversion() {
        for value in ["2026-09-30 00:30:00", "2026-09-30T00:30:00"] {
            let conn = timezone_fixture(value, false);
            assert_eq!(action_source_times(&conn), vec!["2026-09-30 00:30:00"; 2]);
        }
    }

    #[test]
    fn missing_source_unparseable_time_preserves_existing_visible_value() {
        for value in ["", "unknown-time"] {
            let conn = timezone_fixture(value, false);
            assert_eq!(action_source_times(&conn), vec![value; 2]);
        }
    }

    fn action_sort_fixture(
        status: &str,
        items: &[(&str, &str, &str, Option<&str>)],
    ) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into profiles(id, platform, label, created_at, updated_at)
             values('sort-profile', 'wecom', '排序示例', 'fixture', 'fixture')",
            [],
        )
        .expect("profile");
        for &(id, source, updated, completed) in items {
            conn.execute(
                "insert into action_items(
                   id, type, status, priority, title, description, profile_id, platform,
                   chat_id, chat_name, first_detected_at, last_updated_at, completed_at
                 ) values(?1, 'task', ?2, 'medium', '排序示例', '合成示例', 'sort-profile',
                          'wecom', 'sort-chat', '排序示例', ?3, ?4, ?5)",
                params![id, status, source, updated, completed],
            )
            .expect("action");
        }
        conn
    }

    fn sorted_action_ids(conn: &rusqlite::Connection, profile: &str) -> Vec<String> {
        list_actions(conn, "task", profile)
            .expect("actual list_actions")
            .into_iter()
            .map(|item| item.id)
            .collect()
    }

    #[test]
    fn completed_actions_sort_by_instants_across_stored_formats() {
        for (earlier_completed, earlier_updated, later_completed, later_updated) in [
            (
                Some("2026-09-30T03:40:00+00:00"),
                "2026-09-30 03:40:00",
                None,
                "2026-09-30 03:45:00",
            ),
            (
                Some("2026-09-30T11:40:00+08:00"),
                "2026-09-30 03:40:00",
                Some("2026-09-29T23:45:00-04:00"),
                "2026-09-30 03:45:00",
            ),
            (
                Some("2026-11-01T01:50:00-04:00"),
                "2026-11-01 05:50:00",
                Some("2026-11-01T01:10:00-05:00"),
                "2026-11-01 06:10:00",
            ),
        ] {
            let conn = action_sort_fixture(
                "done",
                &[
                    (
                        "earlier",
                        "2026-09-30 11:00:00",
                        earlier_updated,
                        earlier_completed,
                    ),
                    (
                        "later",
                        "2026-09-30 11:00:00",
                        later_updated,
                        later_completed,
                    ),
                ],
            );
            for profile in ["aggregate", "sort-profile"] {
                assert_eq!(sorted_action_ids(&conn, profile), vec!["later", "earlier"]);
            }
        }
    }

    #[test]
    fn completed_actions_preserve_fractional_seconds() {
        let conn = action_sort_fixture(
            "done",
            &[
                (
                    "whole-second",
                    "2026-09-30 11:00:00",
                    "2026-09-30 03:40:00",
                    None,
                ),
                (
                    "half-second",
                    "2026-09-30 11:00:00",
                    "2026-09-30 03:40:00",
                    Some("2026-09-30T03:40:00.500Z"),
                ),
            ],
        );
        for profile in ["aggregate", "sort-profile"] {
            assert_eq!(
                sorted_action_ids(&conn, profile),
                vec!["half-second", "whole-second"]
            );
        }
    }

    #[test]
    fn source_sort_keeps_naive_local_contract_for_open_and_ignored() {
        for status in ["open", "ignored"] {
            let conn = action_sort_fixture(
                status,
                &[
                    ("older", "2026-09-30 11:35:00", "2026-09-30 03:50:00", None),
                    ("newer", "2026-09-30 11:40:00", "2026-09-30 03:30:00", None),
                ],
            );
            let expected = if status == "open" {
                vec!["older", "newer"]
            } else {
                vec!["newer", "older"]
            };
            for profile in ["aggregate", "sort-profile"] {
                assert_eq!(sorted_action_ids(&conn, profile), expected);
                let items = list_actions(&conn, "task", profile).expect("actual list_actions");
                for item in items {
                    let expected_time = if item.id == "older" {
                        "2026-09-30 11:35:00"
                    } else {
                        "2026-09-30 11:40:00"
                    };
                    assert_eq!(item.source_message_at, expected_time);
                }
            }
        }
    }

    #[test]
    fn marking_one_historical_task_done_keeps_other_carry_over_tasks_visible() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into profiles(id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at)
             values('profile-1', 'feishu', '飞书', 1, '{\"remark\":\"工作号\"}', 'normal', 0, datetime('now'), datetime('now'))",
            [],
        )
        .expect("profile");
        conn.execute(
            "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id,
               chat_name, source_message_ids, evidence_summary, carry_over, first_detected_at, last_updated_at
             )
             values('act-1', 'task', 'open', 'medium', '历史待办一', '历史描述',
                    'profile-1', 'feishu', 'chat-1', '旧聊天一', '[\"msg-old-1\"]',
                    '历史证据', 1, '2026-05-05 09:00:00', '2026-05-06 09:30:00'),
                   ('act-2', 'task', 'open', 'medium', '历史待办二', '历史描述',
                    'profile-1', 'feishu', 'chat-2', '旧聊天二', '[\"msg-old-2\"]',
                    '历史证据', 1, '2026-05-05 10:00:00', '2026-05-05 10:00:00'),
                   ('act-3', 'task', 'open', 'medium', '历史待办三', '历史描述',
                    'profile-1', 'feishu', 'chat-3', '旧聊天三', '[\"msg-old-3\"]',
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
