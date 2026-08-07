use std::error::Error;

use crate::security::sanitize_log;
use crate::storage::models::AiConfig;

use super::topic_merge::merge_incremental_summary_topics;
use super::*;

pub(crate) async fn request_profile_analysis(
    config: &AiConfig,
    day: &str,
    profile_id: &str,
    messages: &[AnalysisMessage],
    context: &AnalysisContext,
) -> anyhow::Result<AiAnalysisResult> {
    let payload = serde_json::json!({
        "day": day,
        "profileId": profile_id,
        "messageCount": messages.len(),
        "analysisMode": "incremental",
        "userPrompt": config.user_prompt.trim(),
        "existingActionItems": context.existing_action_items,
        "historicalMessages": context.historical_messages,
        "messages": messages,
    });
    let prompt = analysis_system_prompt(config);
    let output = request_analysis(
        config,
        "待回复和待办事项识别",
        ANALYSIS_REQUEST_TIMEOUT_SECS,
        &prompt,
        &payload,
        analysis_output_tokens(config, false),
    )
    .await?;
    let (mut analysis, diagnostics) = match parse_analysis_output(&output) {
        Ok(analysis) => (analysis, output.diagnostics),
        Err(err) if should_retry_analysis_parse(&err, &output.diagnostics) => {
            let retry_output = request_analysis(
                config,
                "待回复和待办事项识别",
                ANALYSIS_REQUEST_TIMEOUT_SECS,
                &analysis_retry_prompt(&prompt),
                &payload,
                analysis_output_tokens(config, true),
            )
            .await?;
            (
                parse_analysis_output(&retry_output)?,
                retry_output.diagnostics,
            )
        }
        Err(err) => return Err(err.into()),
    };
    analysis.topics.clear();
    analysis.keywords.clear();
    Ok(AiAnalysisResult {
        analysis,
        diagnostics,
    })
}

fn analysis_output_tokens(config: &AiConfig, retry: bool) -> u32 {
    let tokens = if is_local_provider(config) {
        if retry {
            LOCAL_DEEPSEEK_ANALYSIS_OUTPUT_TOKENS + 1_024
        } else {
            LOCAL_DEEPSEEK_ANALYSIS_OUTPUT_TOKENS
        }
    } else if retry {
        OTHER_MODEL_ANALYSIS_OUTPUT_TOKENS * 2
    } else {
        OTHER_MODEL_ANALYSIS_OUTPUT_TOKENS
    };
    tokens.try_into().unwrap_or(u32::MAX)
}

fn analysis_retry_prompt(prompt: &str) -> String {
    format!(
        "{prompt}\n\n重试输出约束：上一次待回复和待办事项 JSON 被截断或不是有效 JSON。请只返回一个完整 JSON 对象，顶层只能包含 actionItems；没有明确事项时返回 {{\"actionItems\":[]}}。请压缩每个字段内容，title 不超过 16 字，description/evidenceSummary/suggestedReply 都用一句短句；最多返回 20 个最重要事项。必须一次性闭合完整 JSON，不要 Markdown，不要解释。"
    )
}

pub(crate) fn analysis_system_prompt(config: &AiConfig) -> String {
    let base_prompt = if config.analysis_prompt.trim().is_empty() {
        DEFAULT_ANALYSIS_PROMPT
    } else {
        config.analysis_prompt.trim()
    };
    let mut prompt = base_prompt.to_owned();
    append_prompt_section_if_missing(&mut prompt, &["主要语言"], PRIMARY_LANGUAGE_OUTPUT_PROMPT);
    append_prompt_section_if_missing(
        &mut prompt,
        &["批内去重", "同一批 messages"],
        BATCH_DEDUP_PROMPT,
    );
    append_prompt_section_if_missing(
        &mut prompt,
        &["沟通氛围", "情绪风险"],
        MANAGEMENT_RISK_PROMPT,
    );
    if is_local_provider(config) && !prompt.contains("不要输出 <think>") {
        prompt.push_str("\n\n");
        prompt.push_str(LOCAL_MODEL_OUTPUT_PROMPT);
    }
    prompt
}

