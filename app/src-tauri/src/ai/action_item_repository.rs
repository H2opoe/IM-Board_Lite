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
