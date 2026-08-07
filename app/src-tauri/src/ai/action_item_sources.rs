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
