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
