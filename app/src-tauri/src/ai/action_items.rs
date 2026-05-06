use std::collections::{BTreeSet, HashMap, HashSet};

use super::*;
use rusqlite::{params, OptionalExtension};

pub(super) fn persist_analysis_across_profiles(
    conn: &rusqlite::Connection,
    day: &str,
    messages: &[AnalysisMessage],
    analysis: AiAnalysis,
) -> anyhow::Result<PersistedAnalysis> {
    let tx = conn.unchecked_transaction()?;
    let message_sources = analysis_message_sources(messages);

    let mut inserted = 0;
    let mut context_requests = Vec::new();
    for item in analysis.action_items {
        if item.item_type != "reply" && item.item_type != "task" {
            continue;
        }
        let Some(source) = resolve_action_item_source(&tx, day, &item, &message_sources)? else {
            continue;
        };
        let inferred_source_message_ids =
            infer_action_source_message_ids(&tx, day, &source, &item)?;
        let id = resolve_action_item_id_for_chat(
            &tx,
            day,
            &source.profile_id,
            &source.chat_id,
            &item,
            &inferred_source_message_ids,
        )?;
        let source_message_ids = merge_source_message_ids(&tx, &id, &inferred_source_message_ids)?;
        let evidence_summary =
            merge_evidence_summary(&tx, &id, &truncate_text(item.evidence_summary.clone(), 500))?;
        let title = truncate_text(item.title.clone(), 80);
        let description = truncate_text(item.description.clone(), 500);
        let suggested_reply = item
            .suggested_reply
            .clone()
            .map(|value| truncate_text(value, 500));
        let mut status = normalize_action_status(item.status.as_deref());
        if status == "open"
            && item.item_type == "reply"
            && reply_has_user_response_after_source(&tx, day, &source, &source_message_ids)?
        {
            status = "done";
        }
        let completed_at = if status == "done" {
            Some(Local::now().to_rfc3339())
        } else {
            None
        };
        let first_detected_at = source_message_first_detected_at(&tx, &source_message_ids)?;
        tx.execute(
            "insert into action_items(
               id, type, status, priority, title, description, suggested_reply, profile_id, platform,
               chat_id, chat_name, source_message_ids, evidence_summary, context_incomplete, carry_over,
               first_detected_at, last_updated_at, completed_at
             )
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 1, ?15, datetime('now'), ?16)
             on conflict(id) do update set
               status = excluded.status,
               priority = excluded.priority,
               title = excluded.title,
               description = excluded.description,
               suggested_reply = excluded.suggested_reply,
               source_message_ids = excluded.source_message_ids,
               evidence_summary = excluded.evidence_summary,
               context_incomplete = excluded.context_incomplete,
               last_updated_at = excluded.last_updated_at,
               completed_at = excluded.completed_at",
            params![
                id,
                item.item_type,
                status,
                normalize_priority(&item.priority),
                title,
                description,
                suggested_reply,
                &source.profile_id,
                &source.platform,
                &source.chat_id,
                &source.chat_name,
                serde_json::to_string(&source_message_ids)?,
                evidence_summary,
                i64::from(item.context_incomplete),
                first_detected_at,
                completed_at,
            ],
        )?;
        inserted += 1;
        if item.context_incomplete {
            context_requests.push(ContextBackfillRequest {
                action_id: id,
                profile_id: source.profile_id,
                chat_id: source.chat_id,
                chat_name: source.chat_name,
                is_group: source.is_group,
            });
        }
    }

    let mut profile_ids = BTreeSet::<String>::new();
    for message in messages {
        tx.execute(
            "update daily_messages set analyzed_at = datetime('now') where id = ?1 and day = ?2 and profile_id = ?3",
            params![message.id, day, message.profile_id],
        )?;
        profile_ids.insert(message.profile_id.clone());
    }
    for profile_id in profile_ids {
        tx.execute(
            "insert into sync_state(profile_id, day, last_analysis_at, cursor_json, updated_at)
             values(?1, ?2, datetime('now'), '{}', datetime('now'))
             on conflict(profile_id, day) do update set
               last_analysis_at = excluded.last_analysis_at,
               updated_at = excluded.updated_at",
            params![profile_id, day],
        )?;
    }
    tx.commit()?;
    Ok(PersistedAnalysis {
        affected_action_items: inserted,
        context_requests,
    })
}

