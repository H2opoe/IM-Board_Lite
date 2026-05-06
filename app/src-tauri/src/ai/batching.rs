use std::collections::HashMap;

use super::{
    ActionContext, AnalysisContext, AnalysisMessage, HistoricalContextMessage, SummaryCandidate,
    MAX_SUMMARY_CANDIDATES_PER_REQUEST, MAX_SUMMARY_SOURCE_MESSAGES_PER_REQUEST,
    MIN_ANALYSIS_BATCH_MESSAGES, OTHER_MODEL_MAX_ANALYSIS_BATCH_MESSAGES,
};

const LARGE_CHAT_OVERLAP_MESSAGES: usize = 4;
const REQUEST_JSON_STRUCTURE_TOKENS: usize = 192;

#[cfg(test)]
pub(crate) fn split_analysis_batches(
    messages: Vec<AnalysisMessage>,
    max_batch_messages: i64,
    max_batch_tokens: usize,
) -> Vec<Vec<AnalysisMessage>> {
    split_analysis_batch_plans(messages, max_batch_messages, max_batch_tokens, 0)
        .into_iter()
        .map(|plan| plan.primary_messages().to_vec())
        .collect()
}

#[derive(Debug, Clone)]
pub(crate) struct AnalysisBatchPlan {
    pub messages: Vec<AnalysisMessage>,
    pub estimated_input_tokens: usize,
    pub overlap_message_count: usize,
}

impl AnalysisBatchPlan {
    fn new(messages: Vec<AnalysisMessage>, overlap_message_count: usize) -> Self {
        let estimated_input_tokens = estimate_analysis_messages_tokens(&messages);
        Self {
            messages,
            estimated_input_tokens,
            overlap_message_count,
        }
    }

    pub(crate) fn primary_messages(&self) -> &[AnalysisMessage] {
        let start = self.overlap_message_count.min(self.messages.len());
        &self.messages[start..]
    }

    pub(crate) fn primary_message_count(&self) -> usize {
        self.primary_messages().len()
    }
}

