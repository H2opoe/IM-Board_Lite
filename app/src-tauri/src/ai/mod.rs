use chrono::Local;
use jieba_rs::Jieba;
use rusqlite::{params, params_from_iter, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt;

#[cfg(test)]
use crate::storage::models::AiConfig;

mod config;
mod prompts;
mod request;

pub(crate) use config::analysis_batch_token_budget;
#[cfg(test)]
pub(crate) use config::normalize_config;
pub use config::{
    get_config, is_configured, is_local_provider, save_config, with_provider_headers,
};
pub(crate) use prompts::{
    BATCH_DEDUP_PROMPT, LOCAL_MODEL_OUTPUT_PROMPT, MANAGEMENT_RISK_PROMPT,
    PRIMARY_LANGUAGE_OUTPUT_PROMPT,
};
pub use prompts::{DEFAULT_ANALYSIS_PROMPT, DEFAULT_SUMMARY_PROMPT};
#[cfg(test)]
pub(crate) use request::merge_incremental_summary_topics;
pub(crate) use request::{
    describe_request_error, request_profile_analysis, request_profile_summary,
};

pub const LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE: i64 = 20;
pub const OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE: i64 = 100;
pub const MIN_ANALYSIS_BATCH_MESSAGES: i64 = 10;
pub const LOCAL_DEEPSEEK_MAX_ANALYSIS_BATCH_MESSAGES: i64 = 30;
pub const OTHER_MODEL_MAX_ANALYSIS_BATCH_MESSAGES: i64 = 300;
const LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS: usize = 4_096;
const OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS: usize = 32_000;
const MAX_SUMMARY_MESSAGES: usize = 800;
const MAX_SUMMARY_CANDIDATES: usize = 12;
const MAX_SUMMARY_CANDIDATES_PER_REQUEST: usize = 4;
const MAX_SUMMARY_SOURCE_MESSAGES_PER_REQUEST: usize = 16;
const MAX_SUMMARY_SNIPPETS_PER_CANDIDATE: usize = 2;
const ANALYSIS_REQUEST_TIMEOUT_SECS: u64 = 180;
const SUMMARY_REQUEST_TIMEOUT_SECS: u64 = 300;
const MAX_LOCAL_KEYWORDS: usize = 16;
const LOCAL_KEYWORD_MESSAGE_LIMIT: usize = 2000;
const LOCAL_CONTEXT_WORD_FREQ: usize = 500_000;
const MIN_REDUNDANT_KEYWORD_OVERLAP: usize = 4;
const MIN_CONTAINED_KEYWORD_CHARS: usize = 2;
pub(crate) const MIN_KEYWORD_CLOUD_COUNT: i64 = 3;

pub const LOCAL_DEEPSEEK_PROVIDER: &str = "本地DeepSeek";
pub const LOCAL_DEEPSEEK_MODEL: &str = "deepseek-r1-distill-qwen-7b-q4_k_m";
pub const OPENROUTER_PROVIDER: &str = "OpenRouter";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnalysisMessage {
    id: String,
    profile_id: String,
    platform: String,
    chat_id: String,
    chat_name: String,
    is_group: bool,
    timestamp: i64,
    sender_id: String,
    sender_name: String,
    is_me: bool,
    time_text: String,
    msg_type: String,
    content: String,
    partial: bool,
}

impl AnalysisMessage {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnalysisContext {
    existing_action_items: Vec<ActionContext>,
    historical_messages: Vec<HistoricalContextMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoricalContextMessage {
    chat_id: String,
    chat_name: String,
    day: String,
    time_text: String,
    sender_name: String,
    content: String,
}

impl HistoricalContextMessage {
    pub(crate) fn new(
        chat_id: String,
        chat_name: String,
        day: String,
        time_text: String,
        sender_name: String,
        content: String,
    ) -> Self {
        Self {
            chat_id,
            chat_name,
            day,
            time_text,
            sender_name,
            content: truncate_text(content, 220),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryCandidate {
    id: String,
    title_hint: String,
    keywords: Vec<String>,
    count: i64,
    source_chats: Vec<SummarySourceChat>,
    source_message_ids: Vec<String>,
    snippets: Vec<String>,
    risk: bool,
}

impl SummaryCandidate {
    pub(crate) fn source_message_count(&self) -> usize {
        self.source_message_ids.len()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AiCallDiagnostics {
    provider: String,
    model: String,
    endpoint: String,
    max_tokens: u32,
    response_format: Option<String>,
    http_status: Option<u16>,
    finish_reason: Option<String>,
    content_empty: bool,
    content_length: usize,
    content_snippet: Option<String>,
    reasoning_content_present: bool,
    reasoning_content_length: usize,
    response_body_length: usize,
    response_body_snippet: Option<String>,
    usage: Option<serde_json::Value>,
}

#[derive(Debug)]
pub(crate) struct AiCallError {
    message: String,
    diagnostics: Option<AiCallDiagnostics>,
}

impl AiCallError {
    fn new(message: impl Into<String>, diagnostics: Option<AiCallDiagnostics>) -> Self {
        Self {
            message: message.into(),
            diagnostics,
        }
    }

    pub(crate) fn diagnostic_json(&self) -> Option<serde_json::Value> {
        self.diagnostics
            .as_ref()
            .and_then(|diagnostics| serde_json::to_value(diagnostics).ok())
    }
}

impl fmt::Display for AiCallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AiCallError {}

#[derive(Debug)]
pub(crate) struct AiAnalysisResult {
    pub analysis: AiAnalysis,
    pub diagnostics: AiCallDiagnostics,
}

#[derive(Debug)]
pub(crate) struct AiSummaryResult {
    pub summary: AiSummary,
    pub diagnostics: AiCallDiagnostics,
}

struct AiRequestOutput {
    content: String,
    diagnostics: AiCallDiagnostics,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummarySourceChat {
    chat_name: String,
    is_group: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryTopicContext {
    id: String,
    title: String,
    summary: String,
    count: i64,
    source_chats: Vec<SummarySourceChat>,
    source_message_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActionContext {
    id: String,
    profile_id: String,
    platform: String,
    #[serde(rename = "type")]
    item_type: String,
    status: String,
    priority: String,
    title: String,
    description: String,
    suggested_reply: Option<String>,
    chat_id: String,
    chat_name: String,
    evidence_summary: String,
    context_incomplete: bool,
    last_updated_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PersistedAnalysis {
    pub affected_action_items: i64,
    pub context_requests: Vec<ContextBackfillRequest>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextBackfillRequest {
    pub action_id: String,
    pub profile_id: String,
    pub chat_id: String,
    pub chat_name: String,
    pub is_group: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextBackfillTarget {
    pub profile_id: String,
    pub chat_id: String,
    pub chat_name: String,
    pub is_group: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AiAnalysis {
    #[serde(default)]
    action_items: Vec<AiActionItem>,
    #[serde(default)]
    topics: Vec<serde_json::Value>,
    #[serde(default)]
    keywords: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AiSummary {
    #[serde(default)]
    pub topics: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiActionItem {
    #[serde(rename = "type")]
    item_type: String,
    #[serde(default)]
    status: Option<String>,
    priority: String,
    title: String,
    description: String,
    #[serde(default)]
    suggested_reply: Option<String>,
    chat_id: String,
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    existing_action_item_id: Option<String>,
    #[serde(default)]
    source_message_ids: Vec<String>,
    #[serde(default)]
    evidence_summary: String,
    #[serde(default)]
    context_incomplete: bool,
}

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
        let sender_id: String = row.get(7)?;
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
            chat_id: row.get(3)?,
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

pub(crate) fn load_summary_candidates(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SummaryCandidate>> {
    Ok(summary_candidates_from_messages(
        load_summary_candidate_messages(conn, day, profile_id)?,
    ))
}

pub(crate) fn load_summary_candidates_for_profiles(
    conn: &rusqlite::Connection,
    day: &str,
    profile_ids: &[String],
) -> anyhow::Result<Vec<SummaryCandidate>> {
    let mut messages = Vec::new();
    for profile_id in profile_ids {
        messages.extend(load_summary_candidate_messages(conn, day, profile_id)?);
    }
    messages.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.profile_id.cmp(&right.profile_id))
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    if messages.len() > MAX_SUMMARY_MESSAGES {
        messages = messages
            .into_iter()
            .rev()
            .take(MAX_SUMMARY_MESSAGES)
            .collect::<Vec<_>>();
        messages.reverse();
    }
    Ok(summary_candidates_from_messages(messages))
}

fn load_summary_candidate_messages(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<AnalysisMessage>> {
    let mut stmt = conn.prepare(
        "select id, profile_id, platform, chat_id, chat_name, is_group, timestamp,
                sender_id, sender_name, time_text, msg_type, content, partial
         from daily_messages
         where day = ?1 and profile_id = ?2 and topic_summarized_at is null and trim(content) <> ''
         order by timestamp asc
         limit ?3",
    )?;
    let rows = stmt.query_map(
        params![day, profile_id, MAX_SUMMARY_MESSAGES as i64],
        |row| {
            let sender_id: String = row.get(7)?;
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
                chat_id: row.get(3)?,
                chat_name: row.get(4)?,
                is_group: row.get::<_, i64>(5)? == 1,
                timestamp: row.get(6)?,
                sender_id,
                sender_name,
                is_me,
                time_text: row.get(9)?,
                msg_type,
                content: truncate_text(content, 220),
                partial: row.get::<_, i64>(12)? == 1,
            }))
        },
    )?;
    Ok(rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>())
}

pub(crate) fn load_existing_summary_topics(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SummaryTopicContext>> {
    let raw = conn
        .query_row(
            "select value_json from daily_stats where day = ?1 and profile_id = ?2 and metric = 'topics'",
            params![day, profile_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let topics = serde_json::from_str::<Vec<serde_json::Value>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(summary_topic_context_from_value)
        .collect::<Vec<_>>();
    Ok(topics)
}

pub(crate) fn split_analysis_batches(
    messages: Vec<AnalysisMessage>,
    max_batch_messages: i64,
    max_batch_tokens: usize,
) -> Vec<Vec<AnalysisMessage>> {
    let max_batch_messages = max_batch_messages.clamp(
        MIN_ANALYSIS_BATCH_MESSAGES,
        OTHER_MODEL_MAX_ANALYSIS_BATCH_MESSAGES,
    ) as usize;
    let mut chat_index = HashMap::<String, usize>::new();
    let mut chat_groups = Vec::<Vec<AnalysisMessage>>::new();
    for message in messages {
        let chat_key = format!("{}|{}", message.profile_id, message.chat_id);
        if let Some(index) = chat_index.get(&chat_key).copied() {
            chat_groups[index].push(message);
            continue;
        }
        chat_index.insert(chat_key, chat_groups.len());
        chat_groups.push(vec![message]);
    }

    let mut batches = Vec::<Vec<AnalysisMessage>>::new();
    let mut current = Vec::<AnalysisMessage>::new();
    let mut current_tokens = 0usize;
    for group in chat_groups {
        let group_tokens = estimate_analysis_messages_tokens(&group);
        let would_cross_message_limit = current.len() + group.len() > max_batch_messages;
        let would_cross_token_limit = current_tokens + group_tokens > max_batch_tokens;
        if !current.is_empty() && (would_cross_message_limit || would_cross_token_limit) {
            batches.push(current);
            current = Vec::new();
            current_tokens = 0;
        }
        if group.len() > max_batch_messages || group_tokens > max_batch_tokens {
            if !current.is_empty() {
                batches.push(current);
                current = Vec::new();
                current_tokens = 0;
            }
            for chunk in split_large_chat_group(group, max_batch_messages, max_batch_tokens) {
                batches.push(chunk);
            }
            continue;
        }
        current.extend(group);
        current_tokens += group_tokens;
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

pub(crate) fn split_summary_candidate_batches(
    candidates: Vec<SummaryCandidate>,
) -> Vec<Vec<SummaryCandidate>> {
    let mut batches = Vec::<Vec<SummaryCandidate>>::new();
    let mut current = Vec::<SummaryCandidate>::new();
    let mut current_source_messages = 0usize;

    for candidate in candidates {
        let candidate_source_messages = candidate.source_message_ids.len().max(1);
        let would_cross_candidate_limit = current.len() >= MAX_SUMMARY_CANDIDATES_PER_REQUEST;
        let would_cross_message_limit = current_source_messages + candidate_source_messages
            > MAX_SUMMARY_SOURCE_MESSAGES_PER_REQUEST;
        if !current.is_empty() && (would_cross_candidate_limit || would_cross_message_limit) {
            batches.push(current);
            current = Vec::new();
            current_source_messages = 0;
        }

        current_source_messages += candidate_source_messages;
        current.push(candidate);
    }

    if !current.is_empty() {
        batches.push(current);
    }

    batches
}

fn split_large_chat_group(
    messages: Vec<AnalysisMessage>,
    max_batch_messages: usize,
    max_batch_tokens: usize,
) -> Vec<Vec<AnalysisMessage>> {
    let mut batches = Vec::<Vec<AnalysisMessage>>::new();
    let mut current = Vec::<AnalysisMessage>::new();
    let mut current_tokens = 0usize;

    for message in messages {
        let message_tokens = estimate_analysis_message_tokens(&message);
        if !current.is_empty()
            && (current.len() >= max_batch_messages
                || current_tokens + message_tokens > max_batch_tokens)
        {
            batches.push(current);
            current = Vec::new();
            current_tokens = 0;
        }
        current_tokens += message_tokens;
        current.push(message);
    }

    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

fn estimate_analysis_messages_tokens(messages: &[AnalysisMessage]) -> usize {
    messages.iter().map(estimate_analysis_message_tokens).sum()
}

fn estimate_analysis_message_tokens(message: &AnalysisMessage) -> usize {
    estimate_text_tokens(&message.id)
        + estimate_text_tokens(&message.profile_id)
        + estimate_text_tokens(&message.platform)
        + estimate_text_tokens(&message.chat_id)
        + estimate_text_tokens(&message.chat_name)
        + estimate_text_tokens(&message.sender_id)
        + estimate_text_tokens(&message.sender_name)
        + estimate_text_tokens(&message.time_text)
        + estimate_text_tokens(&message.msg_type)
        + estimate_text_tokens(&message.content)
        + 32
}

fn estimate_text_tokens(value: &str) -> usize {
    let mut tokens = 0usize;
    let mut ascii_run = 0usize;
    for ch in value.chars() {
        if ch.is_ascii() {
            ascii_run += 1;
        } else {
            if ascii_run > 0 {
                tokens += ascii_run.div_ceil(4);
                ascii_run = 0;
            }
            tokens += 1;
        }
    }
    if ascii_run > 0 {
        tokens += ascii_run.div_ceil(4);
    }
    tokens
}

fn filter_analysis_messages(messages: Vec<AnalysisMessage>) -> Vec<AnalysisMessage> {
    let mut chat_index = HashMap::<String, usize>::new();
    let mut chat_groups = Vec::<Vec<AnalysisMessage>>::new();
    for message in messages {
        if let Some(index) = chat_index.get(&message.chat_id).copied() {
            chat_groups[index].push(message);
            continue;
        }
        chat_index.insert(message.chat_id.clone(), chat_groups.len());
        chat_groups.push(vec![message]);
    }

    chat_groups
        .into_iter()
        .filter(|group| group_has_analysis_signal(group))
        .flatten()
        .collect()
}

fn group_has_analysis_signal(messages: &[AnalysisMessage]) -> bool {
    let has_inbound = messages.iter().any(|message| !message.is_me);
    let has_self_completion = messages
        .iter()
        .any(|message| message.is_me && contains_any(&message.content, ANALYSIS_COMPLETION_TERMS));

    messages.iter().any(|message| {
        let content = message.content.trim();
        if should_skip_ai_message(&message) {
            return false;
        }
        let lower = content.to_ascii_lowercase();
        if contains_any(content, ANALYSIS_RISK_TERMS)
            || contains_any(content, ANALYSIS_TASK_TERMS)
            || contains_any(content, ANALYSIS_URGENCY_TERMS)
        {
            return true;
        }
        if message.is_me && contains_any(content, ANALYSIS_COMPLETION_TERMS) {
            return true;
        }
        if !has_inbound && !has_self_completion {
            return false;
        }
        if !message.is_group && !message.is_me && looks_like_reply_request(content) {
            return true;
        }
        message.is_group
            && (content.contains('@') || lower.contains("at我") || lower.contains("@me"))
    })
}

fn looks_like_reply_request(content: &str) -> bool {
    content.contains('?')
        || content.contains('？')
        || contains_any(content, ANALYSIS_REPLY_TERMS)
        || content.ends_with("吗")
        || content.ends_with("呢")
}

fn should_skip_ai_message(message: &AnalysisMessage) -> bool {
    let content = message.content.trim();
    content.is_empty()
        || should_skip_nonsemantic_message(content)
        || !is_allowed_ai_message_type(&message.msg_type, content)
        || contains_disallowed_media_content(content)
        || contains_unsupported_client_notice(content)
        || is_call_record_message(message)
}

pub(crate) fn clean_message_content_for_ai(msg_type: &str, content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty()
        || should_skip_nonsemantic_message(trimmed)
        || !is_allowed_ai_message_type(msg_type, trimmed)
        || contains_disallowed_media_content(trimmed)
        || contains_unsupported_client_notice(trimmed)
        || is_call_record_text(msg_type, trimmed)
    {
        return None;
    }

    let text = if is_link_or_app_message_type(msg_type) || looks_like_link_or_app_payload(trimmed) {
        clean_link_or_app_message_text(trimmed)
    } else {
        clean_plain_message_text(trimmed)
    };
    let text = text.trim().to_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

pub(crate) fn is_disallowed_dashboard_topic(title: &str, summary: &str) -> bool {
    let text = format!("{}\n{}", title.trim(), summary.trim());
    is_call_record_text("text", &text) || contains_disallowed_media_content(&text)
}

pub(crate) fn is_disallowed_dashboard_keyword(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    normalized.is_empty()
        || is_call_record_text("text", text)
        || contains_disallowed_media_content(text)
        || looks_like_noise_keyword(&normalized)
        || is_local_stopword(&normalized)
        || matches!(
            normalized.as_str(),
            "dmg"
                | "pkg"
                | "exe"
                | "msi"
                | "zip"
                | "rar"
                | "7z"
                | "aarch64"
                | "x86_64"
                | "arm64"
                | "x64"
                | "amd64"
        )
}

fn is_allowed_ai_message_type(msg_type: &str, content: &str) -> bool {
    if is_excluded_ai_message_type(msg_type) {
        return false;
    }
    let kind = normalized_message_type(msg_type);
    matches!(
        kind.as_str(),
        "" | "1"
            | "text"
            | "location"
            | "48"
            | "link"
            | "url"
            | "app"
            | "appmsg"
            | "application"
            | "miniprogram"
            | "miniapp"
            | "news"
            | "card"
            | "49"
            | "system"
            | "sys"
            | "notice"
            | "notification"
            | "10000"
            | "10002"
    ) || (kind.is_empty() && !contains_disallowed_media_content(content))
}

fn is_excluded_ai_message_type(msg_type: &str) -> bool {
    let kind = normalized_message_type(msg_type);
    kind.contains("image")
        || kind.contains("图片")
        || kind == "3"
        || kind.contains("voice")
        || kind.contains("语音")
        || kind == "34"
        || kind.contains("audio")
        || kind.contains("video")
        || kind.contains("视频")
        || kind == "43"
        || kind == "62"
        || kind.contains("emoji")
        || kind.contains("表情")
        || kind == "47"
        || kind.contains("file")
        || kind.contains("文件")
        || kind.contains("call")
        || kind.contains("voip")
        || kind.contains("通话")
}

fn is_link_or_app_message_type(msg_type: &str) -> bool {
    let kind = normalized_message_type(msg_type);
    matches!(
        kind.as_str(),
        "link"
            | "url"
            | "app"
            | "appmsg"
            | "application"
            | "miniprogram"
            | "miniapp"
            | "news"
            | "card"
            | "49"
    )
}

fn normalized_message_type(msg_type: &str) -> String {
    msg_type.trim().to_ascii_lowercase()
}

fn looks_like_link_or_app_payload(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("<appmsg")
        || lower.contains("<title>")
        || lower.contains("\"title\"")
        || lower.contains("'title'")
        || lower.contains("description")
}

fn clean_link_or_app_message_text(content: &str) -> String {
    let mut texts = Vec::<String>::new();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
        collect_link_app_strings(&value, None, &mut texts);
    }
    for tag in ["title", "des", "desc", "description", "digest", "summary"] {
        texts.extend(extract_xml_tag_values(content, tag));
    }
    if texts.is_empty() {
        texts.extend(extract_labeled_message_content(content));
    }
    if texts.is_empty() {
        texts.push(strip_angle_bracket_markup(content));
    }

    texts
        .into_iter()
        .map(|text| sanitize_readable_message_text(&text))
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_readable_message_text(content: &str) -> String {
    let without_xml = strip_angle_bracket_markup(content);
    let without_urls = strip_urls(&without_xml);
    let without_ips = strip_ip_addresses(&without_urls);
    let without_mentions = strip_mentions_and_reply_quotes(&without_ips);
    strip_structural_field_labels(&without_mentions)
}

fn clean_plain_message_text(content: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
        let mut texts = Vec::new();
        collect_message_content_strings(&value, None, &mut texts);
        let cleaned = texts
            .into_iter()
            .map(|text| sanitize_readable_message_text(&text))
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>();
        if !cleaned.is_empty() {
            return cleaned.join(" ");
        }
    }

    let labeled_texts = extract_labeled_message_content(content);
    if !labeled_texts.is_empty() {
        let cleaned = labeled_texts
            .into_iter()
            .map(|text| sanitize_readable_message_text(&text))
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>();
        if !cleaned.is_empty() {
            return cleaned.join(" ");
        }
    }

    sanitize_readable_message_text(content)
}

fn should_skip_nonsemantic_message(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("<sysmsg")
        || lower.contains("<revokemsg")
        || lower.contains("今日已签到")
        || lower.contains("连续签到")
        || lower.contains("积分商城")
        || lower.contains("点击领取")
        || lower.contains("点击进入")
        || lower.contains("点击查看您的答题记录")
}

fn is_call_record_message(message: &AnalysisMessage) -> bool {
    is_call_record_text(&message.msg_type, &message.content)
}

fn is_call_record_text(msg_type: &str, content: &str) -> bool {
    let msg_type = normalized_message_type(msg_type);
    if msg_type.contains("call") || msg_type.contains("voip") || msg_type.contains("通话") {
        return true;
    }

    let content = content.trim();
    if content.is_empty() {
        return false;
    }
    let lower = content.to_ascii_lowercase();
    if lower.contains("voip") || lower.contains("voice call") || lower.contains("video call") {
        return true;
    }
    if content.contains("通话时长") || content.contains("通话记录") {
        return true;
    }
    let mentions_call = content.contains("语音通话")
        || content.contains("视频通话")
        || content.contains("[通话]")
        || content.contains("【通话】");
    mentions_call
        && contains_any(
            content,
            &[
                "已取消",
                "已拒绝",
                "未接通",
                "已结束",
                "通话结束",
                "通话时长",
                "对方无应答",
                "无人接听",
            ],
        )
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

pub(crate) fn persist_local_keyword_stats(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<usize> {
    let keywords = local_keywords(conn, day, profile_id)?;
    upsert_stat(conn, day, profile_id, "keywords", &keywords)?;
    Ok(keywords.len())
}

fn persist_analysis_across_profiles(
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
        let id =
            resolve_action_item_id_for_chat(&tx, day, &source.profile_id, &source.chat_id, &item)?;
        let source_message_ids = merge_source_message_ids(&tx, &id, &item.source_message_ids)?;
        let evidence_summary =
            merge_evidence_summary(&tx, &id, &truncate_text(item.evidence_summary.clone(), 500))?;
        let title = truncate_text(item.title.clone(), 80);
        let description = truncate_text(item.description.clone(), 500);
        let suggested_reply = item
            .suggested_reply
            .clone()
            .map(|value| truncate_text(value, 500));
        let status = normalize_action_status(item.status.as_deref());
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
struct ActionItemSource {
    profile_id: String,
    platform: String,
    chat_id: String,
    chat_name: String,
    is_group: bool,
    timestamp: i64,
}

fn analysis_message_sources(messages: &[AnalysisMessage]) -> HashMap<String, ActionItemSource> {
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

fn resolve_action_item_source(
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
                .map(|(_, _, is_group)| is_group)
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
        if let Some((platform, chat_name, is_group)) =
            chat_meta(conn, day, profile_id, &item.chat_id)?
        {
            return Ok(Some(ActionItemSource {
                profile_id: profile_id.to_owned(),
                platform,
                chat_id: item.chat_id.clone(),
                chat_name,
                is_group,
                timestamp: item
                    .source_message_ids
                    .iter()
                    .filter_map(|id| message_sources.get(id).map(|source| source.timestamp))
                    .min()
                    .unwrap_or_default(),
            }));
        }
    }

    for message_id in &item.source_message_ids {
        if let Some(source) = message_sources.get(message_id) {
            return Ok(Some(source.clone()));
        }
    }

    conn.query_row(
        "select profile_id, platform, chat_id, chat_name, is_group
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
                timestamp: 0,
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
) -> anyhow::Result<Option<(String, String, bool)>> {
    conn.query_row(
        "select platform, chat_name, is_group from daily_messages
         where day = ?1 and profile_id = ?2 and chat_id = ?3
         order by timestamp desc limit 1",
        params![day, profile_id, chat_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? == 1)),
    )
    .optional()
    .map_err(Into::into)
}

fn load_action_items_for_context(
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

fn resolve_action_item_id_for_chat(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    chat_id: &str,
    item: &AiActionItem,
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
        serde_json::to_string(&item.source_message_ids)?
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

fn upsert_stat(
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

#[derive(Debug, Default)]
struct LocalKeywordScore {
    score: f64,
    occurrences: i64,
    chat_ids: HashSet<String>,
    context_hits: i64,
}

#[derive(Debug, Clone)]
struct LocalKeywordMessage {
    chat_id: String,
    content: String,
}

#[derive(Debug, Clone)]
struct LocalKeywordCandidate {
    text: String,
    source: LocalKeywordSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalKeywordSource {
    Segment,
    Search,
    Phrase,
    Fallback,
}

impl LocalKeywordSource {
    fn rank(self) -> u8 {
        match self {
            Self::Fallback => 0,
            Self::Search => 1,
            Self::Segment => 2,
            Self::Phrase => 3,
        }
    }

    fn multiplier(self) -> f64 {
        match self {
            Self::Phrase => 1.25,
            Self::Segment => 1.0,
            Self::Search => 0.9,
            Self::Fallback => 0.55,
        }
    }
}

#[derive(Debug)]
struct LocalKeywordRank {
    text: String,
    value: LocalKeywordScore,
    final_score: f64,
}

#[derive(Debug, Default)]
struct SummaryCandidateDraft {
    score: f64,
    count: i64,
    source_chats: HashSet<SummarySourceChat>,
    source_message_ids: HashSet<String>,
    snippets: Vec<String>,
    keywords: HashSet<String>,
    risk: bool,
}

fn summary_candidates_from_messages(messages: Vec<AnalysisMessage>) -> Vec<SummaryCandidate> {
    let (jieba, context_terms) = build_local_keyword_segmenter();
    let mut drafts = HashMap::<String, SummaryCandidateDraft>::new();

    for message in messages {
        let content = message.content.trim();
        if should_skip_ai_message(&message) {
            continue;
        }
        let cleaned_texts = keyword_texts_from_message_content(content);
        if cleaned_texts.is_empty() {
            continue;
        }
        let source_chat = SummarySourceChat {
            chat_name: message.chat_name.clone(),
            is_group: message.is_group,
        };
        let risk = cleaned_texts
            .iter()
            .any(|text| contains_any(text, ANALYSIS_RISK_TERMS));
        let mut seen = HashSet::<String>::new();
        for text in cleaned_texts {
            let snippet_text = truncate_text(text.clone(), 80);
            let mut candidates = local_keyword_candidates(&jieba, &text, &context_terms)
                .into_iter()
                .map(|candidate| candidate.text)
                .filter(|candidate| keyword_char_count(candidate) >= 2)
                .collect::<Vec<_>>();
            candidates.sort_by(|left, right| {
                right
                    .chars()
                    .count()
                    .cmp(&left.chars().count())
                    .then_with(|| left.cmp(right))
            });
            candidates.truncate(5);
            if candidates.is_empty()
                && (risk
                    || contains_any(&text, ANALYSIS_TASK_TERMS)
                    || contains_any(&text, ANALYSIS_URGENCY_TERMS))
            {
                candidates.push(truncate_text(text.clone(), 16));
            }

            for keyword in candidates {
                if !seen.insert(keyword.clone()) {
                    continue;
                }
                // 热门话题先按“聊天 + 关键词”成候选，避免“买东西/采购/确认”等泛词在本地预筛阶段跨聊天误合并。
                let candidate_key =
                    format!("{}:{}|{}", message.profile_id, message.chat_id, keyword);
                let draft = drafts.entry(candidate_key).or_default();
                draft.count += 1;
                draft.score += if risk { 2.3 } else { 1.0 };
                draft.score += if source_chat.is_group { 0.25 } else { 0.0 };
                draft.source_chats.insert(source_chat.clone());
                draft.source_message_ids.insert(message.id.clone());
                draft.keywords.insert(keyword);
                draft.risk |= risk;
                if draft.snippets.len() < MAX_SUMMARY_SNIPPETS_PER_CANDIDATE {
                    draft.snippets.push(format!(
                        "{}: {}",
                        truncate_text(message.sender_name.clone(), 18),
                        snippet_text
                    ));
                }
            }
        }
    }

    let mut ranked = drafts
        .into_iter()
        .filter(|(key, draft)| {
            draft.count >= 2
                || draft.risk
                || draft.source_chats.len() >= 2
                || summary_candidate_keyword(key)
                    .map(keyword_char_count)
                    .unwrap_or_default()
                    >= 4
        })
        .map(|(key, mut draft)| {
            draft.score += draft.source_chats.len().saturating_sub(1) as f64 * 0.8;
            (
                summary_candidate_keyword(&key).unwrap_or(&key).to_owned(),
                draft,
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .score
            .partial_cmp(&left.1.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.1.count.cmp(&left.1.count))
            .then_with(|| left.0.cmp(&right.0))
    });

    ranked
        .into_iter()
        .take(MAX_SUMMARY_CANDIDATES)
        .enumerate()
        .map(|(index, (keyword, draft))| {
            let mut source_chats = draft.source_chats.into_iter().collect::<Vec<_>>();
            source_chats.sort_by(|left, right| left.chat_name.cmp(&right.chat_name));
            source_chats.truncate(5);
            let mut source_message_ids = draft.source_message_ids.into_iter().collect::<Vec<_>>();
            source_message_ids.sort();
            let mut keywords = draft.keywords.into_iter().collect::<Vec<_>>();
            keywords.sort();
            keywords.truncate(4);
            SummaryCandidate {
                id: format!("topic_candidate_{}", index + 1),
                title_hint: keyword,
                keywords,
                count: draft.count,
                source_chats,
                source_message_ids,
                snippets: draft.snippets,
                risk: draft.risk,
            }
        })
        .collect()
}

fn summary_candidate_keyword(key: &str) -> Option<&str> {
    key.split_once('|').map(|(_, keyword)| keyword)
}

fn local_keywords(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let messages = load_local_keyword_messages(conn, day, profile_id)?;
    let (jieba, context_terms) = build_local_keyword_segmenter();

    let mut scores = HashMap::<String, LocalKeywordScore>::new();
    for message in &messages {
        let mut seen = HashMap::<String, LocalKeywordSource>::new();
        for content in keyword_texts_from_message_content(&message.content) {
            for candidate in local_keyword_candidates(&jieba, &content, &context_terms) {
                seen.entry(candidate.text)
                    .and_modify(|source| {
                        if candidate.source.rank() > source.rank() {
                            *source = candidate.source;
                        }
                    })
                    .or_insert(candidate.source);
            }
        }

        for (candidate, source) in seen {
            let is_context = context_terms.contains(&candidate);
            let entry = scores.entry(candidate.clone()).or_default();
            entry.occurrences += 1;
            entry.chat_ids.insert(message.chat_id.clone());
            if is_context {
                entry.context_hits += 1;
            }
            entry.score += local_keyword_score(&candidate, source, is_context);
        }
    }

    let mut items = scores
        .into_iter()
        .filter(|(text, value)| is_selectable_local_keyword(text, value))
        .map(|(text, value)| {
            let final_score = final_local_keyword_score(&value);
            LocalKeywordRank {
                text,
                value,
                final_score,
            }
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .final_score
            .partial_cmp(&left.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| keyword_char_count(&right.text).cmp(&keyword_char_count(&left.text)))
            .then_with(|| left.text.cmp(&right.text))
    });

    let selected = select_local_keyword_ranks(items);

    Ok(selected
        .into_iter()
        .map(|item| {
            let weight = item.final_score.round().max(item.value.occurrences as f64) as i64;
            serde_json::json!({
                "text": item.text,
                "weight": weight.max(1),
                "count": item.value.occurrences
            })
        })
        .collect())
}

fn select_local_keyword_ranks(items: Vec<LocalKeywordRank>) -> Vec<LocalKeywordRank> {
    let mut selected = Vec::<LocalKeywordRank>::new();
    for item in items {
        let overlapping = selected
            .iter()
            .enumerate()
            .filter_map(|(index, selected_item)| {
                keywords_have_redundant_overlap(&item.text, &selected_item.text).then_some(index)
            })
            .collect::<Vec<_>>();

        if overlapping.is_empty() {
            if selected.len() < MAX_LOCAL_KEYWORDS {
                selected.push(item);
            }
            continue;
        }

        let item_is_context = item.value.context_hits > 0;
        if !item_is_context
            && overlapping
                .iter()
                .any(|index| selected[*index].value.context_hits > 0)
        {
            continue;
        }
        if item_is_context {
            for index in overlapping
                .iter()
                .copied()
                .filter(|index| selected[*index].value.context_hits == 0)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                selected.remove(index);
            }
            if selected.iter().all(|selected_item| {
                !keywords_have_redundant_overlap(&item.text, &selected_item.text)
            }) && selected.len() < MAX_LOCAL_KEYWORDS
            {
                selected.push(item);
            }
            continue;
        }

        if overlapping.iter().any(|index| {
            keyword_char_count(&selected[*index].text) >= keyword_char_count(&item.text)
        }) {
            continue;
        }

        for index in overlapping.into_iter().rev() {
            selected.remove(index);
        }
        if selected.len() < MAX_LOCAL_KEYWORDS {
            selected.push(item);
        }
    }
    selected
}

fn load_local_keyword_messages(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<LocalKeywordMessage>> {
    let mut stmt = conn.prepare(
        "select chat_id, msg_type, content from daily_messages
         where day = ?1 and profile_id = ?2 and trim(content) <> ''
         order by timestamp desc
         limit ?3",
    )?;
    let rows = stmt.query_map(
        params![day, profile_id, LOCAL_KEYWORD_MESSAGE_LIMIT as i64],
        |row| {
            let msg_type: String = row.get(1)?;
            let raw_content: String = row.get(2)?;
            let Some(content) = clean_message_content_for_ai(&msg_type, &raw_content) else {
                return Ok(None);
            };
            Ok(Some(LocalKeywordMessage {
                chat_id: row.get(0)?,
                content,
            }))
        },
    )?;

    let mut messages = Vec::new();
    for row in rows {
        if let Some(message) = row? {
            messages.push(message);
        }
    }
    Ok(messages)
}

fn build_local_keyword_segmenter() -> (Jieba, HashSet<String>) {
    let mut jieba = Jieba::new();
    let mut context_terms = HashSet::new();

    for term in LOCAL_KEYWORD_SEED_TERMS {
        add_context_keyword(&mut jieba, &mut context_terms, term);
    }

    (jieba, context_terms)
}

const LOCAL_KEYWORD_SEED_TERMS: &[&str] = &[
    "IM-Board",
    "关键词词云",
    "今天",
    "待我回复",
    "待办事项",
    "热门讨论话题",
    "企业微信",
    "飞书",
    "钉钉",
    "微信",
    "退货衣服",
    "打包软件",
    "配置页面",
    "通讯录",
    "聊天记录",
    "上下文",
    "同步",
    "词云",
    "分词",
    "热更新",
    "绑定账号",
    "外部单聊",
    "无效关键词",
    "本地词典",
    "jieba-rs",
];

const ANALYSIS_REPLY_TERMS: &[&str] = &[
    "确认",
    "回复",
    "看看",
    "可以吗",
    "行吗",
    "是否",
    "是不是",
    "有没有",
    "怎么",
    "什么时候",
    "哪天",
    "多少",
    "能不能",
    "要不要",
    "需要吗",
];

const ANALYSIS_TASK_TERMS: &[&str] = &[
    "帮我",
    "麻烦",
    "处理",
    "跟进",
    "安排",
    "提交",
    "发送",
    "发一下",
    "确认",
    "付款",
    "转账",
    "开票",
    "预约",
    "交付",
    "上线",
    "修复",
    "更新",
    "同步",
    "配置",
    "对接",
    "联系",
    "准备",
];

const ANALYSIS_URGENCY_TERMS: &[&str] = &[
    "尽快", "马上", "今天", "明天", "今晚", "月底", "截止", "催", "加急", "紧急", "急", "优先",
];

const ANALYSIS_COMPLETION_TERMS: &[&str] = &[
    "好的",
    "收到",
    "已",
    "已经",
    "发了",
    "发你",
    "处理了",
    "弄好了",
    "完成",
    "搞定",
    "安排了",
    "提交了",
    "确认了",
];

const ANALYSIS_RISK_TERMS: &[&str] = &[
    "投诉", "生气", "不满", "抱怨", "质疑", "失望", "争议", "冲突", "吵", "推诿", "扯皮", "催促",
    "升级", "退款", "退货", "赔偿", "质量", "交付", "价格", "服务", "风险",
];

fn contains_any(value: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| value.contains(term))
}

fn add_context_keyword(jieba: &mut Jieba, context_terms: &mut HashSet<String>, value: &str) {
    for term in context_term_candidates(value) {
        if context_terms.insert(term.clone()) {
            jieba.add_word(&term, Some(LOCAL_CONTEXT_WORD_FREQ), Some("n"));
        }
    }
}

fn context_term_candidates(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    push_context_term(value, &mut terms);

    let mut buffer = String::new();
    for ch in value.chars() {
        if is_context_separator(ch) {
            push_context_term(&buffer, &mut terms);
            buffer.clear();
        } else {
            buffer.push(ch);
        }
    }
    push_context_term(&buffer, &mut terms);
    dedupe_strings(terms)
}

fn push_context_term(value: &str, terms: &mut Vec<String>) {
    let Some(token) = normalize_keyword_candidate(value) else {
        return;
    };
    let chars = keyword_char_count(&token);
    if !(2..=24).contains(&chars) || looks_like_noise_keyword(&token) || is_local_stopword(&token) {
        return;
    }
    terms.push(token);
}

fn is_context_separator(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '/' | '\\'
                | '|'
                | ','
                | '，'
                | '、'
                | ';'
                | '；'
                | ':'
                | '：'
                | '·'
                | '-'
                | '_'
                | '('
                | ')'
                | '（'
                | '）'
                | '['
                | ']'
                | '【'
                | '】'
                | '{'
                | '}'
                | '《'
                | '》'
                | '<'
                | '>'
        )
}

fn keyword_texts_from_message_content(content: &str) -> Vec<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if should_skip_keyword_message(trimmed) {
        return Vec::new();
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let mut texts = Vec::new();
        collect_message_content_strings(&value, None, &mut texts);
        let texts = texts
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !texts.is_empty() {
            return texts
                .into_iter()
                .flat_map(|text| sanitize_keyword_text(&text))
                .collect();
        }
    }

    let labeled_texts = extract_labeled_message_content(trimmed);
    if !labeled_texts.is_empty() {
        return labeled_texts
            .into_iter()
            .flat_map(|text| sanitize_keyword_text(&text))
            .collect();
    }

    sanitize_keyword_text(trimmed)
}

fn should_skip_keyword_message(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let trimmed = content.trim();
    lower.contains("<sysmsg")
        || lower.contains("<revokemsg")
        || lower.contains("<appmsg")
        || lower.contains("<title>")
        || lower.contains("<videomsg")
        || lower.contains("<img ")
        || lower.contains("<emoji")
        || lower.contains("今日已签到")
        || lower.contains("连续签到")
        || lower.contains("积分商城")
        || lower.contains("点击领取")
        || lower.contains("点击进入")
        || lower.contains("点击查看您的答题记录")
        || contains_media_placeholder(trimmed)
        || contains_unsupported_client_notice(trimmed)
}

fn contains_media_placeholder(content: &str) -> bool {
    let compact = content
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '[' | ']' | '【' | '】' | ':' | '：'))
        .collect::<String>();
    matches!(
        compact.as_str(),
        "图片"
            | "图片分享"
            | "分享图片"
            | "已分享图片"
            | "视频"
            | "视频分享"
            | "语音"
            | "文件"
            | "文件分享"
            | "链接"
            | "链接分享"
    )
}

fn contains_disallowed_media_content(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("<videomsg")
        || lower.contains("<img ")
        || lower.contains("<emoji")
        || lower.contains("<voip")
        || lower.contains("<appattach")
        || lower.contains("<recorditem")
        || contains_media_placeholder(content)
        || content.contains("[图片]")
        || content.contains("【图片】")
        || content.contains("[语音]")
        || content.contains("【语音】")
        || content.contains("[表情]")
        || content.contains("【表情】")
        || content.contains("[文件]")
        || content.contains("【文件】")
        || content.contains("[视频]")
        || content.contains("【视频】")
        || content.contains("[音视频通话]")
        || content.contains("【音视频通话】")
        || content.contains("[语音通话]")
        || content.contains("【语音通话】")
        || content.contains("[视频通话]")
        || content.contains("【视频通话】")
}

fn contains_unsupported_client_notice(content: &str) -> bool {
    let compact = content
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    (compact.contains("微信版本不支持") || compact.contains("当前版本不支持"))
        && (compact.contains("展示内容")
            || compact.contains("显示内容")
            || compact.contains("查看")
            || compact.contains("升级"))
}

fn sanitize_keyword_text(content: &str) -> Vec<String> {
    if should_skip_keyword_message(content) {
        return Vec::new();
    }

    let text = sanitize_readable_message_text(content);
    let text = text.trim();
    if text.is_empty() || should_skip_keyword_message(text) {
        Vec::new()
    } else {
        vec![text.to_owned()]
    }
}

fn strip_angle_bracket_markup(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_tag = false;
    for ch in content.chars() {
        match ch {
            '<' => {
                in_tag = true;
                result.push(' ');
            }
            '>' if in_tag => {
                in_tag = false;
                result.push(' ');
            }
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

fn strip_urls(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut index = 0usize;
    while index < content.len() {
        let rest = &content[index..];
        if starts_with_url_like_prefix(rest) {
            index += rest
                .char_indices()
                .find_map(|(offset, ch)| is_url_boundary(ch).then_some(offset))
                .unwrap_or(rest.len());
            result.push(' ');
            continue;
        }
        let Some(ch) = rest.chars().next() else {
            break;
        };
        result.push(ch);
        index += ch.len_utf8();
    }
    result
}

fn starts_with_url_like_prefix(value: &str) -> bool {
    if value.starts_with("http://") || value.starts_with("https://") || value.starts_with("www.") {
        return true;
    }
    let Some((scheme, _)) = value.split_once("://") else {
        return false;
    };
    (2..=24).contains(&scheme.len())
        && scheme
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn strip_ip_addresses(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut index = 0usize;
    while index < content.len() {
        let rest = &content[index..];
        let Some(ch) = rest.chars().next() else {
            break;
        };
        if ch.is_ascii_digit() {
            let candidate_len = rest
                .char_indices()
                .find_map(|(offset, ch)| (!ch.is_ascii_digit() && ch != '.').then_some(offset))
                .unwrap_or(rest.len());
            let candidate = &rest[..candidate_len];
            if looks_like_ip_address(candidate) {
                result.push(' ');
                index += candidate_len;
                continue;
            }
        }
        result.push(ch);
        index += ch.len_utf8();
    }
    result
}

fn looks_like_ip_address(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.chars().all(|ch| ch.is_ascii_digit())
                && part.parse::<u8>().is_ok()
        })
}

fn is_url_boundary(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '，' | '。' | '；' | '、' | '）' | ')' | ']' | '】' | '"' | '\''
        )
}

fn strip_mentions_and_reply_quotes(content: &str) -> String {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('@') {
                return None;
            }
            if let Some(index) = trimmed.find("↳ 回复") {
                return Some(trimmed[..index].trim().to_owned());
            }
            Some(trimmed.to_owned())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, Copy)]
struct StructuralLabel {
    key_start: usize,
    value_start: usize,
    is_message_content: bool,
}

fn collect_message_content_strings(
    value: &serde_json::Value,
    parent_key: Option<&str>,
    texts: &mut Vec<String>,
) {
    match value {
        serde_json::Value::String(text) => {
            if parent_key.is_some_and(is_message_content_key) {
                texts.push(text.to_owned());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_message_content_strings(item, parent_key, texts);
            }
        }
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                if is_metadata_content_key(key) {
                    continue;
                }
                collect_message_content_strings(child, Some(key), texts);
            }
        }
        _ => {}
    }
}

fn collect_link_app_strings(
    value: &serde_json::Value,
    parent_key: Option<&str>,
    texts: &mut Vec<String>,
) {
    match value {
        serde_json::Value::String(text) => {
            if parent_key.is_some_and(is_link_app_text_key) && !looks_like_url_or_ip(text) {
                texts.push(text.to_owned());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_link_app_strings(item, parent_key, texts);
            }
        }
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                if is_link_app_metadata_key(key) {
                    continue;
                }
                collect_link_app_strings(child, Some(key), texts);
            }
        }
        _ => {}
    }
}

fn is_link_app_text_key(key: &str) -> bool {
    let key = normalize_structural_key(key);
    matches!(
        key.as_str(),
        "title" | "des" | "desc" | "description" | "digest" | "summary"
    )
}

fn is_link_app_metadata_key(key: &str) -> bool {
    let key = normalize_structural_key(key);
    matches!(
        key.as_str(),
        "url"
            | "link"
            | "href"
            | "host"
            | "ip"
            | "appid"
            | "appname"
            | "thumburl"
            | "coverurl"
            | "imageurl"
            | "iconurl"
            | "sourceurl"
            | "pagepath"
            | "username"
    )
}

fn looks_like_url_or_ip(value: &str) -> bool {
    let trimmed = value.trim();
    starts_with_url_like_prefix(trimmed) || looks_like_ip_address(trimmed)
}

fn extract_xml_tag_values(content: &str, tag: &str) -> Vec<String> {
    let lower = content.to_ascii_lowercase();
    let open = format!("<{}>", tag.to_ascii_lowercase());
    let close = format!("</{}>", tag.to_ascii_lowercase());
    let mut values = Vec::new();
    let mut search_start = 0usize;
    while let Some(open_offset) = lower[search_start..].find(&open) {
        let value_start = search_start + open_offset + open.len();
        let Some(close_offset) = lower[value_start..].find(&close) else {
            break;
        };
        let value_end = value_start + close_offset;
        let value = content[value_start..value_end].trim();
        if !value.is_empty() && !looks_like_url_or_ip(value) {
            values.push(value.to_owned());
        }
        search_start = value_end + close.len();
    }
    values
}

fn extract_labeled_message_content(content: &str) -> Vec<String> {
    let labels = structural_labels(content);
    let mut texts = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        if !label.is_message_content {
            continue;
        }
        let value_end = labels
            .iter()
            .skip(index + 1)
            .map(|next| next.key_start)
            .find(|next_start| *next_start > label.value_start)
            .unwrap_or(content.len());
        let text = trim_structural_value(&content[label.value_start..value_end]);
        if !text.is_empty() {
            texts.push(text);
        }
    }
    texts
}

fn structural_labels(content: &str) -> Vec<StructuralLabel> {
    let mut labels = Vec::new();
    for (separator_index, separator) in content.char_indices() {
        if !matches!(separator, ':' | '：') {
            continue;
        }
        let Some((key_start, key)) = structural_key_before(content, separator_index) else {
            continue;
        };
        let is_message_content = is_message_content_key(key);
        if !is_message_content && !is_metadata_content_key(key) {
            continue;
        }
        labels.push(StructuralLabel {
            key_start,
            value_start: separator_index + separator.len_utf8(),
            is_message_content,
        });
    }
    labels
}

fn structural_key_before(content: &str, separator_index: usize) -> Option<(usize, &str)> {
    let mut key_end = separator_index;
    while key_end > 0 {
        let ch = content[..key_end].chars().next_back()?;
        if ch.is_whitespace() || matches!(ch, '"' | '\'' | '`') {
            key_end -= ch.len_utf8();
        } else {
            break;
        }
    }

    let mut key_start = key_end;
    for (index, ch) in content[..key_end].char_indices().rev() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
            key_start = index;
        } else {
            break;
        }
    }
    if key_start == key_end {
        return None;
    }
    Some((key_start, &content[key_start..key_end]))
}

fn trim_structural_value(value: &str) -> String {
    value
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\''
                        | '`'
                        | ','
                        | '，'
                        | ';'
                        | '；'
                        | '{'
                        | '}'
                        | '['
                        | ']'
                        | '('
                        | ')'
                        | '（'
                        | '）'
                )
        })
        .to_owned()
}

fn is_message_content_key(key: &str) -> bool {
    let key = normalize_structural_key(key);
    matches!(
        key.as_str(),
        "content" | "text" | "message" | "msg" | "body"
    )
}

fn is_metadata_content_key(key: &str) -> bool {
    let key = normalize_structural_key(key);
    matches!(
        key.as_str(),
        "chat"
            | "chatid"
            | "chatname"
            | "chatroom"
            | "group"
            | "groupid"
            | "groupname"
            | "sender"
            | "senderid"
            | "sendername"
            | "user"
            | "userid"
            | "username"
            | "nickname"
            | "displayname"
            | "profile"
            | "profileid"
            | "platform"
            | "type"
            | "msgtype"
            | "rawtype"
            | "rawjson"
            | "timestamp"
            | "time"
            | "timetext"
            | "localid"
            | "contenthash"
    )
}

fn normalize_structural_key(key: &str) -> String {
    key.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn strip_structural_field_labels(content: &str) -> String {
    content
        .split_whitespace()
        .filter(|part| {
            let key = part
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | '-'));
            !is_metadata_content_key(key) && !is_message_content_key(key)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn local_keyword_candidates(
    jieba: &Jieba,
    content: &str,
    context_terms: &HashSet<String>,
) -> Vec<LocalKeywordCandidate> {
    let mut candidates = Vec::new();
    let mut phrase_tokens = Vec::<String>::new();

    for tag in jieba.tag(content, true) {
        if let Some(token) = normalize_keyword_candidate(tag.word) {
            if should_keep_segment_token(&token, tag.tag, context_terms) {
                candidates.push(LocalKeywordCandidate {
                    text: token.clone(),
                    source: LocalKeywordSource::Segment,
                });
                phrase_tokens.push(token);
                continue;
            }
        }
        phrase_tokens.push(String::new());
    }

    candidates.extend(phrase_keyword_candidates(&phrase_tokens));

    for word in jieba.cut_for_search(content, true) {
        let Some(token) = normalize_keyword_candidate(word) else {
            continue;
        };
        if should_keep_search_token(&token, context_terms) {
            candidates.push(LocalKeywordCandidate {
                text: token,
                source: LocalKeywordSource::Search,
            });
        }
    }

    candidates.extend(
        fallback_keyword_candidates(content)
            .into_iter()
            .map(|text| LocalKeywordCandidate {
                text,
                source: LocalKeywordSource::Fallback,
            }),
    );

    dedupe_candidates(candidates)
}

fn phrase_keyword_candidates(tokens: &[String]) -> Vec<LocalKeywordCandidate> {
    let mut candidates = Vec::new();
    for size in 2..=3 {
        for window in tokens.windows(size) {
            if window.iter().any(|token| token.is_empty()) {
                continue;
            }
            let phrase = window.join("");
            let Some(token) = normalize_keyword_candidate(&phrase) else {
                continue;
            };
            let chars = keyword_char_count(&token);
            if !(3..=12).contains(&chars)
                || looks_like_noise_keyword(&token)
                || is_local_stopword(&token)
            {
                continue;
            }
            candidates.push(LocalKeywordCandidate {
                text: token,
                source: LocalKeywordSource::Phrase,
            });
        }
    }
    candidates
}

fn fallback_keyword_candidates(content: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut latin = String::new();
    let mut cjk = String::new();

    for ch in content.chars() {
        if is_keyword_latin(ch) {
            flush_cjk_candidates(&mut cjk, &mut candidates);
            latin.push(ch.to_ascii_lowercase());
        } else if is_cjk(ch) {
            flush_latin_candidate(&mut latin, &mut candidates);
            if !is_ignored_cjk_particle(ch) {
                cjk.push(ch);
            }
        } else {
            flush_latin_candidate(&mut latin, &mut candidates);
            flush_cjk_candidates(&mut cjk, &mut candidates);
        }
    }
    flush_latin_candidate(&mut latin, &mut candidates);
    flush_cjk_candidates(&mut cjk, &mut candidates);
    dedupe_strings(candidates)
}

fn flush_latin_candidate(buffer: &mut String, candidates: &mut Vec<String>) {
    let token = normalize_keyword_candidate(buffer);
    buffer.clear();
    let Some(token) = token else {
        return;
    };
    if keyword_char_count(&token) < 2
        || looks_like_noise_keyword(&token)
        || is_local_stopword(&token)
    {
        return;
    }
    candidates.push(token);
}

fn flush_cjk_candidates(buffer: &mut String, candidates: &mut Vec<String>) {
    let chars = buffer.chars().collect::<Vec<_>>();
    buffer.clear();
    if chars.len() < 2 {
        return;
    }

    if chars.len() <= 6 {
        push_cjk_candidate(chars.iter().collect::<String>(), candidates);
    }

    for size in 2..=usize::min(5, chars.len()) {
        for window in chars.windows(size) {
            push_cjk_candidate(window.iter().collect::<String>(), candidates);
        }
    }
}

fn push_cjk_candidate(value: String, candidates: &mut Vec<String>) {
    let Some(token) = normalize_cjk_candidate(&value) else {
        return;
    };
    if is_local_stopword(&token) {
        return;
    }
    candidates.push(token);
}

fn normalize_cjk_candidate(value: &str) -> Option<String> {
    let token = normalize_keyword_candidate(value)?;
    if keyword_char_count(&token) < 2 || is_local_stopword(&token) {
        return None;
    }
    if token.chars().all(is_weak_cjk_char) {
        return None;
    }
    Some(token)
}

fn normalize_keyword_candidate(value: &str) -> Option<String> {
    let mut token = value
        .trim_matches(|ch: char| !is_keyword_body_char(ch))
        .chars()
        .filter(|ch| is_keyword_body_char(*ch))
        .flat_map(char::to_lowercase)
        .collect::<String>();
    token = token.trim_matches(is_weak_keyword_edge).to_owned();

    if token.is_empty()
        || keyword_char_count(&token) < 2
        || looks_like_noise_keyword(&token)
        || is_local_stopword(&token)
        || token.chars().all(is_weak_cjk_char)
    {
        return None;
    }
    Some(token)
}

fn should_keep_segment_token(token: &str, tag: &str, context_terms: &HashSet<String>) -> bool {
    if context_terms.contains(token) {
        return true;
    }
    if looks_like_noise_keyword(token) || is_local_stopword(token) {
        return false;
    }
    if is_ascii_keyword(token) {
        return is_strong_ascii_keyword(token);
    }
    let chars = keyword_char_count(token);
    if chars < 2 || token.chars().all(is_weak_cjk_char) {
        return false;
    }
    is_informative_jieba_tag(tag) || chars >= 3
}

fn should_keep_search_token(token: &str, context_terms: &HashSet<String>) -> bool {
    if context_terms.contains(token) {
        return true;
    }
    if looks_like_noise_keyword(token) || is_local_stopword(token) {
        return false;
    }
    if is_ascii_keyword(token) {
        return is_strong_ascii_keyword(token);
    }
    keyword_char_count(token) >= 3
}

fn is_informative_jieba_tag(tag: &str) -> bool {
    tag.starts_with('n') || matches!(tag, "eng" | "vn" | "v" | "a" | "l" | "i" | "j")
}

fn local_keyword_score(token: &str, source: LocalKeywordSource, is_context: bool) -> f64 {
    let chars = keyword_char_count(token);
    let length_bonus = usize::min(chars.saturating_sub(2), 4) as f64 * 0.35;
    let ascii_bonus = if is_ascii_keyword(token) { 0.6 } else { 0.0 };
    let context_bonus = if is_context { 0.8 } else { 0.0 };
    (1.0 + length_bonus + ascii_bonus + context_bonus) * source.multiplier()
}

fn final_local_keyword_score(value: &LocalKeywordScore) -> f64 {
    let chat_bonus = 1.0 + value.chat_ids.len().saturating_sub(1).min(4) as f64 * 0.22;
    let context_bonus = 1.0 + value.context_hits.min(3) as f64 * 0.18;
    value.score * chat_bonus * context_bonus
}

fn is_selectable_local_keyword(text: &str, value: &LocalKeywordScore) -> bool {
    if keyword_char_count(text) < 2 || looks_like_noise_keyword(text) || is_local_stopword(text) {
        return false;
    }
    // 词云只展示真正反复出现的关键词，避免一次性闲聊或噪声词进入看板。
    if value.occurrences < MIN_KEYWORD_CLOUD_COUNT {
        return false;
    }
    value.context_hits > 0 || value.occurrences > 1 || keyword_char_count(text) >= 4
}

fn keywords_have_redundant_overlap(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    if keywords_have_containment_overlap(left, right) {
        return true;
    }
    longest_common_keyword_overlap(left, right) >= MIN_REDUNDANT_KEYWORD_OVERLAP
}

fn keywords_have_containment_overlap(left: &str, right: &str) -> bool {
    let left_chars = keyword_char_count(left);
    let right_chars = keyword_char_count(right);
    let shorter_chars = left_chars.min(right_chars);
    if shorter_chars < MIN_CONTAINED_KEYWORD_CHARS {
        return false;
    }
    left.contains(right) || right.contains(left)
}

fn longest_common_keyword_overlap(left: &str, right: &str) -> usize {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    if left_chars.is_empty() || right_chars.is_empty() {
        return 0;
    }

    let mut previous = vec![0usize; right_chars.len() + 1];
    let mut best = 0usize;
    for left_char in &left_chars {
        let mut current = vec![0usize; right_chars.len() + 1];
        for (right_index, right_char) in right_chars.iter().enumerate() {
            if left_char == right_char {
                current[right_index + 1] = previous[right_index] + 1;
                best = best.max(current[right_index + 1]);
            }
        }
        previous = current;
    }
    best
}

fn keyword_char_count(value: &str) -> usize {
    value.chars().count()
}

fn is_ascii_keyword(value: &str) -> bool {
    value.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn is_strong_ascii_keyword(value: &str) -> bool {
    let chars = keyword_char_count(value);
    chars >= 3 && !looks_like_noise_keyword(value) && !is_local_stopword(value)
}

fn is_keyword_latin(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '#' | '+')
}

fn is_keyword_body_char(ch: char) -> bool {
    is_keyword_latin(ch) || is_cjk(ch)
}

fn looks_like_noise_keyword(value: &str) -> bool {
    value.len() > 64
        || value.starts_with("http")
        || looks_like_domain_keyword(value)
        || looks_like_technical_payload_keyword(value)
        || value.starts_with("msg")
        || value.starts_with("local_")
        || value.starts_with("wxid")
        || value.starts_with("gh_")
        || value.starts_with("gh-")
        || value
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '_' | '.' | '#' | '+'))
}

fn looks_like_technical_payload_keyword(value: &str) -> bool {
    const TECHNICAL_FRAGMENTS: &[&str] = &[
        "darkmode",
        "selfintroducetext",
        "openedbyminiapp",
        "needredirect",
        "containertype",
        "slidepaneloption",
        "redirecturl",
        "hrmregister",
        "empprofile",
        "groupwelcome",
        "dingtalkclient",
        "openapp",
    ];
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return false;
    }
    if TECHNICAL_FRAGMENTS
        .iter()
        .any(|fragment| value.contains(fragment))
    {
        return true;
    }

    let chars = value.chars().count();
    let digit_count = value.chars().filter(|ch| ch.is_ascii_digit()).count();
    if chars >= 12 && digit_count * 2 >= chars {
        return true;
    }
    if chars >= 8
        && ["22", "26", "3a", "3f", "7b"]
            .iter()
            .any(|prefix| value.starts_with(prefix))
        && digit_count > 0
        && value.chars().any(|ch| ch.is_ascii_alphabetic())
    {
        return true;
    }
    if chars >= 10
        && value.contains("26")
        && ["cid", "false", "true", "profile", "group"]
            .iter()
            .any(|fragment| value.contains(fragment))
    {
        return true;
    }
    value.starts_with("dding") && chars >= 12 && digit_count >= 4
}

fn looks_like_domain_keyword(value: &str) -> bool {
    value.contains('.')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '/'))
        && value.split('.').filter(|part| !part.is_empty()).count() >= 2
}

fn is_ignored_cjk_particle(ch: char) -> bool {
    matches!(
        ch,
        '的' | '了' | '吗' | '呢' | '吧' | '啊' | '哦' | '呀' | '哟'
    )
}

fn is_weak_keyword_edge(ch: char) -> bool {
    matches!(
        ch,
        '我' | '你'
            | '他'
            | '她'
            | '它'
            | '们'
            | '把'
            | '被'
            | '给'
            | '在'
            | '是'
            | '有'
            | '就'
            | '都'
            | '也'
            | '还'
            | '要'
            | '能'
            | '会'
            | '去'
            | '来'
            | '请'
            | '将'
            | '和'
            | '与'
            | '及'
            | '再'
            | '又'
            | '先'
            | '后'
            | '让'
            | '用'
            | '跟'
            | '到'
            | '等'
            | '个'
            | '很'
            | '太'
            | '更'
            | '真'
    )
}

fn is_weak_cjk_char(ch: char) -> bool {
    is_weak_keyword_edge(ch)
        || matches!(
            ch,
            '可' | '以'
                | '不'
                | '没'
                | '嘛'
                | '啥'
                | '呢'
                | '啦'
                | '哈'
                | '哦'
                | '嗯'
                | '好'
                | '行'
                | '啊'
        )
}

fn is_local_stopword(value: &str) -> bool {
    const STOPWORDS: &[&str] = &[
        "http",
        "https",
        "www",
        "com",
        "cn",
        "local",
        "local_id",
        "true",
        "false",
        "null",
        "xml",
        "version",
        "type",
        "sysmsg",
        "revokemsg",
        "revoketime",
        "appmsg",
        "videomsg",
        "img",
        "emoji",
        "aeskey",
        "cdnthumburl",
        "cdnvideourl",
        "fromusername",
        "newmd5",
        "rawmd5",
        "md5",
        "appid",
        "sdkver",
        "ion",
        "rs",
        "im",
        "content",
        "body",
        "board",
        "darkmode",
        "containertype",
        "selfintroducetext",
        "openedbyminiapp",
        "needredirect",
        "slidepaneloption",
        "redirecturl",
        "dingtalkclient",
        "openapp",
        "hrmregister",
        "empprofile",
        "groupwelcome",
        "cid",
        "corpid",
        "message",
        "messages",
        "data",
        "items",
        "records",
        "chat",
        "chatid",
        "chat_id",
        "chatname",
        "chat_name",
        "chatroom",
        "group",
        "groupid",
        "group_id",
        "groupname",
        "group_name",
        "sender",
        "senderid",
        "sender_id",
        "sendername",
        "sender_name",
        "user",
        "userid",
        "user_id",
        "username",
        "user_name",
        "nickname",
        "displayname",
        "display_name",
        "profile",
        "profileid",
        "profile_id",
        "platform",
        "msgtype",
        "msg_type",
        "rawtype",
        "raw_type",
        "rawjson",
        "raw_json",
        "timestamp",
        "timetext",
        "time_text",
        "localid",
        "contenthash",
        "content_hash",
        "text",
        "image",
        "voice",
        "video",
        "emoji",
        "file",
        "audio",
        "msg",
        "url",
        "link",
        "ok",
        "okay",
        "yes",
        "no",
        "这个",
        "那个",
        "这些",
        "那些",
        "今天",
        "明天",
        "昨天",
        "现在",
        "刚刚",
        "一下",
        "等下",
        "等等",
        "然后",
        "因为",
        "所以",
        "但是",
        "如果",
        "还是",
        "就是",
        "不是",
        "没有",
        "可以",
        "不能",
        "不用",
        "不要",
        "已经",
        "觉得",
        "感觉",
        "知道",
        "看到",
        "收到",
        "回复",
        "处理",
        "消息",
        "聊天",
        "内容",
        "事情",
        "问题",
        "情况",
        "时间",
        "上午",
        "下午",
        "晚上",
        "中午",
        "好的",
        "好哟",
        "哈哈",
        "哈哈哈",
        "哈哈哈哈",
        "帮我",
        "我把",
        "你把",
        "我们",
        "你们",
        "他们",
        "她们",
        "什么",
        "怎么",
        "这样",
        "那样",
        "这里",
        "那里",
        "链接",
        "领取",
        "连续",
        "解锁",
        "连续签到",
        "连续签到解锁",
        "签到",
        "积分",
        "积分商城",
        "商城",
        "好礼",
        "邀请",
        "好友",
        "入群",
        "奖励",
        "有效期",
        "兑换",
        "点击",
        "点击领取",
        "点击进入",
        "卡值",
        "精彩",
        "礼品",
        "恭喜",
        "完成",
        "今日",
        "答题",
        "获奖",
        "答案",
        "记录",
        "直接",
        "真的",
        "可能",
        "需要",
        "进行",
        "过去",
        "回来",
        "起来",
        "出来",
        "一个",
        "两个",
        "几个",
    ];
    STOPWORDS.contains(&value)
        || value.starts_with("msg")
        || value.starts_with("wxid")
        || value.starts_with("gh_")
        || value.contains("哈哈")
}

fn dedupe_candidates(candidates: Vec<LocalKeywordCandidate>) -> Vec<LocalKeywordCandidate> {
    let mut seen = HashMap::<String, usize>::new();
    let mut deduped = Vec::<LocalKeywordCandidate>::new();
    for candidate in candidates {
        if let Some(index) = seen.get(&candidate.text).copied() {
            if candidate.source.rank() > deduped[index].source.rank() {
                deduped[index].source = candidate.source;
            }
            continue;
        }
        seen.insert(candidate.text.clone(), deduped.len());
        deduped.push(candidate);
    }
    deduped
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}

fn extract_json_object(content: &str) -> anyhow::Result<String> {
    let trimmed = content.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Ok(trimmed.to_owned());
    }
    let Some(start) = trimmed.find('{') else {
        anyhow::bail!("AI返回内容不是JSON");
    };
    let Some(end) = trimmed.rfind('}') else {
        anyhow::bail!("AI返回内容不是完整JSON");
    };
    Ok(trimmed[start..=end].to_owned())
}

fn is_self_sender(sender_id: &str, sender_name: &str) -> bool {
    let sender_id = sender_id.trim().to_ascii_lowercase();
    let sender_name = sender_name.trim().to_ascii_lowercase();
    matches!(sender_id.as_str(), "me" | "self" | "我")
        || matches!(sender_name.as_str(), "me" | "self" | "我")
}

fn normalize_priority(value: &str) -> &str {
    match value {
        "high" | "medium" | "low" => value,
        _ => "medium",
    }
}

fn normalize_action_status(value: Option<&str>) -> &str {
    match value.map(str::trim) {
        Some("done") => "done",
        _ => "open",
    }
}

fn normalized_action_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_alphanumeric() || is_cjk(*ch))
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
        || ('\u{3400}'..='\u{4dbf}').contains(&ch)
        || ('\u{f900}'..='\u{faff}').contains(&ch)
}

fn truncate_text(value: String, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn summary_topic_context_from_value(value: serde_json::Value) -> Option<SummaryTopicContext> {
    let title = value
        .get("title")
        .and_then(|inner| inner.as_str())
        .map(str::trim)
        .filter(|inner| !inner.is_empty())?
        .to_owned();
    let summary = value
        .get("summary")
        .and_then(|inner| inner.as_str())
        .unwrap_or_default()
        .trim()
        .to_owned();
    let id = value
        .get("id")
        .and_then(|inner| inner.as_str())
        .map(str::trim)
        .filter(|inner| !inner.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| summary_topic_id(&title, &summary));
    let mut source_message_ids = value
        .get("sourceMessageIds")
        .and_then(|inner| inner.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();
    source_message_ids.sort();
    let count = if source_message_ids.is_empty() {
        value
            .get("count")
            .and_then(|inner| inner.as_i64())
            .unwrap_or_default()
    } else {
        source_message_ids.len().try_into().unwrap_or_default()
    };
    let source_chats = summary_source_chats_from_value(&value);
    Some(SummaryTopicContext {
        id,
        title,
        summary,
        count,
        source_chats,
        source_message_ids,
    })
}

pub(crate) fn summary_topic_contexts_from_summary(summary: &AiSummary) -> Vec<SummaryTopicContext> {
    summary
        .topics
        .iter()
        .cloned()
        .filter_map(summary_topic_context_from_value)
        .collect()
}

fn summary_topic_context_to_value(topic: &SummaryTopicContext) -> serde_json::Value {
    serde_json::json!({
        "id": topic.id,
        "title": topic.title,
        "summary": topic.summary,
        "count": topic.count,
        "sourceChats": topic.source_chats,
        "sourceMessageIds": topic.source_message_ids,
    })
}

fn summary_source_chats_from_value(value: &serde_json::Value) -> Vec<SummarySourceChat> {
    value
        .get("sourceChats")
        .and_then(|inner| inner.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let chat_name = item
                        .get("chatName")
                        .or_else(|| item.get("chat_name"))
                        .and_then(|inner| inner.as_str())
                        .map(str::trim)
                        .filter(|inner| !inner.is_empty())?;
                    let is_group = item
                        .get("isGroup")
                        .or_else(|| item.get("is_group"))
                        .and_then(|inner| inner.as_bool())
                        .unwrap_or(false);
                    Some(SummarySourceChat {
                        chat_name: chat_name.to_owned(),
                        is_group,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn summary_topic_id(title: &str, summary: &str) -> String {
    format!("topic_{}", &hash_text(&format!("{title}|{summary}"))[..16])
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_keyword_candidates_keep_meaningful_terms() {
        let mut jieba = Jieba::new();
        let mut context_terms = HashSet::new();
        add_context_keyword(&mut jieba, &mut context_terms, "IM-Board");
        add_context_keyword(&mut jieba, &mut context_terms, "退货衣服");
        let candidates = local_keyword_candidates(
            &jieba,
            "猪猪，等下帮我把退货的衣服拿下一楼。OpenAI 和 IM-Board 都更新了。",
            &context_terms,
        );

        assert!(candidates.iter().any(|value| value.text == "退货衣服"));
        assert!(candidates.iter().any(|value| value.text == "一楼"));
        assert!(candidates.iter().any(|value| value.text == "openai"));
        assert!(candidates.iter().any(|value| value.text == "im-board"));
    }

    #[test]
    fn persist_local_keyword_stats_uses_jieba_and_local_context() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'wechat', '微信工作号', '{\"remark\":\"工作号\"}', datetime('now'), datetime('now'))",
            [],
        )
        .expect("profile");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board 研发群', 1, ?2, ?3, ?4, '09:00', 'text', ?5, ?6)",
            params![
                "msg-1",
                "u-1",
                "同事甲",
                1_i64,
                "IM-Board 词云今天接入 jieba-rs 分词，先不用 AI。",
                "hash-1"
            ],
        )
        .expect("message 1");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board 研发群', 1, ?2, ?3, ?4, '09:01', 'text', ?5, ?6)",
            params![
                "msg-2",
                "u-2",
                "同事乙",
                2_i64,
                "词云继续用 jieba-rs 本地分词，降低无效关键词。",
                "hash-2"
            ],
        )
        .expect("message 2");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board 研发群', 1, ?2, ?3, ?4, '09:02', 'text', ?5, ?6)",
            params![
                "msg-3",
                "u-3",
                "同事丙",
                3_i64,
                "词云继续验证 jieba-rs 关键词频次阈值。",
                "hash-3"
            ],
        )
        .expect("message 3");

        let count = persist_local_keyword_stats(&conn, "2026-05-01", "profile-1")
            .expect("local keyword stats");
        assert!(count > 0);
        let raw: String = conn
            .query_row(
                "select value_json from daily_stats where day = '2026-05-01' and profile_id = 'profile-1' and metric = 'keywords'",
                [],
                |row| row.get(0),
            )
            .expect("keywords");
        let values: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("keyword json");
        let texts = values
            .iter()
            .filter_map(|value| value.get("text").and_then(|value| value.as_str()))
            .collect::<Vec<_>>();
        assert!(texts.contains(&"词云"), "got {texts:?}");
        assert!(texts.contains(&"jieba-rs"), "got {texts:?}");
    }

    #[test]
    fn selectable_local_keyword_requires_three_occurrences() {
        let mut low_frequency = LocalKeywordScore {
            score: 10.0,
            occurrences: MIN_KEYWORD_CLOUD_COUNT - 1,
            chat_ids: HashSet::new(),
            context_hits: MIN_KEYWORD_CLOUD_COUNT - 1,
        };
        assert!(!is_selectable_local_keyword("词云", &low_frequency));

        low_frequency.occurrences = MIN_KEYWORD_CLOUD_COUNT;
        assert!(is_selectable_local_keyword("词云", &low_frequency));
    }

    #[test]
    fn persist_local_keyword_stats_ignores_message_metadata() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'wechat', '微信工作号', '{\"remark\":\"工作号\"}', datetime('now'), datetime('now'))",
            [],
        )
        .expect("profile");

        let structured_content = serde_json::json!({
            "chatName": "IM-Board研发群",
            "senderName": "群主小吴",
            "msgType": "text",
            "content": "词云分词只看消息正文，忽略返回结构。"
        })
        .to_string();
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board研发群', 1, 'u-1', '群主小吴', 1, '09:00', 'text', ?2, 'hash-1')",
            params!["msg-1", structured_content],
        )
        .expect("message 1");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board研发群', 1, 'u-2', '群员小李', 2, '09:01', 'text', ?2, 'hash-2')",
            params![
                "msg-2",
                "chatName: IM-Board研发群 senderName: 群员小李 content: 继续优化词云分词，不要混入群名和用户名。"
            ],
        )
        .expect("message 2");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board研发群', 1, 'u-3', '群员小周', 3, '09:02', 'text', ?2, 'hash-3')",
            params!["msg-3", "继续验证词云分词，只统计消息正文里的关键词。"],
        )
        .expect("message 3");

        persist_local_keyword_stats(&conn, "2026-05-01", "profile-1").expect("local keyword stats");
        let raw: String = conn
            .query_row(
                "select value_json from daily_stats where day = '2026-05-01' and profile_id = 'profile-1' and metric = 'keywords'",
                [],
                |row| row.get(0),
            )
            .expect("keywords");
        let values: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("keyword json");
        let texts = values
            .iter()
            .filter_map(|value| value.get("text").and_then(|value| value.as_str()))
            .collect::<Vec<_>>();

        assert!(texts.contains(&"词云"), "got {texts:?}");
        assert!(texts.contains(&"分词"), "got {texts:?}");
        for ignored in [
            "im-board",
            "研发群",
            "群主小吴",
            "小吴",
            "群员小李",
            "小李",
            "chatname",
            "sendername",
            "msgtype",
            "content",
            "微信工作号",
            "工作号",
        ] {
            assert!(
                !texts.contains(&ignored),
                "metadata keyword should be ignored: {ignored}; got {texts:?}"
            );
        }
    }

    #[test]
    fn keyword_texts_from_message_content_filters_xml_urls_and_bot_templates() {
        assert!(keyword_texts_from_message_content(
            r#"[系统] <?xml version="1.0"?><sysmsg type="revokemsg"><revokemsg><content>"Joyce" 撤回了一条消息</content><revoketime>0</revoketime></revokemsg></sysmsg>"#
        )
        .is_empty());
        assert!(keyword_texts_from_message_content(
            "@Joyce\n🕹今日已签到！\n连续签到解锁更多精彩好礼\n点击进入积分商城 https://u.isaveu.cn/ixh1o"
        )
        .is_empty());

        let texts = keyword_texts_from_message_content(
            "词云过滤 XML 元数据和链接 https://u.isaveu.cn/ixh1o，保留真正消息正文。",
        );
        assert_eq!(texts.len(), 1);
        assert!(texts[0].contains("词云过滤"));
        assert!(!texts[0].contains("u.isaveu.cn"));
    }

    #[test]
    fn keyword_texts_from_message_content_filters_dingtalk_custom_scheme_urls() {
        let texts = keyword_texts_from_message_content(
            "佛山市戴胜文化传媒有限公司\n让我们一起欢迎新人~\n群小钉\n[dingtalk://dingtalkclient/action/openapp?slide_panel_option=%7B%22width%22%3A480%2C%22hidesTitle%22%3Atrue%7D&containerType=board&dd_darkmode=false&selfIntroduceText=&openedByMiniApp=true&needRedirect=true&corpId=ding298a3a7e22a45692f2c783f7214b6d69]",
        );
        let joined = texts.join(" ");

        assert!(joined.contains("欢迎新人"), "got {joined}");
        for ignored in [
            "dingtalk",
            "containerType",
            "board",
            "dd_darkmode",
            "selfIntroduceText",
            "openedByMiniApp",
            "needRedirect",
            "corpId",
        ] {
            assert!(
                !joined
                    .to_ascii_lowercase()
                    .contains(&ignored.to_ascii_lowercase()),
                "DingTalk URL noise should be stripped: {ignored}; got {joined}"
            );
        }
    }

    #[test]
    fn dashboard_keyword_filter_rejects_dingtalk_payload_fragments() {
        for ignored in [
            "board",
            "containertype",
            "3fdd_darkmode",
            "26selfintroducetext",
            "26openedbyminiapp",
            "26needredirect",
            "dfalse26cid3",
            "7b22width22",
            "3a4802c22",
            "dding298a3a7e22a45692f2c783f7214b6d6926",
            "d0129131342192618510926",
            "d500000000485825726",
            "dgroupwelcome26",
            "d7480156731726",
            "dempprofile26",
            "fhrmregister2",
        ] {
            assert!(
                is_disallowed_dashboard_keyword(ignored),
                "DingTalk payload keyword should be ignored: {ignored}"
            );
        }

        for kept in ["im-board", "openai", "2500rmb", "确保数据安全"] {
            assert!(
                !is_disallowed_dashboard_keyword(kept),
                "meaningful keyword should stay selectable: {kept}"
            );
        }
    }

    #[test]
    fn keyword_texts_from_message_content_filters_media_and_client_notices() {
        for content in [
            "图片分享",
            "[图片]",
            "分享图片",
            "微信版本不支持展示内容，请升级至最新版本查看",
            "当前版本不支持显示内容",
        ] {
            assert!(
                keyword_texts_from_message_content(content).is_empty(),
                "placeholder or client notice should be ignored: {content}"
            );
        }

        let texts = keyword_texts_from_message_content("客户发来的装修图片需要确认预算方案");
        assert_eq!(texts, vec!["客户发来的装修图片需要确认预算方案".to_owned()]);
    }

    #[test]
    fn clean_message_content_for_ai_keeps_only_allowed_message_types() {
        assert_eq!(
            clean_message_content_for_ai("text", "客户今天需要确认报价"),
            Some("客户今天需要确认报价".to_owned())
        );
        assert_eq!(
            clean_message_content_for_ai("location", "上海市浦东新区世纪大道"),
            Some("上海市浦东新区世纪大道".to_owned())
        );
        assert_eq!(
            clean_message_content_for_ai("system", "[系统] 张三加入了群聊"),
            Some("[系统] 张三加入了群聊".to_owned())
        );

        for (msg_type, content) in [
            ("image", "客户发来的装修图片需要确认预算方案"),
            ("voice", "[语音]"),
            ("emoji", "[表情]"),
            ("file", "报价单.pdf"),
            ("video", "[视频]"),
            ("voip", "[语音通话] 通话时长 00:12"),
        ] {
            assert_eq!(
                clean_message_content_for_ai(msg_type, content),
                None,
                "{msg_type} should not be sent to AI"
            );
        }
    }

    #[test]
    fn clean_link_or_app_message_text_keeps_title_and_description_without_urls_or_ips() {
        let content = r#"{
          "title": "客户续费方案",
          "description": "需要确认 5 月报价和审批节奏",
          "url": "https://example.com/order?id=1",
          "host": "192.168.1.9"
        }"#;

        let text = clean_message_content_for_ai("appmsg", content).expect("cleaned app text");

        assert!(text.contains("客户续费方案"));
        assert!(text.contains("审批节奏"));
        assert!(!text.contains("example.com"));
        assert!(!text.contains("192.168.1.9"));
    }

    #[test]
    fn persist_local_keyword_stats_filters_structural_noise_terms() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'wechat', '微信工作号', '{}', datetime('now'), datetime('now'))",
            [],
        )
        .expect("profile");

        for (id, content, timestamp) in [
            (
                "msg-1",
                r#"[系统] <?xml version="1.0"?><sysmsg type="revokemsg"><revokemsg><content>"Joyce" 撤回了一条消息</content><revoketime>0</revoketime></revokemsg></sysmsg>"#,
                1_i64,
            ),
            (
                "msg-2",
                "@Joyce\n🕹今日已签到！\n连续签到解锁更多精彩好礼\n点击进入🛒积分商城 https://u.isaveu.cn/ixh1o",
                2_i64,
            ),
            (
                "msg-3",
                "词云过滤 XML 元数据和链接，保留真正消息正文。",
                3_i64,
            ),
            (
                "msg-4",
                "继续优化词云过滤，不要显示 version type sysmsg revoketime。",
                4_i64,
            ),
            (
                "msg-5",
                "词云过滤继续保留真正消息正文，避免结构字段混入。",
                5_i64,
            ),
            (
                "msg-6",
                "佛山市戴胜文化传媒有限公司\n让我们一起欢迎新人~\n群小钉\n[dingtalk://dingtalkclient/action/openapp?slide_panel_option=%7B%22width%22%3A480%2C%22hidesTitle%22%3Atrue%7D&containerType=board&dd_darkmode=false&selfIntroduceText=&openedByMiniApp=true&needRedirect=true&corpId=ding298a3a7e22a45692f2c783f7214b6d69]",
                6_i64,
            ),
        ] {
            conn.execute(
                "insert into daily_messages(
                   id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
                   timestamp, time_text, msg_type, content, content_hash
                 )
                 values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', '测试群', 1, 'u-1', '测试用户', ?2, '09:00', 'text', ?3, ?4)",
                params![id, timestamp, content, format!("hash-{id}")],
            )
            .expect("message");
        }

        persist_local_keyword_stats(&conn, "2026-05-01", "profile-1").expect("local keyword stats");
        let raw: String = conn
            .query_row(
                "select value_json from daily_stats where day = '2026-05-01' and profile_id = 'profile-1' and metric = 'keywords'",
                [],
                |row| row.get(0),
            )
            .expect("keywords");
        let values: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("keyword json");
        let texts = values
            .iter()
            .filter_map(|value| value.get("text").and_then(|value| value.as_str()))
            .collect::<Vec<_>>();

        assert!(texts.contains(&"词云"), "got {texts:?}");
        for ignored in [
            "xml",
            "version",
            "type",
            "sysmsg",
            "revoketime",
            "u.isaveu.cn",
            "joyce",
            "连续",
            "解锁",
            "领取",
            "链接",
            "board",
            "containertype",
            "3fdd_darkmode",
            "26selfintroducetext",
            "26openedbyminiapp",
            "26needredirect",
            "dingtalkclient",
            "hrmregister",
        ] {
            assert!(
                !texts.contains(&ignored),
                "structural keyword should be ignored: {ignored}; got {texts:?}"
            );
        }
    }

    #[test]
    fn select_local_keyword_ranks_keeps_longest_four_char_overlap() {
        let selected = select_local_keyword_ranks(vec![
            test_keyword_rank("潦草的一生", 100.0),
            test_keyword_rank("我们这潦草的一生", 1.0),
            test_keyword_rank("退货衣服", 10.0),
        ]);
        let texts = selected
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();

        assert!(texts.contains(&"我们这潦草的一生"));
        assert!(!texts.contains(&"潦草的一生"));
        assert!(texts.contains(&"退货衣服"));
    }

    #[test]
    fn select_local_keyword_ranks_collapses_contained_fragments() {
        let selected = select_local_keyword_ranks(vec![
            test_keyword_rank("飞书", 100.0),
            test_keyword_rank("李俊彦", 90.0),
            test_keyword_rank("俊彦飞", 80.0),
            test_keyword_rank("李俊彦飞书", 10.0),
            test_keyword_rank("确保数据安全", 8.0),
        ]);
        let texts = selected
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();

        assert!(texts.contains(&"李俊彦飞书"));
        assert!(!texts.contains(&"飞书"));
        assert!(!texts.contains(&"李俊彦"));
        assert!(!texts.contains(&"俊彦飞"));
        assert!(texts.contains(&"确保数据安全"));
    }

    #[test]
    fn persist_summary_stats_does_not_overwrite_local_keywords() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
        upsert_stat(
            &conn,
            "2026-05-01",
            "profile-1",
            "keywords",
            &[serde_json::json!({ "text": "本地词云", "weight": 3 })],
        )
        .expect("seed keywords");

        persist_summary_stats(
            &conn,
            "2026-05-01",
            "profile-1",
            AiSummary {
                topics: vec![serde_json::json!({ "title": "话题", "summary": "摘要", "count": 2 })],
            },
            &[],
        )
        .expect("summary stats");

        let raw: String = conn
            .query_row(
                "select value_json from daily_stats where day = '2026-05-01' and profile_id = 'profile-1' and metric = 'keywords'",
                [],
                |row| row.get(0),
            )
            .expect("keywords");
        assert!(raw.contains("本地词云"));
        assert!(!raw.contains("ai词云"));
    }

    #[test]
    fn split_analysis_batches_keeps_each_chat_together() {
        let mut messages = Vec::new();
        for index in 0..30 {
            messages.push(test_message(format!("a-{index}"), "chat-a"));
            messages.push(test_message(format!("b-{index}"), "chat-b"));
        }

        let batches = split_analysis_batches(
            messages,
            OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE,
            OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS,
        );
        assert!(batches
            .iter()
            .all(|batch| estimate_analysis_messages_tokens(batch)
                <= OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS));
        assert!(batches.iter().any(|batch| batch
            .iter()
            .filter(|message| message.chat_id == "chat-a")
            .count()
            == 30));
        assert!(batches.iter().any(|batch| batch
            .iter()
            .filter(|message| message.chat_id == "chat-b")
            .count()
            == 30));
    }

    #[test]
    fn split_analysis_batches_allows_large_single_chat_batch() {
        let messages = (0..195)
            .map(|index| test_message(format!("a-{index}"), "chat-a"))
            .collect::<Vec<_>>();

        let batches = split_analysis_batches(
            messages,
            OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE,
            OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS,
        );
        assert!(batches.len() > 1);
        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 195);
        assert!(batches
            .iter()
            .all(|batch| estimate_analysis_messages_tokens(batch)
                <= OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS));
    }

    #[test]
    fn split_analysis_batches_splits_long_single_chat_by_token_budget() {
        let mut messages = (0..12)
            .map(|index| test_message(format!("a-{index}"), "chat-a"))
            .collect::<Vec<_>>();
        for message in &mut messages {
            message.content = "需要确认这个客户投诉和交付风险。".repeat(35);
        }

        let batches = split_analysis_batches(
            messages,
            OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE,
            LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS,
        );

        assert!(batches.len() > 1);
        assert!(batches
            .iter()
            .all(|batch| estimate_analysis_messages_tokens(batch)
                <= LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS));
    }

    #[test]
    fn split_analysis_batches_packs_multiple_chats_until_near_limit() {
        let messages = (0..170)
            .map(|index| test_message(format!("a-{index}"), &format!("chat-{index}")))
            .collect::<Vec<_>>();

        let batches = split_analysis_batches(
            messages,
            OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE,
            OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS,
        );

        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 170);
        assert!(batches.len() > 1);
        assert!(batches
            .iter()
            .all(|batch| estimate_analysis_messages_tokens(batch)
                <= OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS));
    }

    #[test]
    fn split_analysis_batches_starts_next_batch_before_crossing_limit() {
        let messages = (0..90)
            .map(|index| test_message(format!("a-{index}"), &format!("chat-{index}")))
            .collect::<Vec<_>>();

        let batches = split_analysis_batches(
            messages,
            LOCAL_DEEPSEEK_MAX_ANALYSIS_BATCH_MESSAGES,
            LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS,
        );

        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 90);
        assert!(batches.len() > 2);
        assert!(batches
            .iter()
            .all(|batch| estimate_analysis_messages_tokens(batch)
                <= LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS));
    }

    #[test]
    fn normalize_analysis_batch_size_uses_provider_limits() {
        let local = AiConfig {
            provider: LOCAL_DEEPSEEK_PROVIDER.to_owned(),
            api_key: String::new(),
            base_url: "http://127.0.0.1:11434/v1".to_owned(),
            model: LOCAL_DEEPSEEK_MODEL.to_owned(),
            user_prompt: String::new(),
            analysis_prompt: String::new(),
            summary_prompt: String::new(),
            analysis_prompt_custom: false,
            summary_prompt_custom: false,
            analysis_batch_size: 50,
            enabled: false,
            test_status: "untested".to_owned(),
        };
        assert_eq!(
            normalize_config(local).analysis_batch_size,
            LOCAL_DEEPSEEK_MAX_ANALYSIS_BATCH_MESSAGES
        );

        let other = AiConfig {
            provider: "火山方舟".to_owned(),
            model: "doubao-seed".to_owned(),
            analysis_batch_size: 0,
            ..normalize_config(AiConfig {
                provider: LOCAL_DEEPSEEK_PROVIDER.to_owned(),
                api_key: String::new(),
                base_url: String::new(),
                model: LOCAL_DEEPSEEK_MODEL.to_owned(),
                user_prompt: String::new(),
                analysis_prompt: String::new(),
                summary_prompt: String::new(),
                analysis_prompt_custom: false,
                summary_prompt_custom: false,
                analysis_batch_size: LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE,
                enabled: false,
                test_status: String::new(),
            })
        };
        assert_eq!(
            normalize_config(other.clone()).analysis_batch_size,
            OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE
        );

        let other_over_limit = AiConfig {
            provider: "火山方舟".to_owned(),
            model: "doubao-seed".to_owned(),
            analysis_batch_size: 999,
            ..other
        };
        assert_eq!(
            normalize_config(other_over_limit).analysis_batch_size,
            OTHER_MODEL_MAX_ANALYSIS_BATCH_MESSAGES
        );
    }

    #[test]
    fn default_prompts_follow_builtin_updates_until_customized() {
        let saved_default = AiConfig {
            provider: LOCAL_DEEPSEEK_PROVIDER.to_owned(),
            api_key: String::new(),
            base_url: "http://127.0.0.1:11434/v1".to_owned(),
            model: LOCAL_DEEPSEEK_MODEL.to_owned(),
            user_prompt: String::new(),
            analysis_prompt: "你是一个本地即时通讯工作助理。请只根据输入的今天聊天消息识别真正需要用户处理的事项。\n\n请返回严格 JSON，不要 Markdown，不要解释：".to_owned(),
            summary_prompt: "你是一个本地即时通讯工作助理。请根据输入的今天聊天消息生成看板话题。\n\n话题规则：candidateTopics只包含本次尚未汇总的新消息候选；不要使用群聊当天总消息数。\n\n请返回严格 JSON，不要 Markdown，不要解释：".to_owned(),
            analysis_prompt_custom: false,
            summary_prompt_custom: false,
            analysis_batch_size: LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE,
            enabled: true,
            test_status: "untested".to_owned(),
        };

        let normalized = normalize_config(saved_default);
        assert_eq!(normalized.analysis_prompt, DEFAULT_ANALYSIS_PROMPT);
        assert_eq!(normalized.summary_prompt, DEFAULT_SUMMARY_PROMPT);
        assert!(!normalized.analysis_prompt_custom);
        assert!(!normalized.summary_prompt_custom);
    }

    #[test]
    fn custom_prompts_do_not_follow_builtin_updates() {
        let saved_custom = AiConfig {
            provider: LOCAL_DEEPSEEK_PROVIDER.to_owned(),
            api_key: String::new(),
            base_url: "http://127.0.0.1:11434/v1".to_owned(),
            model: LOCAL_DEEPSEEK_MODEL.to_owned(),
            user_prompt: String::new(),
            analysis_prompt: "自定义待办识别提示词".to_owned(),
            summary_prompt: "自定义话题识别提示词".to_owned(),
            analysis_prompt_custom: true,
            summary_prompt_custom: true,
            analysis_batch_size: LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE,
            enabled: true,
            test_status: "untested".to_owned(),
        };

        let normalized = normalize_config(saved_custom);
        assert_eq!(normalized.analysis_prompt, "自定义待办识别提示词");
        assert_eq!(normalized.summary_prompt, "自定义话题识别提示词");
        assert!(normalized.analysis_prompt_custom);
        assert!(normalized.summary_prompt_custom);
    }

    #[test]
    fn filter_analysis_messages_skips_low_signal_chats() {
        let mut message = test_message("msg-1".to_owned(), "chat-a");
        message.content = "哈哈哈哈，天气也太热了".to_owned();

        assert!(filter_analysis_messages(vec![message]).is_empty());
    }

    #[test]
    fn filter_analysis_messages_keeps_self_completion_evidence() {
        let mut message = test_message("msg-1".to_owned(), "chat-a");
        message.sender_id = "me".to_owned();
        message.sender_name = "我".to_owned();
        message.is_me = true;
        message.content = "好的，已经处理了".to_owned();

        assert_eq!(filter_analysis_messages(vec![message]).len(), 1);
    }

    #[test]
    fn filter_analysis_messages_skips_call_records() {
        let mut message = test_message("msg-1".to_owned(), "chat-a");
        message.content = "[语音通话] 通话时长 00:12".to_owned();

        assert!(filter_analysis_messages(vec![message]).is_empty());
    }

    #[test]
    fn summary_candidates_skip_call_records() {
        let mut call_record = test_message("msg-1".to_owned(), "chat-a");
        call_record.content = "[视频通话] 通话时长 10:03".to_owned();

        let mut business_message = test_message("msg-2".to_owned(), "chat-a");
        business_message.content = "客户投诉交付延迟，需要今天跟进处理".to_owned();

        let candidates = summary_candidates_from_messages(vec![call_record, business_message]);

        assert!(!candidates.iter().any(|candidate| {
            candidate.title_hint.contains("通话")
                || candidate
                    .snippets
                    .iter()
                    .any(|snippet| snippet.contains("通话"))
        }));
        assert!(!candidates.is_empty());
    }

    #[test]
    fn summary_candidates_do_not_premerge_generic_buying_across_chats() {
        let mut partner_message = test_message("msg-1".to_owned(), "partner-chat");
        partner_message.chat_name = "女朋友".to_owned();
        partner_message.content = "你昨天答应买东西，今天到底有没有买？".to_owned();

        let mut procurement_message = test_message("msg-2".to_owned(), "company-chat");
        procurement_message.chat_name = "公司采购群".to_owned();
        procurement_message.is_group = true;
        procurement_message.content = "办公用品采购的东西有没有买，供应商那边等确认。".to_owned();

        let candidates =
            summary_candidates_from_messages(vec![partner_message, procurement_message]);
        let buy_candidates = candidates
            .iter()
            .filter(|candidate| {
                candidate.title_hint.contains("买")
                    || candidate
                        .keywords
                        .iter()
                        .any(|keyword| keyword.contains("买"))
                    || candidate
                        .snippets
                        .iter()
                        .any(|snippet| snippet.contains("有没有买"))
            })
            .collect::<Vec<_>>();

        assert!(
            buy_candidates.len() >= 2,
            "generic buying topic should stay split by chat before AI summary; got {candidates:?}"
        );
        assert!(
            buy_candidates
                .iter()
                .all(|candidate| candidate.source_chats.len() == 1),
            "generic buying candidates should not contain multiple source chats; got {buy_candidates:?}"
        );
    }

    #[test]
    fn load_summary_candidates_only_reads_unsummarized_messages() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
        for (id, content, summarized_at) in [
            (
                "msg-old",
                "客户投诉交付延迟，需要今天跟进处理",
                "datetime('now')",
            ),
            ("msg-new", "客户投诉交付质量，需要今天跟进处理", "null"),
        ] {
            conn.execute(
                &format!(
                    "insert into daily_messages(
                       id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id,
                       sender_name, timestamp, time_text, msg_type, content, content_hash,
                       topic_summarized_at
                     )
                     values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', '测试群', 1,
                            'u-1', '客户', 1, '09:00', 'text', ?2, ?3, {summarized_at})"
                ),
                params![id, content, format!("hash-{id}")],
            )
            .expect("message");
        }

        let candidates =
            load_summary_candidates(&conn, "2026-05-01", "profile-1").expect("candidates");
        let source_ids = candidates
            .iter()
            .flat_map(|candidate| candidate.source_message_ids.iter().cloned())
            .collect::<HashSet<_>>();

        assert!(source_ids.contains("msg-new"));
        assert!(!source_ids.contains("msg-old"));
    }

    #[test]
    fn default_prompt_uses_explicit_existing_action_item_id() {
        assert!(DEFAULT_ANALYSIS_PROMPT.contains("existingActionItemId"));
        assert!(BATCH_DEDUP_PROMPT.contains("existingActionItemId"));
    }

    #[test]
    fn default_prompts_require_primary_chat_language() {
        assert!(DEFAULT_ANALYSIS_PROMPT.contains("主要语言"));
        assert!(DEFAULT_SUMMARY_PROMPT.contains("主要语言"));
    }

    #[test]
    fn summary_prompt_prevents_generic_cross_scene_merges() {
        assert!(DEFAULT_SUMMARY_PROMPT.contains("不能只因为共享"));
        assert!(DEFAULT_SUMMARY_PROMPT.contains("女朋友让我买东西"));
        assert!(DEFAULT_SUMMARY_PROMPT.contains("公司群讨论采购是否已买"));
        assert!(DEFAULT_SUMMARY_PROMPT.contains("existingTopics"));
        assert!(DEFAULT_SUMMARY_PROMPT.contains("sourceMessageIds"));
        assert!(DEFAULT_SUMMARY_PROMPT.contains("必须等于去重后的 sourceMessageIds"));
    }

    #[test]
    fn merge_incremental_summary_topics_extends_existing_count() {
        let existing_topics = vec![SummaryTopicContext {
            id: "topic-send-dfw".to_owned(),
            title: "5月14日早上5:40到DFW一人送机需求".to_owned(),
            summary: "用户发布送机需求".to_owned(),
            count: 2,
            source_chats: vec![SummarySourceChat {
                chat_name: "Baylor 生活群".to_owned(),
                is_group: true,
            }],
            source_message_ids: vec!["old-1".to_owned(), "old-2".to_owned()],
        }];
        let candidates = vec![SummaryCandidate {
            id: "candidate-1".to_owned(),
            title_hint: "送机".to_owned(),
            keywords: vec!["送机".to_owned()],
            count: 2,
            source_chats: vec![SummarySourceChat {
                chat_name: "Baylor 生活群".to_owned(),
                is_group: true,
            }],
            source_message_ids: vec!["new-1".to_owned(), "new-2".to_owned()],
            snippets: vec!["找送机 5月14号早上5:40出发到dfw一人".to_owned()],
            risk: false,
        }];
        let mut summary = AiSummary {
            topics: vec![serde_json::json!({
                "id": "topic-send-dfw",
                "title": "5月14日早上5:40到DFW一人送机需求",
                "summary": "用户发布送机需求",
                "count": 16,
                "sourceMessageIds": ["new-1", "new-1", "new-2", "fake-msg"],
                "sourceChats": [{ "chatName": "Baylor 生活群", "isGroup": true }]
            })],
        };

        merge_incremental_summary_topics(&mut summary, &existing_topics, &candidates);

        assert_eq!(summary.topics[0]["count"], serde_json::json!(4));
        assert_eq!(
            summary.topics[0]["sourceMessageIds"],
            serde_json::json!(["new-1", "new-2", "old-1", "old-2"])
        );
    }

    #[test]
    fn merge_incremental_summary_topics_keeps_existing_when_ids_omitted() {
        let existing_topics = vec![SummaryTopicContext {
            id: "topic-send-dfw".to_owned(),
            title: "5月14日早上5:40到DFW一人送机需求".to_owned(),
            summary: "用户发布送机需求".to_owned(),
            count: 2,
            source_chats: vec![SummarySourceChat {
                chat_name: "Baylor 生活群".to_owned(),
                is_group: true,
            }],
            source_message_ids: vec!["old-1".to_owned(), "old-2".to_owned()],
        }];
        let candidates = vec![SummaryCandidate {
            id: "candidate-1".to_owned(),
            title_hint: "送机".to_owned(),
            keywords: vec!["送机".to_owned()],
            count: 3,
            source_chats: vec![SummarySourceChat {
                chat_name: "Baylor 生活群".to_owned(),
                is_group: true,
            }],
            source_message_ids: vec!["msg-1".to_owned(), "msg-2".to_owned(), "msg-3".to_owned()],
            snippets: vec!["找送机 5月14号早上5:40出发到dfw一人".to_owned()],
            risk: false,
        }];
        let mut summary = AiSummary {
            topics: vec![serde_json::json!({
                "id": "topic-send-dfw",
                "title": "5月14日早上5:40到DFW一人送机需求",
                "summary": "用户发布送机需求",
                "count": 16,
                "sourceChats": [{ "chatName": "Baylor 生活群", "isGroup": true }]
            })],
        };

        merge_incremental_summary_topics(&mut summary, &existing_topics, &candidates);

        assert_eq!(summary.topics[0]["count"], serde_json::json!(2));
        assert_eq!(
            summary.topics[0]["sourceMessageIds"],
            serde_json::json!(["old-1", "old-2"])
        );
    }

    #[test]
    fn merge_incremental_summary_topics_keeps_existing_topics_omitted_by_ai() {
        let existing_topics = vec![SummaryTopicContext {
            id: "topic-existing".to_owned(),
            title: "旧话题".to_owned(),
            summary: "旧摘要".to_owned(),
            count: 2,
            source_chats: vec![SummarySourceChat {
                chat_name: "旧群".to_owned(),
                is_group: true,
            }],
            source_message_ids: vec!["old-1".to_owned(), "old-2".to_owned()],
        }];
        let candidates = vec![SummaryCandidate {
            id: "candidate-1".to_owned(),
            title_hint: "新话题".to_owned(),
            keywords: vec!["新话题".to_owned()],
            count: 1,
            source_chats: vec![SummarySourceChat {
                chat_name: "新群".to_owned(),
                is_group: true,
            }],
            source_message_ids: vec!["new-1".to_owned()],
            snippets: vec!["新话题讨论".to_owned()],
            risk: false,
        }];
        let mut summary = AiSummary {
            topics: vec![serde_json::json!({
                "id": "topic-new",
                "title": "新话题",
                "summary": "新摘要",
                "count": 1,
                "sourceMessageIds": ["new-1"],
                "sourceChats": [{ "chatName": "新群", "isGroup": true }]
            })],
        };

        merge_incremental_summary_topics(&mut summary, &existing_topics, &candidates);

        let ids = summary
            .topics
            .iter()
            .filter_map(|topic| topic.get("id").and_then(|value| value.as_str()))
            .collect::<HashSet<_>>();
        assert!(ids.contains("topic-existing"));
        assert!(ids.contains("topic-new"));
    }

    #[test]
    fn persist_analysis_can_write_resolved_reply_as_done() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, ?2, ?3, 'wechat', ?4, ?5, 0, 'friend', '朋友', 1, '09:00', 'text', '你看下可以吗', 'hash-1')",
            params![
                "msg-1",
                "2026-05-01",
                "profile-1",
                "chat-1",
                "测试聊天"
            ],
        )
        .expect("message");

        let analysis = AiAnalysis {
            action_items: vec![AiActionItem {
                item_type: "reply".to_owned(),
                status: Some("done".to_owned()),
                priority: "medium".to_owned(),
                title: "回复确认问题".to_owned(),
                description: "对方询问后，用户已经回复处理。".to_owned(),
                suggested_reply: None,
                chat_id: "chat-1".to_owned(),
                profile_id: None,
                existing_action_item_id: None,
                source_message_ids: vec!["msg-1".to_owned()],
                evidence_summary: "对方询问后已回复。".to_owned(),
                context_incomplete: false,
            }],
            topics: Vec::new(),
            keywords: Vec::new(),
        };

        persist_analysis_across_profiles(&conn, "2026-05-01", &[], analysis)
            .expect("persist analysis");
        let (status, completed_at, first_detected_at, expected_detected_at): (
            String,
            Option<String>,
            String,
            String,
        ) = conn
            .query_row(
                "select status, completed_at, first_detected_at, datetime(1, 'unixepoch', 'localtime')
                 from action_items where type = 'reply'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("action item");
        assert_eq!(status, "done");
        assert!(completed_at.is_some());
        assert_eq!(first_detected_at, expected_detected_at);
    }

    #[test]
    fn persist_analysis_does_not_merge_open_items_without_existing_action_item_id() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values('msg-1', '2026-05-01', 'profile-1', 'wechat', 'chat-1', '测试聊天',
                    0, 'friend', '朋友', 1, '09:00', 'text', '请确认合同', 'hash-1')",
            [],
        )
        .expect("message");
        conn.execute(
            "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id,
               chat_name, source_message_ids, evidence_summary, first_detected_at, last_updated_at
             )
             values('act-existing', 'task', 'open', 'medium', '确认合同', '旧事项',
                    'profile-1', 'wechat', 'chat-1', '测试聊天', '[\"old-msg\"]',
                    '旧证据', datetime('now'), datetime('now'))",
            [],
        )
        .expect("existing action item");

        let analysis = AiAnalysis {
            action_items: vec![AiActionItem {
                item_type: "task".to_owned(),
                status: Some("open".to_owned()),
                priority: "medium".to_owned(),
                title: "确认合同".to_owned(),
                description: "新消息要求确认合同。".to_owned(),
                suggested_reply: None,
                chat_id: "chat-1".to_owned(),
                profile_id: None,
                existing_action_item_id: None,
                source_message_ids: vec!["msg-1".to_owned()],
                evidence_summary: "对方要求确认合同。".to_owned(),
                context_incomplete: false,
            }],
            topics: Vec::new(),
            keywords: Vec::new(),
        };

        persist_analysis_across_profiles(&conn, "2026-05-01", &[], analysis)
            .expect("persist analysis");
        let count: i64 = conn
            .query_row(
                "select count(*) from action_items where profile_id = 'profile-1' and chat_id = 'chat-1'",
                [],
                |row| row.get(0),
            )
            .expect("action item count");
        let old_sources: String = conn
            .query_row(
                "select source_message_ids from action_items where id = 'act-existing'",
                [],
                |row| row.get(0),
            )
            .expect("old sources");
        assert_eq!(count, 2);
        assert_eq!(old_sources, "[\"old-msg\"]");
    }

    #[test]
    fn persist_analysis_uses_existing_action_item_without_action_item_group_column() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("schema");
        conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values('msg-1', '2026-05-01', 'profile-1', 'wechat', 'chat-1', '测试群',
                    1, 'member-1', '成员', 1, '09:00', 'text', '继续确认合同', 'hash-1')",
            [],
        )
        .expect("message");
        conn.execute(
            "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id,
               chat_name, source_message_ids, evidence_summary, first_detected_at, last_updated_at
             )
             values('act-existing', 'task', 'open', 'medium', '确认合同', '旧事项',
                    'profile-1', 'wechat', 'chat-1', '测试群', '[\"old-msg\"]',
                    '旧证据', datetime('now'), datetime('now'))",
            [],
        )
        .expect("existing action item");

        let analysis = AiAnalysis {
            action_items: vec![AiActionItem {
                item_type: "task".to_owned(),
                status: Some("open".to_owned()),
                priority: "high".to_owned(),
                title: "确认合同".to_owned(),
                description: "群里继续催促确认合同。".to_owned(),
                suggested_reply: None,
                chat_id: "chat-1".to_owned(),
                profile_id: None,
                existing_action_item_id: Some("act-existing".to_owned()),
                source_message_ids: vec!["msg-1".to_owned()],
                evidence_summary: "群里继续催促。".to_owned(),
                context_incomplete: true,
            }],
            topics: Vec::new(),
            keywords: Vec::new(),
        };

        let persisted = persist_analysis_across_profiles(&conn, "2026-05-01", &[], analysis)
            .expect("persist analysis");
        let count: i64 = conn
            .query_row("select count(*) from action_items", [], |row| row.get(0))
            .expect("action item count");
        let priority: String = conn
            .query_row(
                "select priority from action_items where id = 'act-existing'",
                [],
                |row| row.get(0),
            )
            .expect("updated priority");

        assert_eq!(count, 1);
        assert_eq!(priority, "high");
        assert_eq!(persisted.context_requests.len(), 1);
        assert!(persisted.context_requests[0].is_group);
    }

    fn test_message(id: String, chat_id: &str) -> AnalysisMessage {
        AnalysisMessage {
            id,
            profile_id: "profile-1".to_owned(),
            platform: "wechat".to_owned(),
            chat_id: chat_id.to_owned(),
            chat_name: chat_id.to_owned(),
            is_group: false,
            timestamp: 1,
            sender_id: "friend".to_owned(),
            sender_name: "朋友".to_owned(),
            is_me: false,
            time_text: "09:00".to_owned(),
            msg_type: "text".to_owned(),
            content: "测试消息".to_owned(),
            partial: false,
        }
    }

    fn test_keyword_rank(text: &str, score: f64) -> LocalKeywordRank {
        LocalKeywordRank {
            text: text.to_owned(),
            value: LocalKeywordScore {
                score,
                occurrences: 1,
                chat_ids: HashSet::new(),
                context_hits: 0,
            },
            final_score: score,
        }
    }
}
