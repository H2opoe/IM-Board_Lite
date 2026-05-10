use std::collections::{HashMap, HashSet};

use rusqlite::params;

use crate::messages::wechat_accounts::is_wechat_system_account;

use super::action_items::{
    analysis_message_sources, load_action_items_for_context,
    load_action_items_for_profiles_context, persist_analysis_across_profiles,
    resolve_action_item_source,
};
use super::*;

pub(crate) fn load_analysis_messages(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<AnalysisMessage>> {
    let mut stmt = conn.prepare(
        "select id, profile_id, platform, chat_id, chat_name, is_group, timestamp,
                sender_id, sender_name, time_text, msg_type, content, partial
         from daily_messages
         where day = ?1 and profile_id = ?2 and analyzed_at is null and trim(content) <> ''
         order by timestamp asc
         limit 1200",
    )?;
    let rows = stmt.query_map(params![day, profile_id], |row| {
        let chat_id: String = row.get(3)?;
        if is_wechat_system_account(&chat_id) {
            return Ok(None);
        }
        let sender_id: String = row.get(7)?;
        if is_wechat_system_account(&sender_id) {
            return Ok(None);
        }
        let sender_name: String = row.get(8)?;
        let is_me = is_self_sender(&sender_id, &sender_name);
        let msg_type: String = row.get(10)?;
        let raw_content: String = row.get(11)?;
        let Some(content) = clean_message_content_for_ai(&msg_type, &raw_content) else {
            return Ok(None);
        };
        Ok(Some(AnalysisMessage {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            platform: row.get(2)?,
            chat_id,
            chat_name: row.get(4)?,
            is_group: row.get::<_, i64>(5)? == 1,
            timestamp: row.get(6)?,
            sender_id,
            sender_name,
            is_me,
            time_text: row.get(9)?,
            msg_type,
            content: truncate_text(content, 500),
            partial: row.get::<_, i64>(12)? == 1,
        }))
    })?;
    let messages = rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    Ok(filter_analysis_messages(messages))
}

pub(crate) fn load_analysis_messages_for_profiles(
    conn: &rusqlite::Connection,
    day: &str,
    profile_ids: &[String],
) -> anyhow::Result<Vec<AnalysisMessage>> {
    let mut messages = Vec::new();
    for profile_id in profile_ids {
        messages.extend(load_analysis_messages(conn, day, profile_id)?);
    }
    messages.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.profile_id.cmp(&right.profile_id))
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    Ok(messages)
}

pub(crate) fn load_analysis_context_for_messages(
    conn: &rusqlite::Connection,
    messages: &[AnalysisMessage],
) -> anyhow::Result<AnalysisContext> {
    let mut chat_ids_by_profile = HashMap::<String, HashSet<String>>::new();
    for message in messages {
        chat_ids_by_profile
            .entry(message.profile_id.clone())
            .or_default()
            .insert(message.chat_id.clone());
    }
    let mut existing_action_items = Vec::new();
    for (profile_id, chat_ids) in chat_ids_by_profile {
        existing_action_items.extend(load_action_items_for_context(conn, &profile_id, &chat_ids)?);
    }
    let profile_ids = messages
        .iter()
        .map(|message| message.profile_id.clone())
        .collect::<HashSet<_>>();
    // 同一次当天首次同步/重新分析会按批落库。后续批必须看到前批已识别事项，
    // 才能在 AI 层判断“继续推进/已完成/重复事项”，前端整块刷新才不会丢掉前批语义。
    existing_action_items.extend(load_action_items_for_profiles_context(conn, &profile_ids)?);
    let mut seen = HashSet::new();
    existing_action_items.retain(|item| seen.insert(item.id.clone()));
    Ok(AnalysisContext {
        existing_action_items,
        historical_messages: Vec::new(),
    })
}

pub(crate) fn extend_analysis_context_with_history(
    context: &mut AnalysisContext,
    messages: Vec<HistoricalContextMessage>,
) {
    context.historical_messages = messages;
}

pub(crate) fn context_backfill_targets_for_messages(
    conn: &rusqlite::Connection,
    day: &str,
    messages: &[AnalysisMessage],
    analysis: &AiAnalysis,
) -> anyhow::Result<Vec<ContextBackfillTarget>> {
    let message_sources = analysis_message_sources(messages);
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for item in analysis
        .action_items
        .iter()
        .filter(|item| item.context_incomplete)
    {
        let Some(source) = resolve_action_item_source(conn, day, item, &message_sources)? else {
            continue;
        };
        if !seen.insert((source.profile_id.clone(), source.chat_id.clone())) {
            continue;
        }
        targets.push(ContextBackfillTarget {
            profile_id: source.profile_id,
            chat_id: source.chat_id,
            chat_name: source.chat_name,
            is_group: source.is_group,
        });
    }
    Ok(targets)
}

pub(crate) fn persist_analysis_for_messages(
    conn: &rusqlite::Connection,
    day: &str,
    messages: &[AnalysisMessage],
    analysis: AiAnalysis,
) -> anyhow::Result<PersistedAnalysis> {
    persist_analysis_across_profiles(conn, day, messages, analysis)
}

pub(crate) fn keep_analysis_pending_for_messages(
    conn: &rusqlite::Connection,
    day: &str,
    messages: &[AnalysisMessage],
) -> anyhow::Result<()> {
    for message in messages {
        conn.execute(
            "update daily_messages set analyzed_at = null where id = ?1 and day = ?2 and profile_id = ?3",
            params![message.id, day, message.profile_id],
        )?;
    }
    Ok(())
}