pub(crate) async fn request_profile_summary(
    config: &AiConfig,
    day: &str,
    profile_id: &str,
    existing_topics: &[SummaryTopicContext],
    candidates: &[SummaryCandidate],
    keyword_refine: Option<&serde_json::Value>,
) -> anyhow::Result<AiSummaryResult> {
    let mut payload = serde_json::json!({
        "day": day,
        "profileId": profile_id,
        "summaryMode": "incremental",
        "existingTopics": existing_topics,
        "candidateCount": candidates.len(),
        "userPrompt": config.user_prompt.trim(),
        "candidateTopics": candidates,
    });
    if let Some(keyword_refine) = keyword_refine {
        payload["keywordRefine"] = keyword_refine.clone();
    }
    let base_prompt = if config.summary_prompt.trim().is_empty() {
        DEFAULT_SUMMARY_PROMPT
    } else {
        config.summary_prompt.trim()
    };
    let mut prompt = base_prompt.to_owned();
    append_prompt_section_if_missing(&mut prompt, &["主要语言"], PRIMARY_LANGUAGE_OUTPUT_PROMPT);
    if is_local_provider(config) {
        prompt.push_str("\n\n");
        prompt.push_str(LOCAL_MODEL_OUTPUT_PROMPT);
    }
    let max_tokens = summary_output_tokens(config, false);
    let output = request_analysis(
        config,
        "热门话题和关键词识别",
        SUMMARY_REQUEST_TIMEOUT_SECS,
        &prompt,
        &payload,
        max_tokens,
    )
    .await?;
    let (mut summary, diagnostics) = match parse_summary_output(&output) {
        Ok(summary) => (summary, output.diagnostics),
        Err(err) if should_retry_summary_parse(&err, &output.diagnostics) => {
            let retry_output = request_analysis(
                config,
                "热门话题和关键词识别",
                SUMMARY_REQUEST_TIMEOUT_SECS,
                &summary_retry_prompt(&prompt),
                &payload,
                summary_output_tokens(config, true),
            )
            .await?;
            (
                parse_summary_output(&retry_output)?,
                retry_output.diagnostics,
            )
        }
        Err(err) => return Err(err.into()),
    };
    merge_incremental_summary_topics(&mut summary, existing_topics, candidates);
    if keyword_refine.is_none() {
        summary.keywords.clear();
    }
    Ok(AiSummaryResult {
        summary,
        diagnostics,
    })
}

fn summary_output_tokens(config: &AiConfig, retry: bool) -> u32 {
    let tokens = if is_local_provider(config) {
        if retry {
            LOCAL_DEEPSEEK_SUMMARY_OUTPUT_TOKENS + 1_024
        } else {
            LOCAL_DEEPSEEK_SUMMARY_OUTPUT_TOKENS
        }
    } else if retry {
        OTHER_MODEL_SUMMARY_OUTPUT_TOKENS * 2
    } else {
        OTHER_MODEL_SUMMARY_OUTPUT_TOKENS
    };
    tokens.try_into().unwrap_or(u32::MAX)
}

fn summary_retry_prompt(prompt: &str) -> String {
    format!(
        "{prompt}\n\n重试输出约束：上一次汇总 JSON 被截断。请压缩 summary，旧话题只返回 id/title/summary 和本批新增 sourceMessageIds；不要重复输出 existingTopics 已有的旧 sourceMessageIds。必须一次性闭合完整 JSON。"
    )
}

fn append_prompt_section_if_missing(prompt: &mut String, markers: &[&str], section: &str) {
    if markers.iter().any(|marker| prompt.contains(marker)) {
        return;
    }
    prompt.push_str("\n\n");
    prompt.push_str(section);
}

include!("response_parsing.rs");
include!("request_transport.rs");
include!("provider_adapters.rs");
include!("request_diagnostics.rs");
include!("request_tests.rs");
