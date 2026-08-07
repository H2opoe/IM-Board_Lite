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