pub(crate) fn split_analysis_batch_plans(
    messages: Vec<AnalysisMessage>,
    max_batch_messages: i64,
    max_batch_tokens: usize,
    fixed_request_tokens: usize,
) -> Vec<AnalysisBatchPlan> {
    let max_batch_messages = max_batch_messages.clamp(
        MIN_ANALYSIS_BATCH_MESSAGES,
        OTHER_MODEL_MAX_ANALYSIS_BATCH_MESSAGES,
    ) as usize;
    let available_message_tokens = max_batch_tokens
        .saturating_sub(fixed_request_tokens)
        .max(1_200);
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

    let mut batches = Vec::<AnalysisBatchPlan>::new();
    let mut current = Vec::<AnalysisMessage>::new();
    let mut current_tokens = 0usize;
    for group in chat_groups {
        let group_tokens = estimate_analysis_messages_tokens(&group);
        let would_cross_message_limit = current.len() + group.len() > max_batch_messages;
        let would_cross_token_limit = current_tokens + group_tokens > available_message_tokens;
        if !current.is_empty() && (would_cross_message_limit || would_cross_token_limit) {
            batches.push(AnalysisBatchPlan::new(current, 0));
            current = Vec::new();
            current_tokens = 0;
        }
        // 同一会话优先放在同一批，避免 AI 因上下文被切断而误判待回复或待办；只有单个会话本身超限时才继续拆分。
        if group.len() > max_batch_messages || group_tokens > available_message_tokens {
            if !current.is_empty() {
                batches.push(AnalysisBatchPlan::new(current, 0));
                current = Vec::new();
            }
            for chunk in split_large_chat_group(group, max_batch_messages, available_message_tokens)
            {
                batches.push(chunk);
            }
            continue;
        }
        current.extend(group);
        current_tokens += group_tokens;
    }
    if !current.is_empty() {
        batches.push(AnalysisBatchPlan::new(current, 0));
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
) -> Vec<AnalysisBatchPlan> {
    let mut batches = Vec::<AnalysisBatchPlan>::new();
    let mut current = Vec::<AnalysisMessage>::new();
    let mut current_tokens = 0usize;
    let mut next_overlap = Vec::<AnalysisMessage>::new();

    for message in messages {
        let message_tokens = estimate_analysis_message_tokens(&message);
        if !current.is_empty()
            && (current.len() >= max_batch_messages
                || current_tokens + message_tokens > max_batch_tokens)
        {
            let overlap = tail_overlap_messages(&current, max_batch_tokens);
            batches.push(AnalysisBatchPlan::new(current, next_overlap.len()));
            next_overlap = overlap.into_iter().rev().collect::<Vec<_>>();
            current = next_overlap.clone();
            current_tokens = estimate_analysis_messages_tokens(&current);
            if current_tokens + message_tokens > max_batch_tokens {
                current.clear();
                current_tokens = 0;
                next_overlap.clear();
            }
        }
        current_tokens += message_tokens;
        current.push(message);
    }

    if !current.is_empty() {
        batches.push(AnalysisBatchPlan::new(current, next_overlap.len()));
    }
    batches
}

fn tail_overlap_messages(
    messages: &[AnalysisMessage],
    max_batch_tokens: usize,
) -> Vec<AnalysisMessage> {
    let overlap_token_budget = (max_batch_tokens / 4).max(1);
    let mut overlap = Vec::new();
    let mut overlap_tokens = 0usize;
    for message in messages.iter().rev() {
        if overlap.len() >= LARGE_CHAT_OVERLAP_MESSAGES {
            break;
        }
        let message_tokens = estimate_analysis_message_tokens(message);
        if !overlap.is_empty() && overlap_tokens + message_tokens > overlap_token_budget {
            break;
        }
        overlap_tokens += message_tokens;
        overlap.push(message.clone());
    }
    overlap
}

pub(crate) fn estimate_analysis_messages_tokens(messages: &[AnalysisMessage]) -> usize {
    messages.iter().map(estimate_analysis_message_tokens).sum()
}

pub(crate) fn estimate_analysis_context_tokens(context: &AnalysisContext) -> usize {
    estimate_action_context_tokens(&context.existing_action_items)
        + estimate_historical_context_tokens(&context.historical_messages)
        + REQUEST_JSON_STRUCTURE_TOKENS
}

pub(crate) fn estimate_analysis_request_tokens(
    prompt: &str,
    user_prompt: &str,
    messages: &[AnalysisMessage],
    context: &AnalysisContext,
) -> usize {
    estimate_text_tokens(prompt)
        + estimate_text_tokens(user_prompt)
        + estimate_analysis_messages_tokens(messages)
        + estimate_analysis_context_tokens(context)
        + REQUEST_JSON_STRUCTURE_TOKENS
}

pub(crate) fn estimate_text_for_request(value: &str) -> usize {
    estimate_text_tokens(value)
}

fn estimate_action_context_tokens(items: &[ActionContext]) -> usize {
    items
        .iter()
        .map(|item| {
            estimate_text_tokens(&item.id)
                + estimate_text_tokens(&item.profile_id)
                + estimate_text_tokens(&item.platform)
                + estimate_text_tokens(&item.item_type)
                + estimate_text_tokens(&item.status)
                + estimate_text_tokens(&item.priority)
                + estimate_text_tokens(&item.title)
                + estimate_text_tokens(&item.description)
                + item
                    .suggested_reply
                    .as_deref()
                    .map(estimate_text_tokens)
                    .unwrap_or_default()
                + estimate_text_tokens(&item.chat_id)
                + estimate_text_tokens(&item.chat_name)
                + estimate_text_tokens(&item.evidence_summary)
                + estimate_text_tokens(&item.last_updated_at)
                + 48
        })
        .sum()
}

fn estimate_historical_context_tokens(messages: &[HistoricalContextMessage]) -> usize {
    messages
        .iter()
        .map(|message| {
            estimate_text_tokens(&message.chat_id)
                + estimate_text_tokens(&message.chat_name)
                + estimate_text_tokens(&message.day)
                + estimate_text_tokens(&message.time_text)
                + estimate_text_tokens(&message.sender_name)
                + estimate_text_tokens(&message.content)
                + 32
        })
        .sum()
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
