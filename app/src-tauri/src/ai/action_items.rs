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

include!("action_item_sources.rs");
include!("action_item_completion.rs");
include!("action_item_repository.rs");
include!("action_item_merge.rs");