#[derive(Debug, Clone)]
pub(super) struct ActionItemSource {
    pub(super) profile_id: String,
    platform: String,
    pub(super) chat_id: String,
    pub(super) chat_name: String,
    pub(super) is_group: bool,
    timestamp: i64,
}

pub(super) fn analysis_message_sources(
    messages: &[AnalysisMessage],
) -> HashMap<String, ActionItemSource> {
    messages
        .iter()
        .map(|message| {
            (
                message.id.clone(),
                ActionItemSource {
                    profile_id: message.profile_id.clone(),
                    platform: message.platform.clone(),
                    chat_id: message.chat_id.clone(),
                    chat_name: message.chat_name.clone(),
                    is_group: message.is_group,
                    timestamp: message.timestamp,
                },
            )
        })
        .collect()
}

fn source_message_first_detected_at(
    conn: &rusqlite::Connection,
    source_message_ids: &[String],
) -> anyhow::Result<String> {
    if source_message_ids.is_empty() {
        return Ok(Local::now().to_rfc3339());
    }
    let placeholders = (1..=source_message_ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "select datetime(min(timestamp), 'unixepoch', 'localtime')
         from daily_messages
         where id in ({placeholders})"
    );
    Ok(conn
        .query_row(&sql, params_from_iter(source_message_ids.iter()), |row| {
            row.get::<_, Option<String>>(0)
        })?
        .unwrap_or_else(|| Local::now().to_rfc3339()))
}

