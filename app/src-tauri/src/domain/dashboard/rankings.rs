use std::collections::{HashMap, HashSet};

use rusqlite::params;

use crate::analysis::message_noise;

use super::sources::{is_aggregate, source_label};

pub fn chat_rank(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut sql = "select chat_name, daily_messages.platform,
                          coalesce(json_extract(profiles.config_json, '$.remark'), ''), count(*)
                   from daily_messages
                   left join profiles on profiles.id = daily_messages.profile_id
                   where day = ?1 and is_group = 0"
        .to_owned();
    if !is_aggregate(profile_id) {
        sql.push_str(" and daily_messages.profile_id = ?2");
    }
    sql.push_str(
        " group by daily_messages.profile_id, chat_id, chat_name, daily_messages.platform
          having sum(case when lower(trim(sender_id)) in ('me', 'self') or trim(sender_name) = '我' or lower(trim(sender_name)) in ('me', 'self') then 1 else 0 end) > 0
             and sum(case when lower(trim(sender_id)) not in ('me', 'self') and trim(sender_name) <> '我' and lower(trim(sender_name)) not in ('me', 'self') then 1 else 0 end) > 0
          order by count(*) desc limit 10",
    );
    let mut stmt = conn.prepare(&sql)?;
    if is_aggregate(profile_id) {
        let rows = stmt.query_map(params![day], map_chat_rank_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    } else {
        let rows = stmt.query_map(params![day, profile_id], map_chat_rank_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn map_chat_rank_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<serde_json::Value> {
    let chat: String = row.get(0)?;
    let platform: String = row.get(1)?;
    let remark: String = row.get(2)?;
    let count: i64 = row.get(3)?;
    Ok(serde_json::json!({
        "chat": chat,
        "count": count,
        "sourceLabel": source_label(&platform, &remark)
    }))
}

pub fn speaker_top(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    sender_rank(conn, day, profile_id, "speaker")
}

#[derive(Debug)]
struct SenderRankRow {
    platform: String,
    remark: String,
    profile_id: String,
    sender_id: String,
    sender_name: String,
    chat_id: String,
    chat_name: String,
    is_group: bool,
    msg_type: String,
    content: String,
}

fn sender_rank(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    label_key: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let reciprocal_direct_chats = reciprocal_direct_chats(conn, day, profile_id)?;
    let mut sql = "select daily_messages.platform,
                          coalesce(json_extract(profiles.config_json, '$.remark'), ''),
                          daily_messages.profile_id, sender_id, sender_name, chat_id, chat_name, is_group, msg_type, content
                   from daily_messages
                   left join profiles on profiles.id = daily_messages.profile_id
                   where day = ?1"
        .to_owned();
    if !is_aggregate(profile_id) {
        sql.push_str(" and daily_messages.profile_id = ?2");
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = if is_aggregate(profile_id) {
        stmt.query_map(params![day], map_sender_rank_row)?
    } else {
        stmt.query_map(params![day, profile_id], map_sender_rank_row)?
    };

    let mut counts = HashMap::<(String, String), i64>::new();
    for row in rows {
        let row = row?;
        if message_noise::is_message_noise(Some(&row.msg_type), &row.content, row.is_group) {
            continue;
        }
        if !row.is_group
            && !reciprocal_direct_chats.contains(&(row.profile_id.clone(), row.chat_id.clone()))
        {
            continue;
        }
        if is_self_sender(&row.sender_id) || is_self_sender(&row.sender_name) {
            continue;
        }
        if let Some(sender) = display_sender(&row) {
            *counts
                .entry((sender, source_label(&row.platform, &row.remark)))
                .or_insert(0) += 1;
        }
    }

    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(
        |((left_name, left_source), left_count), ((right_name, right_source), right_count)| {
            right_count
                .cmp(left_count)
                .then_with(|| left_name.cmp(right_name))
                .then_with(|| left_source.cmp(right_source))
        },
    );
    ranked.truncate(10);

    Ok(ranked
        .into_iter()
        .map(|((label, source_label), count)| serde_json::json!({ label_key: label, "count": count, "sourceLabel": source_label }))
        .collect())
}

fn map_sender_rank_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SenderRankRow> {
    Ok(SenderRankRow {
        platform: row.get(0)?,
        remark: row.get(1)?,
        profile_id: row.get(2)?,
        sender_id: row.get(3)?,
        sender_name: row.get(4)?,
        chat_id: row.get(5)?,
        chat_name: row.get(6)?,
        is_group: row.get::<_, i64>(7)? == 1,
        msg_type: row.get(8)?,
        content: row.get(9)?,
    })
}

fn reciprocal_direct_chats(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<HashSet<(String, String)>> {
    let mut sql = "select profile_id, chat_id
                   from daily_messages
                   where day = ?1 and is_group = 0"
        .to_owned();
    if !is_aggregate(profile_id) {
        sql.push_str(" and profile_id = ?2");
    }
    sql.push_str(
        " group by profile_id, chat_id
          having sum(case when lower(trim(sender_id)) in ('me', 'self') or trim(sender_name) = '我' or lower(trim(sender_name)) in ('me', 'self') then 1 else 0 end) > 0
             and sum(case when lower(trim(sender_id)) not in ('me', 'self') and trim(sender_name) <> '我' and lower(trim(sender_name)) not in ('me', 'self') then 1 else 0 end) > 0",
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = if is_aggregate(profile_id) {
        stmt.query_map(params![day], map_profile_chat_row)?
    } else {
        stmt.query_map(params![day, profile_id], map_profile_chat_row)?
    };
    rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
}

fn map_profile_chat_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String)> {
    Ok((row.get(0)?, row.get(1)?))
}

fn display_sender(row: &SenderRankRow) -> Option<String> {
    if row.is_group {
        if let Some(sender) = content_sender_prefix(&row.content) {
            return Some(sender);
        }
    }

    let sender_name = row.sender_name.trim();
    if sender_name.is_empty() {
        return None;
    }
    if row.is_group && (sender_name == row.chat_name.trim() || sender_name == row.chat_id.trim()) {
        return None;
    }
    Some(sender_name.to_owned())
}

fn content_sender_prefix(content: &str) -> Option<String> {
    let body = if let Some((_, body)) = content.trim().strip_prefix('[')?.split_once("] ") {
        body
    } else {
        content.trim()
    };
    let (sender, message) = body.split_once(": ")?;
    let sender = sender.trim();
    if sender.is_empty() || message.trim().is_empty() || is_self_sender(sender) {
        None
    } else {
        Some(sender.to_owned())
    }
}

fn is_self_sender(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "me" | "self" | "我"
    )
}
