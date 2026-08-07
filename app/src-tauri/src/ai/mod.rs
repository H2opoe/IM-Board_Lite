use chrono::Local;
use rusqlite::params_from_iter;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

use crate::analysis::local_keywords::{is_cjk, is_self_sender};
#[cfg(test)]
use crate::storage::models::AiConfig;

mod action_items;
mod analysis_loader;
mod batching;
mod config;
mod json_utils;
mod keyword_refine;
mod message_cleaning;
mod prompts;
mod request;
mod stats_persistence;
mod summary_candidates;
mod topic_merge;

#[cfg(test)]
use action_items::persist_analysis_across_profiles;
pub(crate) use analysis_loader::{
    context_backfill_targets_for_messages, extend_analysis_context_with_history,
    keep_analysis_pending_for_messages, load_analysis_context_for_messages,
    load_analysis_messages_for_profiles, persist_analysis_for_messages,
};
#[cfg(test)]
pub(crate) use batching::estimate_analysis_messages_tokens;
#[cfg(test)]
pub(crate) use batching::split_analysis_batches;
pub(crate) use batching::{
    estimate_analysis_context_tokens, estimate_analysis_request_tokens, estimate_text_for_request,
    split_analysis_batch_plans, split_summary_candidate_batches, AnalysisBatchPlan,
};
#[cfg(test)]
pub(crate) use config::normalize_config;
pub(crate) use config::{analysis_batch_token_budget, apply_hardware_batch_limit};
pub use config::{
    default_config, get_config, is_configured, is_local_provider, save_config,
    with_provider_headers,
};
pub(crate) use json_utils::{extract_json_object, hash_text, truncate_text};
pub(crate) use keyword_refine::{
    build_keyword_refine_plan, mark_keyword_refine_failed, mark_keyword_refine_pending,
    persist_refined_keywords_from_analysis, KeywordRefinePlan,
};
pub(crate) use message_cleaning::{
    clean_message_content_for_ai, filter_analysis_messages, is_disallowed_dashboard_keyword,
    is_disallowed_dashboard_topic, should_skip_ai_message,
};
pub(crate) use prompts::{
    BATCH_DEDUP_PROMPT, LOCAL_MODEL_OUTPUT_PROMPT, MANAGEMENT_RISK_PROMPT,
    PRIMARY_LANGUAGE_OUTPUT_PROMPT,
};
pub use prompts::{DEFAULT_ANALYSIS_PROMPT, DEFAULT_SUMMARY_PROMPT};
pub(crate) use request::{
    analysis_system_prompt, describe_request_error, request_profile_analysis,
    request_profile_summary,
};
#[cfg(test)]
pub(crate) use stats_persistence::persist_local_keyword_stats;
pub(crate) use stats_persistence::{
    current_keyword_version, persist_local_keyword_stats_with_status, persist_summary_stats,
    upsert_keyword_meta, upsert_stat,
};
#[cfg(test)]
use summary_candidates::summary_candidates_from_messages;
pub(crate) use summary_candidates::{
    load_existing_summary_topics, load_summary_candidates, load_summary_candidates_for_profiles,
    summary_topic_context_to_value, summary_topic_contexts_from_summary, summary_topic_id,
};
pub(crate) use topic_merge::merge_duplicate_summary_topics;
#[cfg(test)]
pub(crate) use topic_merge::merge_incremental_summary_topics;

pub const LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE: i64 = 20;
pub const OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE: i64 = 100;
pub const MIN_ANALYSIS_BATCH_MESSAGES: i64 = 10;
pub const LOCAL_DEEPSEEK_MAX_ANALYSIS_BATCH_MESSAGES: i64 = 30;
pub const OTHER_MODEL_MAX_ANALYSIS_BATCH_MESSAGES: i64 = 300;
const LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS: usize = 4_096;
const OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS: usize = 32_000;
const LOCAL_DEEPSEEK_ANALYSIS_OUTPUT_TOKENS: usize = 1_536;
const OTHER_MODEL_ANALYSIS_OUTPUT_TOKENS: usize = 2_048;
const LOCAL_DEEPSEEK_SUMMARY_OUTPUT_TOKENS: usize = 3_072;
const OTHER_MODEL_SUMMARY_OUTPUT_TOKENS: usize = 6_144;
const ANALYSIS_REQUEST_RESERVED_TOKENS: usize = 640;
const MAX_SUMMARY_MESSAGES: usize = 800;
const MAX_SUMMARY_CANDIDATES: usize = 12;
const MAX_SUMMARY_CANDIDATES_PER_REQUEST: usize = 4;
const MAX_SUMMARY_SOURCE_MESSAGES_PER_REQUEST: usize = 16;
const MAX_SUMMARY_SNIPPETS_PER_CANDIDATE: usize = 2;
const ANALYSIS_REQUEST_TIMEOUT_SECS: u64 = 180;
const SUMMARY_REQUEST_TIMEOUT_SECS: u64 = 300;
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
    pub(crate) keywords: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AiSummary {
    #[serde(default)]
    pub topics: Vec<serde_json::Value>,
    #[serde(default)]
    pub keywords: Vec<serde_json::Value>,
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

#[cfg(test)]
mod tests;