pub(super) fn resolve_action_item_source(
    conn: &rusqlite::Connection,
    day: &str,
    item: &AiActionItem,
    message_sources: &HashMap<String, ActionItemSource>,
) -> anyhow::Result<Option<ActionItemSource>> {
    if let Some(existing_id) = item
        .existing_action_item_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let matched = conn
            .query_row(
                "select profile_id, platform, chat_id, chat_name
                 from action_items where id = ?1 and status = 'open'",
                params![existing_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        if let Some((profile_id, platform, chat_id, chat_name)) = matched {
            // action_items 只保存待办归属，群聊属性从当日消息回补，避免待办表重复维护派生上下文。
            let is_group = chat_meta(conn, day, &profile_id, &chat_id)?
                .map(|(_, _, is_group, _)| is_group)
                .unwrap_or(false);
            return Ok(Some(ActionItemSource {
                profile_id,
                platform,
                chat_id,
                chat_name,
                is_group,
                timestamp: 0,
            }));
        }
    }

    if let Some(profile_id) = item
        .profile_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(source) = item
            .source_message_ids
            .iter()
            .filter_map(|id| message_sources.get(id))
            .find(|source| source.profile_id == profile_id)
        {
            return Ok(Some(source.clone()));
        }
        if let Some((platform, chat_name, is_group, timestamp)) =
            chat_meta(conn, day, profile_id, &item.chat_id)?
        {
            return Ok(Some(ActionItemSource {
                profile_id: profile_id.to_owned(),
                platform,
                chat_id: item.chat_id.clone(),
                chat_name,
                is_group,
                timestamp,
            }));
        }
    }

    for message_id in &item.source_message_ids {
        if let Some(source) = message_sources.get(message_id) {
            return Ok(Some(source.clone()));
        }
    }

    conn.query_row(
        "select profile_id, platform, chat_id, chat_name, is_group, timestamp
         from daily_messages where day = ?1 and chat_id = ?2
         order by timestamp desc limit 1",
        params![day, item.chat_id],
        |row| {
            Ok(ActionItemSource {
                profile_id: row.get(0)?,
                platform: row.get(1)?,
                chat_id: row.get(2)?,
                chat_name: row.get(3)?,
                is_group: row.get::<_, i64>(4)? == 1,
                timestamp: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn chat_meta(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    chat_id: &str,
) -> anyhow::Result<Option<(String, String, bool, i64)>> {
    conn.query_row(
        "select platform, chat_name, is_group, timestamp from daily_messages
         where day = ?1 and profile_id = ?2 and chat_id = ?3
         order by timestamp desc limit 1",
        params![day, profile_id, chat_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, i64>(2)? == 1,
                row.get(3)?,
            ))
        },
    )
    .optional()
    .map_err(Into::into)
}

fn infer_action_source_message_ids(
    conn: &rusqlite::Connection,
    day: &str,
    source: &ActionItemSource,
    item: &AiActionItem,
) -> anyhow::Result<Vec<String>> {
    if !item.source_message_ids.is_empty() {
        return Ok(item.source_message_ids.clone());
    }
    if item.item_type != "reply" {
        return latest_chat_message_id(conn, day, source).map(|id| id.into_iter().collect());
    }

    let messages = load_chat_messages_until_source(conn, day, source)?;
    // 模型偶尔会漏填 sourceMessageIds。待回复事项优先挂到最后一条真实提问，
    // 避免把 AI 写入时间误当成消息时间，也为后续“已回复”校正提供稳定锚点。
    let source_id = messages
        .iter()
        .rev()
        .find(|message| !message.is_me && !is_reply_acknowledgement(&message.content))
        .map(|message| message.id.clone())
        .or_else(|| {
            messages
                .iter()
                .rev()
                .find(|message| !message.is_me)
                .map(|message| message.id.clone())
        });
    Ok(source_id.into_iter().collect())
}

fn latest_chat_message_id(
    conn: &rusqlite::Connection,
    day: &str,
    source: &ActionItemSource,
) -> anyhow::Result<Option<String>> {
    conn.query_row(
        "select id from daily_messages
         where day = ?1 and profile_id = ?2 and chat_id = ?3 and trim(content) <> ''
         order by timestamp desc limit 1",
        params![day, source.profile_id, source.chat_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

#[derive(Debug)]
struct ChatMessageForAction {
    id: String,
    is_me: bool,
    content: String,
}

fn load_chat_messages_until_source(
    conn: &rusqlite::Connection,
    day: &str,
    source: &ActionItemSource,
) -> anyhow::Result<Vec<ChatMessageForAction>> {
    let timestamp_clause = if source.timestamp > 0 {
        "and timestamp <= ?4"
    } else {
        "and 1 = ?4"
    };
    let sql = format!(
        "select id, sender_id, sender_name, content
         from daily_messages
         where day = ?1 and profile_id = ?2 and chat_id = ?3
           and trim(content) <> '' and partial = 0 {timestamp_clause}
         order by timestamp asc"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params![
            day,
            source.profile_id,
            source.chat_id,
            if source.timestamp > 0 {
                source.timestamp
            } else {
                1
            }
        ],
        |row| {
            let sender_id: String = row.get(1)?;
            let sender_name: String = row.get(2)?;
            Ok(ChatMessageForAction {
                id: row.get(0)?,
                is_me: is_self_sender(&sender_id, &sender_name),
                content: row.get(3)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn reply_has_user_response_after_source(
    conn: &rusqlite::Connection,
    day: &str,
    source: &ActionItemSource,
    source_message_ids: &[String],
) -> anyhow::Result<bool> {
    let source_timestamp = source_message_timestamp(conn, source_message_ids)?
        .or_else(|| (source.timestamp > 0).then_some(source.timestamp));
    let Some(source_timestamp) = source_timestamp else {
        return Ok(false);
    };

    let mut stmt = conn.prepare(
        "select sender_id, sender_name, content
         from daily_messages
         where day = ?1 and profile_id = ?2 and chat_id = ?3
           and timestamp > ?4 and trim(content) <> '' and partial = 0
         order by timestamp asc",
    )?;
    let rows = stmt.query_map(
        params![day, source.profile_id, source.chat_id, source_timestamp],
        |row| {
            let sender_id: String = row.get(0)?;
            let sender_name: String = row.get(1)?;
            Ok((
                is_self_sender(&sender_id, &sender_name),
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut user_replied = false;
    for row in rows {
        let (is_me, content) = row?;
        if is_me {
            user_replied = true;
            continue;
        }
        if user_replied && is_follow_up_reply_request(&content) {
            return Ok(false);
        }
    }
    Ok(user_replied)
}

fn source_message_timestamp(
    conn: &rusqlite::Connection,
    source_message_ids: &[String],
) -> anyhow::Result<Option<i64>> {
    if source_message_ids.is_empty() {
        return Ok(None);
    }
    let placeholders = (1..=source_message_ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("select max(timestamp) from daily_messages where id in ({placeholders})");
    conn.query_row(&sql, params_from_iter(source_message_ids.iter()), |row| {
        row.get(0)
    })
    .map_err(Into::into)
}

fn is_reply_acknowledgement(content: &str) -> bool {
    let value = normalize_reply_short_text(content);
    matches!(
        value.as_str(),
        "好" | "好的" | "好呀" | "好啊" | "ok" | "okay" | "收到" | "明白" | "嗯" | "嗯嗯"
    )
}

fn is_follow_up_reply_request(content: &str) -> bool {
    if is_reply_acknowledgement(content) {
        return false;
    }
    let value = normalize_reply_short_text(content);
    value.contains('？')
        || value.contains('?')
        || value.contains("几点")
        || value.contains("什么时候")
        || value.contains("怎么")
        || value.ends_with('吗')
        || value.ends_with('嘛')
        || value.ends_with('么')
        || value.ends_with('呢')
}

fn normalize_reply_short_text(content: &str) -> String {
    content
        .trim()
        .trim_matches(|ch: char| ch.is_ascii_punctuation() || "，。！？、；：… ".contains(ch))
        .to_lowercase()
}

pub(super) fn load_action_items_for_context(
    conn: &rusqlite::Connection,
    profile_id: &str,
    chat_ids: &HashSet<String>,
) -> anyhow::Result<Vec<ActionContext>> {
    if chat_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (2..chat_ids.len() + 2)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "select id, profile_id, platform, type, status, priority, title, description, suggested_reply,
                chat_id, chat_name, evidence_summary, context_incomplete, last_updated_at
         from action_items
         where profile_id = ?1 and chat_id in ({placeholders})
         order by case status when 'open' then 0 when 'done' then 1 else 2 end,
                  last_updated_at desc
         limit 80"
    );
    let mut values = vec![profile_id.to_owned()];
    let mut ordered_chat_ids = chat_ids.iter().cloned().collect::<Vec<_>>();
    ordered_chat_ids.sort();
    values.extend(ordered_chat_ids);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values.iter()), |row| {
        Ok(ActionContext {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            platform: row.get(2)?,
            item_type: row.get(3)?,
            status: row.get(4)?,
            priority: row.get(5)?,
            title: row.get(6)?,
            description: truncate_text(row.get::<_, String>(7)?, 240),
            suggested_reply: row.get::<_, Option<String>>(8)?,
            chat_id: row.get(9)?,
            chat_name: row.get(10)?,
            evidence_summary: truncate_text(row.get::<_, String>(11)?, 240),
            context_incomplete: row.get::<_, i64>(12)? == 1,
            last_updated_at: row.get(13)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(super) fn load_action_items_for_profiles_context(
    conn: &rusqlite::Connection,
    profile_ids: &HashSet<String>,
) -> anyhow::Result<Vec<ActionContext>> {
    if profile_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (1..=profile_ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "select id, profile_id, platform, type, status, priority, title, description, suggested_reply,
                chat_id, chat_name, evidence_summary, context_incomplete, last_updated_at
         from action_items
         where profile_id in ({placeholders})
         order by case status when 'open' then 0 when 'done' then 1 else 2 end,
                  last_updated_at desc
         limit 80"
    );
    let mut ordered_profile_ids = profile_ids.iter().cloned().collect::<Vec<_>>();
    ordered_profile_ids.sort();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(ordered_profile_ids.iter()), |row| {
        Ok(ActionContext {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            platform: row.get(2)?,
            item_type: row.get(3)?,
            status: row.get(4)?,
            priority: row.get(5)?,
            title: row.get(6)?,
            description: truncate_text(row.get::<_, String>(7)?, 240),
            suggested_reply: row.get::<_, Option<String>>(8)?,
            chat_id: row.get(9)?,
            chat_name: row.get(10)?,
            evidence_summary: truncate_text(row.get::<_, String>(11)?, 240),
            context_incomplete: row.get::<_, i64>(12)? == 1,
            last_updated_at: row.get(13)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn resolve_action_item_id_for_chat(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    chat_id: &str,
    item: &AiActionItem,
    source_message_ids: &[String],
) -> anyhow::Result<String> {
    if let Some(existing_id) = item
        .existing_action_item_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let matched = conn
            .query_row(
                "select id from action_items
                 where id = ?1 and profile_id = ?2 and status = 'open'
                   and type = ?3 and chat_id = ?4",
                params![existing_id, profile_id, item.item_type, chat_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(id) = matched {
            return Ok(id);
        }
    }

    let title_key = normalized_action_key(&item.title);
    let base = format!(
        "{}|{}|{}|{}|{}|{}",
        day,
        profile_id,
        item.item_type,
        chat_id,
        title_key,
        serde_json::to_string(source_message_ids)?
    );
    let mut id = format!("act_{}", hash_text(&base));
    let existing_status = conn
        .query_row(
            "select status from action_items where id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if existing_status
        .as_deref()
        .is_some_and(|status| status != "open")
    {
        id = format!(
            "act_{}",
            hash_text(&format!(
                "{}|{}",
                base,
                Local::now().timestamp_nanos_opt().unwrap_or_default()
            ))
        );
    }
    Ok(id)
}

fn merge_source_message_ids(
    conn: &rusqlite::Connection,
    action_id: &str,
    incoming: &[String],
) -> anyhow::Result<Vec<String>> {
    let existing = conn
        .query_row(
            "select source_message_ids from action_items where id = ?1",
            params![action_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let mut merged = existing
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default();
    for id in incoming {
        if !id.trim().is_empty() && !merged.iter().any(|existing| existing == id) {
            merged.push(id.clone());
        }
    }
    Ok(merged)
}

fn merge_evidence_summary(
    conn: &rusqlite::Connection,
    action_id: &str,
    incoming: &str,
) -> anyhow::Result<String> {
    let existing = conn
        .query_row(
            "select evidence_summary from action_items where id = ?1",
            params![action_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_default();
    Ok(merge_text(existing, incoming.to_owned(), 700))
}

fn merge_text(existing: String, incoming: String, max_chars: usize) -> String {
    let existing = existing.trim();
    let incoming = incoming.trim();
    if incoming.is_empty() {
        return truncate_text(existing.to_owned(), max_chars);
    }
    if existing.is_empty() || incoming.contains(existing) {
        return truncate_text(incoming.to_owned(), max_chars);
    }
    if existing.contains(incoming) {
        return truncate_text(existing.to_owned(), max_chars);
    }
    truncate_text(format!("{existing}；{incoming}"), max_chars)
}
