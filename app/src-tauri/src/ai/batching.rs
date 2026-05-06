use std::collections::HashMap;

use super::{
    AnalysisMessage, SummaryCandidate, MAX_SUMMARY_CANDIDATES_PER_REQUEST,
    MAX_SUMMARY_SOURCE_MESSAGES_PER_REQUEST, MIN_ANALYSIS_BATCH_MESSAGES,
    OTHER_MODEL_MAX_ANALYSIS_BATCH_MESSAGES,
};

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
        // 同一会话优先放在同一批，避免 AI 因上下文被切断而误判待回复或待办；只有单个会话本身超限时才继续拆分。
        if group.len() > max_batch_messages || group_tokens > max_batch_tokens {
            if !current.is_empty() {
                batches.push(current);
                current = Vec::new();
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

pub(crate) fn estimate_analysis_messages_tokens(messages: &[AnalysisMessage]) -> usize {
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
