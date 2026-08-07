use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::Local;
use rusqlite::params;
use serde::Deserialize;

use crate::analysis::local_keywords::{is_generic_single_term, should_skip_keyword_message};
use crate::analysis::message_noise;

use super::{
    clean_message_content_for_ai, current_keyword_version, truncate_text, upsert_keyword_meta,
    upsert_stat, MIN_KEYWORD_CLOUD_COUNT,
};

const MAX_REFINED_KEYWORDS: usize = 30;
pub(crate) const MAX_KEYWORD_REFINE_ANALYSIS_MESSAGES: usize = 300;

#[derive(Debug, Clone)]
pub(crate) struct KeywordRefinePlan {
    pub(crate) payload: serde_json::Value,
    versions: HashMap<String, String>,
    messages_by_profile: HashMap<String, Vec<KeywordRefineMessageForAi>>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct KeywordRefineMessageForAi {
    id: String,
    profile_id: String,
    chat_id: String,
    chat_name: String,
    is_group: bool,
    sender_name: String,
    time_text: String,
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiKeywordItem {
    #[serde(default)]
    profile_id: String,
    display: String,
    #[serde(default)]
    aliases: Vec<String>,
    category: String,
    #[serde(default)]
    valid: bool,
    confidence: f64,
    #[serde(default, alias = "score_multiplier")]
    score_multiplier: f64,
    #[serde(default, alias = "source_message_ids")]
    source_message_ids: Vec<String>,
}

pub(crate) fn build_keyword_refine_plan(
    conn: &rusqlite::Connection,
    day: &str,
    profile_ids: &[String],
    analysis_message_count: usize,
) -> anyhow::Result<Option<KeywordRefinePlan>> {
    if analysis_message_count == 0 || analysis_message_count > MAX_KEYWORD_REFINE_ANALYSIS_MESSAGES
    {
        return Ok(None);
    }

    let mut versions = HashMap::new();
    let mut messages_by_profile = HashMap::new();
    let mut payload_messages = Vec::new();
    for profile_id in profile_ids {
        let Some(version) = current_keyword_version(conn, day, profile_id)? else {
            continue;
        };
        let messages = load_keyword_refine_messages(conn, day, profile_id)?;
        if messages.is_empty() {
            continue;
        }
        versions.insert(profile_id.clone(), version);
        payload_messages.extend(
            messages
                .iter()
                .cloned()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?,
        );
        messages_by_profile.insert(profile_id.clone(), messages);
    }

    if payload_messages.is_empty() || payload_messages.len() > MAX_KEYWORD_REFINE_ANALYSIS_MESSAGES
    {
        return Ok(None);
    }
    Ok(Some(KeywordRefinePlan {
        payload: serde_json::json!({
            "scene": "IM聚合AI看板关键词词云",
            "date": day,
            "goal": "直接从当天原始聊天消息中识别适合展示在今日热词词云里的关键词",
            "rulesVersion": "keyword_ai_direct_from_messages_v1",
            "maxValidKeywords": MAX_REFINED_KEYWORDS,
            "messages": payload_messages,
        }),
        versions,
        messages_by_profile,
    }))
}

pub(crate) fn mark_keyword_refine_pending(
    conn: &rusqlite::Connection,
    day: &str,
    plan: &KeywordRefinePlan,
) -> anyhow::Result<()> {
    invalidate_aggregate_keyword_cache(conn, day)?;
    for (profile_id, version) in &plan.versions {
        upsert_keyword_meta(conn, day, profile_id, version, "local_pending_ai")?;
        mark_keywords_status(conn, day, profile_id, "local_pending_ai")?;
    }
    Ok(())
}

pub(crate) fn mark_keyword_refine_failed(
    conn: &rusqlite::Connection,
    day: &str,
    plan: &KeywordRefinePlan,
) -> anyhow::Result<()> {
    invalidate_aggregate_keyword_cache(conn, day)?;
    for (profile_id, version) in &plan.versions {
        if current_keyword_version(conn, day, profile_id)?.as_deref() == Some(version.as_str()) {
            upsert_keyword_meta(conn, day, profile_id, version, "ai_failed")?;
            mark_keywords_status(conn, day, profile_id, "ai_failed")?;
        }
    }
    Ok(())
}

pub(crate) fn persist_refined_keywords_from_analysis(
    conn: &rusqlite::Connection,
    day: &str,
    plan: &KeywordRefinePlan,
    keyword_values: &[serde_json::Value],
) -> anyhow::Result<usize> {
    let mut grouped = HashMap::<String, Vec<AiKeywordItem>>::new();
    for value in keyword_values {
        let item = serde_json::from_value::<AiKeywordItem>(value.clone());
        let Ok(item) = item else {
            continue;
        };
        if !plan.messages_by_profile.contains_key(&item.profile_id) {
            continue;
        }
        grouped
            .entry(item.profile_id.clone())
            .or_default()
            .push(item);
    }

    invalidate_aggregate_keyword_cache(conn, day)?;
    let mut replaced = 0;
    for (profile_id, version) in &plan.versions {
        let Some(messages) = plan.messages_by_profile.get(profile_id) else {
            continue;
        };
        let items = grouped.remove(profile_id).unwrap_or_default();
        let refined = keywords_from_ai_items(messages, items);
        if replace_keywords_if_current(conn, day, profile_id, version, refined, "ai_refined")? {
            replaced += 1;
        }
    }
    Ok(replaced)
}

fn load_keyword_refine_messages(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<KeywordRefineMessageForAi>> {
    let mut stmt = conn.prepare(
        "select id, profile_id, chat_id, chat_name, is_group, sender_name, time_text, msg_type, content
         from daily_messages
         where day = ?1 and profile_id = ?2 and trim(content) <> ''
         order by timestamp asc
         limit 1200",
    )?;
    let rows = stmt.query_map(params![day, profile_id], |row| {
        let msg_type: String = row.get(7)?;
        let raw_content: String = row.get(8)?;
        let is_group = row.get::<_, i64>(4)? == 1;
        if message_noise::is_message_noise(Some(&msg_type), &raw_content, is_group) {
            return Ok(None);
        }
        let Some(content) = clean_message_content_for_ai(&msg_type, &raw_content) else {
            return Ok(None);
        };
        if should_skip_keyword_message(&raw_content, Some(&msg_type))
            || should_skip_keyword_message(&content, Some(&msg_type))
        {
            return Ok(None);
        }
        let content = redact_keyword_sample(&content);
        if content.is_empty() {
            return Ok(None);
        }
        Ok(Some(KeywordRefineMessageForAi {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            chat_id: row.get(2)?,
            chat_name: row.get(3)?,
            is_group,
            sender_name: row.get(5)?,
            time_text: row.get(6)?,
            content,
        }))
    })?;
    let mut messages = Vec::new();
    for row in rows {
        let Some(message) = row? else {
            continue;
        };
        messages.push(message);
        if messages.len() > MAX_KEYWORD_REFINE_ANALYSIS_MESSAGES {
            break;
        }
    }
    Ok(messages)
}

fn redact_keyword_sample(content: &str) -> String {
    truncate_text(
        content
            .split_whitespace()
            .map(redact_sensitive_token)
            .collect::<Vec<_>>()
            .join(" "),
        120,
    )
}

fn redact_sensitive_token(token: &str) -> String {
    let ascii_digits = token.chars().filter(|ch| ch.is_ascii_digit()).count();
    if token.contains('@') && token.contains('.') {
        return "[邮箱]".to_owned();
    }
    if ascii_digits >= 11 {
        return "[号码]".to_owned();
    }
    if ascii_digits >= 6 && token.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return "[编号]".to_owned();
    }
    token.to_owned()
}

fn keywords_from_ai_items(
    messages: &[KeywordRefineMessageForAi],
    ai_items: Vec<AiKeywordItem>,
) -> Vec<serde_json::Value> {
    let message_by_id = messages
        .iter()
        .map(|message| (message.id.as_str(), message))
        .collect::<HashMap<_, _>>();
    let mut merged = Vec::new();
    for item in ai_items.into_iter().take(MAX_REFINED_KEYWORDS) {
        let display = item.display.trim();
        let confidence = item.confidence.clamp(0.0, 1.0);
        let multiplier = item.score_multiplier.clamp(0.0, 1.5);
        if !item.valid
            || display.is_empty()
            || multiplier <= 0.0
            || matches!(
                item.category.as_str(),
                "system_noise" | "marketing_noise" | "technical_noise" | "generic" | "person"
            )
        {
            continue;
        }
        if is_generic_single_term(display) {
            continue;
        }
        let aliases = normalized_aliases(display, &item.aliases);
        let matched_messages = matched_keyword_messages(messages, &message_by_id, &item, &aliases);
        let message_count = matched_messages.len() as i64;
        if message_count < MIN_KEYWORD_CLOUD_COUNT {
            continue;
        }
        let chat_count = matched_messages
            .iter()
            .map(|message| message.chat_id.as_str())
            .collect::<HashSet<_>>()
            .len()
            .max(1) as i64;
        let confidence_weight = 0.5 + 0.5 * confidence;
        let final_score = ((message_count as f64).ln_1p() * (chat_count as f64).ln_1p() * 10.0)
            * multiplier
            * confidence_weight;
        let weight = final_score.round().max(message_count as f64).max(1.0) as i64;
        merged.push(serde_json::json!({
            "text": display,
            "display": display,
            "score": final_score,
            "localScore": 0.0,
            "aiScore": final_score,
            "weight": weight,
            "count": message_count,
            "messageCount": message_count,
            "chatCount": chat_count,
            "category": item.category,
            "confidence": confidence,
            "aliases": aliases,
            "sourceMessageIds": matched_messages.iter().map(|message| message.id.clone()).collect::<Vec<_>>(),
            "source": "ai_refined",
        }));
    }
    merged.sort_by(|left, right| {
        let left_score = left
            .get("score")
            .and_then(|value| value.as_f64())
            .unwrap_or_default();
        let right_score = right
            .get("score")
            .and_then(|value| value.as_f64())
            .unwrap_or_default();
        right_score
            .partial_cmp(&left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    merged.truncate(MAX_REFINED_KEYWORDS);
    merged
}

fn normalized_aliases(display: &str, aliases: &[String]) -> Vec<String> {
    let mut values = BTreeSet::new();
    values.insert(display.to_owned());
    for alias in aliases {
        let alias = alias.trim();
        if !alias.is_empty() && !is_generic_single_term(alias) {
            values.insert(alias.to_owned());
        }
    }
    values.into_iter().collect()
}

fn matched_keyword_messages<'a>(
    messages: &'a [KeywordRefineMessageForAi],
    message_by_id: &HashMap<&str, &'a KeywordRefineMessageForAi>,
    item: &AiKeywordItem,
    aliases: &[String],
) -> Vec<&'a KeywordRefineMessageForAi> {
    let mut matched = BTreeSet::new();
    for message_id in &item.source_message_ids {
        if message_by_id.contains_key(message_id.as_str()) {
            matched.insert(message_id.as_str());
        }
    }
    if matched.is_empty() {
        let needles = aliases
            .iter()
            .map(|alias| alias.to_lowercase())
            .collect::<Vec<_>>();
        for message in messages {
            let content = message.content.to_lowercase();
            if needles.iter().any(|needle| content.contains(needle)) {
                matched.insert(message.id.as_str());
            }
        }
    }
    matched
        .into_iter()
        .filter_map(|message_id| message_by_id.get(message_id).copied())
        .collect()
}

fn replace_keywords_if_current(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    version: &str,
    mut keywords: Vec<serde_json::Value>,
    status: &str,
) -> anyhow::Result<bool> {
    if current_keyword_version(conn, day, profile_id)?.as_deref() != Some(version) {
        return Ok(false);
    }
    invalidate_aggregate_keyword_cache(conn, day)?;
    let updated_at = Local::now().to_rfc3339();
    for keyword in &mut keywords {
        if let Some(object) = keyword.as_object_mut() {
            object.insert("status".to_owned(), serde_json::json!(status));
            object.insert("version".to_owned(), serde_json::json!(version));
            object.insert("updatedAt".to_owned(), serde_json::json!(updated_at));
        }
    }
    upsert_stat(conn, day, profile_id, "keywords", &keywords)?;
    upsert_keyword_meta(conn, day, profile_id, version, status)?;
    Ok(true)
}

fn invalidate_aggregate_keyword_cache(
    conn: &rusqlite::Connection,
    day: &str,
) -> anyhow::Result<()> {
    conn.execute(
        "delete from daily_stats where day = ?1 and profile_id = 'aggregate' and metric = 'keywords'",
        params![day],
    )?;
    Ok(())
}

fn mark_keywords_status(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    status: &str,
) -> anyhow::Result<()> {
    let raw: String = conn.query_row(
        "select value_json from daily_stats where day = ?1 and profile_id = ?2 and metric = 'keywords'",
        params![day, profile_id],
        |row| row.get(0),
    )?;
    let mut keywords = serde_json::from_str::<Vec<serde_json::Value>>(&raw)?;
    let updated_at = Local::now().to_rfc3339();
    for keyword in &mut keywords {
        if let Some(object) = keyword.as_object_mut() {
            object.insert("status".to_owned(), serde_json::json!(status));
            object.insert("updatedAt".to_owned(), serde_json::json!(updated_at));
        }
    }
    upsert_stat(conn, day, profile_id, "keywords", &keywords)
}
