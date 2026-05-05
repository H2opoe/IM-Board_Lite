fn reset_ai_generated_cache(
    state: &State<'_, AppState>,
    day: &daily_cache::DashboardDay,
    target_profiles: &[ImProfile],
) -> anyhow::Result<()> {
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    reset_ai_generated_cache_conn(&conn, day, target_profiles)
}

fn reset_ai_generated_cache_conn(
    conn: &rusqlite::Connection,
    day: &daily_cache::DashboardDay,
    target_profiles: &[ImProfile],
) -> anyhow::Result<()> {
    let tx = conn.unchecked_transaction()?;

    tx.execute(
        "delete from daily_stats where day = ?1 and profile_id = 'aggregate' and metric in ('topics', 'keywords')",
        params![day.day],
    )?;

    for profile in target_profiles {
        tx.execute(
            "update daily_messages set analyzed_at = null, topic_summarized_at = null where day = ?1 and profile_id = ?2",
            params![day.day, profile.id],
        )?;
        delete_regenerable_action_items_for_profile(&tx, profile.id.as_str(), day)?;
        tx.execute(
            "delete from daily_stats where day = ?1 and profile_id = ?2 and metric in ('topics', 'keywords')",
            params![day.day, profile.id],
        )?;
        tx.execute(
            "delete from daily_topics where day = ?1 and profile_id = ?2",
            params![day.day, profile.id],
        )?;
        tx.execute(
            "delete from ai_analysis_runs where day = ?1 and profile_id = ?2",
            params![day.day, profile.id],
        )?;
        tx.execute(
            "update sync_state set last_analysis_at = null, updated_at = datetime('now') where day = ?1 and profile_id = ?2",
            params![day.day, profile.id],
        )?;
    }

    tx.commit()?;
    Ok(())
}

fn delete_regenerable_action_items_for_profile(
    conn: &rusqlite::Connection,
    profile_id: &str,
    day: &daily_cache::DashboardDay,
) -> anyhow::Result<()> {
    // 历史未完成事项按来源消息发生时间判断；旧数据找不到来源消息时才退回 first_detected_at。
    conn.execute(
        "delete from action_items
         where profile_id = ?1
           and not (
             status = 'open'
             and type in ('reply', 'task')
             and carry_over = 1
             and coalesce((
               select min(daily_messages.timestamp)
               from daily_messages
               where daily_messages.id in (
                 select value from json_each(action_items.source_message_ids)
               )
             ), cast(strftime('%s', first_detected_at) as integer), 0) < ?2
           )",
        params![profile_id, day.day_start_timestamp],
    )?;
    Ok(())
}

fn delete_regenerable_action_items(
    conn: &rusqlite::Connection,
    day: &daily_cache::DashboardDay,
) -> anyhow::Result<()> {
    // 全量重新同步会重建今日消息与分析；历史口径仍以来源消息发生时间为准。
    conn.execute(
        "delete from action_items
         where not (
           status = 'open'
           and type in ('reply', 'task')
           and carry_over = 1
           and coalesce((
             select min(daily_messages.timestamp)
             from daily_messages
             where daily_messages.id in (
               select value from json_each(action_items.source_message_ids)
             )
           ), cast(strftime('%s', first_detected_at) as integer), 0) < ?1
         )",
        params![day.day_start_timestamp],
    )?;
    Ok(())
}

fn clear_dashboard_cache(
    state: &State<'_, AppState>,
    day: &daily_cache::DashboardDay,
) -> anyhow::Result<()> {
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let tx = conn.unchecked_transaction()?;

    tx.execute("delete from daily_messages", [])?;
    delete_regenerable_action_items(&tx, day)?;
    tx.execute("delete from daily_stats", [])?;
    tx.execute("delete from daily_topics", [])?;
    tx.execute("delete from ai_analysis_runs", [])?;
    tx.execute("delete from sync_state", [])?;

    tx.commit()?;
    Ok(())
}

fn insert_daily_message(
    conn: &rusqlite::Connection,
    message: &DailyMessage,
) -> anyhow::Result<i64> {
    Ok(conn.execute(
        "insert into daily_messages(
           id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
           timestamp, time_text, msg_type, content, raw_type, local_id, raw_json, content_hash, partial
         )
         values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
         on conflict(id) do update set
           chat_name = excluded.chat_name,
           sender_name = excluded.sender_name,
           time_text = excluded.time_text,
           msg_type = excluded.msg_type,
           raw_type = excluded.raw_type,
           raw_json = excluded.raw_json,
           partial = excluded.partial",
        params![
            message.id,
            message.day,
            message.profile_id,
            message.platform,
            message.chat_id,
            message.chat_name,
            i64::from(message.is_group),
            message.sender_id,
            message.sender_name,
            message.timestamp,
            message.time_text,
            message.msg_type,
            message.content,
            message.raw_type,
            message.local_id,
            message.raw_json,
            message.content_hash,
            i64::from(message.partial),
        ],
    )? as i64)
}

fn insert_messages(state: &State<'_, AppState>, messages: &[DailyMessage]) -> anyhow::Result<i64> {
    if messages.is_empty() {
        return Ok(0);
    }
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let tx = conn.unchecked_transaction()?;
    let mut inserted = 0;
    for message in messages {
        inserted += insert_daily_message(&tx, message)?;
    }
    tx.commit()?;
    Ok(inserted)
}

fn latest_saved_message_timestamps(
    state: &State<'_, AppState>,
    profile: &ImProfile,
    day: &str,
) -> anyhow::Result<HashMap<String, i64>> {
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let mut stmt = conn.prepare(
        "select chat_id, max(timestamp)
         from daily_messages
         where day = ?1 and profile_id = ?2 and platform = ?3
         group by chat_id",
    )?;
    let rows = stmt.query_map(params![day, profile.id, profile.platform], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut timestamps = HashMap::new();
    for row in rows {
        let (chat_id, timestamp) = row?;
        timestamps.insert(chat_id, timestamp);
    }
    Ok(timestamps)
}

fn resolve_target_profiles(
    conn: &rusqlite::Connection,
    profile_id: &str,
) -> anyhow::Result<Vec<ImProfile>> {
    let mut profiles = Vec::new();
    if profile_id == "aggregate" {
        let mut stmt = conn.prepare(
            "select id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at
             from profiles where enabled = 1 order by sort_order",
        )?;
        let rows = stmt.query_map([], map_profile)?;
        for row in rows {
            profiles.push(row?);
        }
    } else {
        profiles.push(conn.query_row(
            "select id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at
             from profiles where id = ?1",
            params![profile_id],
            map_profile,
        )?);
    }

    Ok(profiles)
}

fn resolve_all_profiles(conn: &rusqlite::Connection) -> anyhow::Result<Vec<ImProfile>> {
    let mut profiles = Vec::new();
    let mut stmt = conn.prepare(
        "select id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at
         from profiles order by sort_order",
    )?;
    let rows = stmt.query_map([], map_profile)?;
    for row in rows {
        profiles.push(row?);
    }
    Ok(profiles)
}

fn map_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<ImProfile> {
    let config_json: String = row.get(4)?;
    Ok(ImProfile {
        id: row.get(0)?,
        platform: row.get(1)?,
        label: row.get(2)?,
        enabled: row.get::<_, i64>(3)? == 1,
        config_json: serde_json::from_str(&config_json).unwrap_or_else(|_| serde_json::json!({})),
        status: row.get(5)?,
        sort_order: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}
