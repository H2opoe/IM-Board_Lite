use std::collections::BTreeSet;

use rusqlite::{params, OptionalExtension};

use crate::analysis::local_keywords::local_keywords;

use super::*;

pub(crate) fn persist_summary_stats(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    summary: AiSummary,
    candidates: &[SummaryCandidate],
) -> anyhow::Result<()> {
    let tx = conn.unchecked_transaction()?;
    upsert_stat(&tx, day, profile_id, "topics", &summary.topics)?;
    mark_topic_candidates_summarized(&tx, day, profile_id, candidates)?;
    tx.commit()?;
    Ok(())
}

fn mark_topic_candidates_summarized(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    candidates: &[SummaryCandidate],
) -> anyhow::Result<()> {
    let message_ids = candidates
        .iter()
        .flat_map(|candidate| candidate.source_message_ids.iter())
        .filter(|id| !id.trim().is_empty())
        .collect::<BTreeSet<_>>();
    for message_id in message_ids {
        if profile_id == "aggregate" {
            conn.execute(
                "update daily_messages set topic_summarized_at = datetime('now') where id = ?1 and day = ?2",
                params![message_id, day],
            )?;
        } else {
            conn.execute(
                "update daily_messages set topic_summarized_at = datetime('now') where id = ?1 and day = ?2 and profile_id = ?3",
                params![message_id, day, profile_id],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn persist_local_keyword_stats(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<usize> {
    persist_local_keyword_stats_with_status(conn, day, profile_id, "local_final")
}

pub(crate) fn persist_local_keyword_stats_with_status(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    status: &str,
) -> anyhow::Result<usize> {
    let mut keywords = local_keywords(conn, day, profile_id)?;
    let version = keyword_stats_version(day, profile_id, &keywords)?;
    let updated_at = chrono::Local::now().to_rfc3339();
    for keyword in &mut keywords {
        if let Some(object) = keyword.as_object_mut() {
            object.insert("status".to_owned(), serde_json::json!(status));
            object.insert("version".to_owned(), serde_json::json!(version));
            object.insert("updatedAt".to_owned(), serde_json::json!(updated_at));
        }
    }
    upsert_stat(conn, day, profile_id, "keywords", &keywords)?;
    upsert_keyword_meta(conn, day, profile_id, &version, status)?;
    Ok(keywords.len())
}

pub(crate) fn keyword_stats_version(
    day: &str,
    profile_id: &str,
    keywords: &[serde_json::Value],
) -> anyhow::Result<String> {
    Ok(format!(
        "kw_{}",
        hash_text(&format!(
            "{}|{}|{}",
            day,
            profile_id,
            serde_json::to_string(keywords)?
        ))
    ))
}

pub(crate) fn upsert_keyword_meta(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    version: &str,
    status: &str,
) -> anyhow::Result<()> {
    let key = keyword_meta_key(day, profile_id);
    conn.execute(
        "insert into app_meta(key, value, updated_at) values(?1, ?2, datetime('now'))
         on conflict(key) do update set value = excluded.value, updated_at = excluded.updated_at",
        params![
            key,
            serde_json::to_string(&serde_json::json!({
                "version": version,
                "status": status,
            }))?
        ],
    )?;
    Ok(())
}

pub(crate) fn current_keyword_version(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Option<String>> {
    let key = keyword_meta_key(day, profile_id);
    let value = conn
        .query_row(
            "select value from app_meta where key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(value
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .and_then(|value| {
            value
                .get("version")
                .and_then(|inner| inner.as_str())
                .map(str::to_owned)
        }))
}

fn keyword_meta_key(day: &str, profile_id: &str) -> String {
    format!("keyword_refine:{day}:{profile_id}")
}

pub(crate) fn upsert_stat(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    metric: &str,
    values: &[serde_json::Value],
) -> anyhow::Result<()> {
    let id = format!(
        "stat_{}",
        hash_text(&format!("{day}|{profile_id}|{metric}"))
    );
    conn.execute(
        "insert into daily_stats(id, day, profile_id, metric, value_json, updated_at)
         values(?1, ?2, ?3, ?4, ?5, datetime('now'))
         on conflict(day, profile_id, metric) do update set
           value_json = excluded.value_json,
           updated_at = excluded.updated_at",
        params![id, day, profile_id, metric, serde_json::to_string(values)?],
    )?;
    Ok(())
}
