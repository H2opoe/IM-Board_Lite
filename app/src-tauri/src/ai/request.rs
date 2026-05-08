use std::collections::{BTreeSet, HashMap, HashSet};
use std::error::Error;

use crate::security::sanitize_log;
use crate::storage::models::AiConfig;

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

fn parse_analysis_output(output: &AiRequestOutput) -> Result<AiAnalysis, AiCallError> {
    let json = extract_json_object(&output.content)
        .map_err(|err| AiCallError::new(err.to_string(), Some(output.diagnostics.clone())))?;
    serde_json::from_str(&json).map_err(|err| {
        AiCallError::new(
            format!(
                "AI返回JSON解析失败：{}；片段：{}",
                err,
                truncate_text(json.replace('\n', " "), 360)
            ),
            Some(output.diagnostics.clone()),
        )
    })
}

fn should_retry_analysis_parse(err: &AiCallError, diagnostics: &AiCallDiagnostics) -> bool {
    should_retry_json_parse(err, diagnostics)
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

fn parse_summary_output(output: &AiRequestOutput) -> Result<AiSummary, AiCallError> {
    let json = extract_json_object(&output.content)
        .map_err(|err| AiCallError::new(err.to_string(), Some(output.diagnostics.clone())))?;
    serde_json::from_str(&json).map_err(|err| {
        AiCallError::new(
            format!(
                "AI返回汇总JSON解析失败：{}；片段：{}",
                err,
                truncate_text(json.replace('\n', " "), 360)
            ),
            Some(output.diagnostics.clone()),
        )
    })
}

fn should_retry_summary_parse(err: &AiCallError, diagnostics: &AiCallDiagnostics) -> bool {
    should_retry_json_parse(err, diagnostics)
}

fn should_retry_json_parse(err: &AiCallError, diagnostics: &AiCallDiagnostics) -> bool {
    let message = err.message.as_str();
    let looks_incomplete = message.contains("不是完整JSON")
        || message.contains("不是JSON")
        || message.contains("EOF while parsing")
        || message.contains("expected")
        || message.contains("trailing characters");
    let finish_reason = diagnostics.finish_reason.as_deref().unwrap_or_default();
    looks_incomplete || matches!(finish_reason, "length" | "max_tokens")
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

pub(crate) fn merge_incremental_summary_topics(
    summary: &mut AiSummary,
    existing_topics: &[SummaryTopicContext],
    candidates: &[SummaryCandidate],
) {
    let mut valid_message_ids = existing_topics
        .iter()
        .flat_map(|topic| topic.source_message_ids.iter().cloned())
        .collect::<HashSet<_>>();
    valid_message_ids.extend(
        candidates
            .iter()
            .flat_map(|candidate| candidate.source_message_ids.iter().cloned()),
    );
    let existing_by_id = existing_topics
        .iter()
        .map(|topic| (topic.id.clone(), topic))
        .collect::<HashMap<_, _>>();
    for topic in &mut summary.topics {
        let Some(object) = topic.as_object_mut() else {
            continue;
        };
        let id = object
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                let title = object
                    .get("title")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let summary = object
                    .get("summary")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                summary_topic_id(title, summary)
            });
        let mut source_message_ids = existing_by_id
            .get(&id)
            .map(|existing| {
                existing
                    .source_message_ids
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        source_message_ids.extend(
            object
                .get("sourceMessageIds")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(str::trim)
                        .filter(|id| !id.is_empty() && valid_message_ids.contains(*id))
                        .map(str::to_owned)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default(),
        );
        object.insert("id".to_owned(), serde_json::json!(id));
        object.insert(
            "sourceMessageIds".to_owned(),
            serde_json::json!(source_message_ids.iter().cloned().collect::<Vec<_>>()),
        );
        let count = if source_message_ids.is_empty() {
            existing_by_id
                .get(&id)
                .map(|topic| topic.count)
                .unwrap_or_default()
        } else if let Some(existing) = existing_by_id.get(&id) {
            if existing.source_message_ids.is_empty() {
                existing
                    .count
                    .saturating_add(source_message_ids.len().try_into().unwrap_or_default())
            } else {
                source_message_ids.len().try_into().unwrap_or_default()
            }
        } else {
            source_message_ids.len().try_into().unwrap_or_default()
        };
        object.insert("count".to_owned(), serde_json::json!(count));
    }
    let returned_ids = summary
        .topics
        .iter()
        .filter_map(|topic| topic.get("id").and_then(|value| value.as_str()))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    for existing in existing_topics {
        if returned_ids.contains(&existing.id) {
            continue;
        }
        summary
            .topics
            .push(summary_topic_context_to_value(existing));
    }
    summary.topics.sort_by(|left, right| {
        let left_count = left
            .get("count")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        let right_count = right
            .get("count")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        right_count.cmp(&left_count)
    });
    summary.topics.truncate(MAX_SUMMARY_CANDIDATES);
}

pub(crate) fn describe_request_error(prefix: &str, err: reqwest::Error) -> String {
    let mut details = vec![err.to_string()];
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !details.iter().any(|detail| detail == &cause_text) {
            details.push(cause_text);
        }
        source = cause.source();
    }

    let hints = request_error_hints(&err, &details);

    let hint_text = if hints.is_empty() {
        String::new()
    } else {
        format!("；{}", hints.join("；"))
    };
    format!(
        "{prefix}：{}{}",
        truncate_text(details.join("；原因："), 700),
        hint_text
    )
}

fn request_error_hints(err: &reqwest::Error, details: &[String]) -> Vec<&'static str> {
    let mut hints = Vec::new();
    // rustls 的 unexpected-eof 常发生在 TLS 连接被服务商或中间网络提前断开时，
    // reqwest 可能把它归为 request/send 阶段，这里单独提示，避免误导用户只检查 Base URL。
    let tls_closed_early = details_contain(
        details,
        &[
            "peer closed connection without sending tls close_notify",
            "unexpected-eof",
            "unexpected eof",
        ],
    );
    if err.is_timeout() {
        hints.push("请求超时，请检查网络、代理或服务商响应速度");
    }
    if err.is_connect() {
        hints.push("连接失败，请检查 DNS、代理/VPN、防火墙或公司网络策略");
    }
    if tls_closed_early {
        hints.push("TLS 连接被对端或中间网络提前断开，通常是服务商、代理/VPN、网关或公司网络临时中断，可稍后重试或切换网络/代理");
    } else if err.is_request() {
        hints.push("请求未能发出，请确认 Base URL 可访问且系统时间正常");
    }
    hints
}

fn is_retryable_request_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || is_tls_closed_early(err)
}

fn is_tls_closed_early(err: &reqwest::Error) -> bool {
    let mut details = vec![err.to_string()];
    let mut source = err.source();
    while let Some(cause) = source {
        details.push(cause.to_string());
        source = cause.source();
    }
    details_contain(
        &details,
        &[
            "peer closed connection without sending tls close_notify",
            "unexpected-eof",
            "unexpected eof",
        ],
    )
}

fn details_contain(details: &[String], needles: &[&str]) -> bool {
    details.iter().any(|detail| {
        let detail = detail.to_lowercase();
        needles.iter().any(|needle| detail.contains(needle))
    })
}

pub(super) async fn request_analysis(
    config: &AiConfig,
    request_label: &str,
    timeout_secs: u64,
    system_prompt: &str,
    input: &serde_json::Value,
    max_tokens: u32,
) -> anyhow::Result<AiRequestOutput> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()?;
    let base_url = config.base_url.trim().trim_end_matches('/');
    let user_content = serde_json::to_string(input)?;
    let mut endpoint = format!("{base_url}/chat/completions");
    let mut response_format = None;

    let response = if config.provider == "Claude" || base_url.contains("anthropic.com") {
        endpoint = format!("{base_url}/messages");
        send_with_retry(request_label, || {
            client
                .post(&endpoint)
                .header("x-api-key", config.api_key.trim())
                .header("anthropic-version", "2023-06-01")
                .json(&serde_json::json!({
                    "model": config.model,
                    "max_tokens": max_tokens,
                    "temperature": 0,
                    "system": system_prompt,
                    "messages": [{ "role": "user", "content": user_content }]
                }))
        })
        .await?
    } else {
        let mut body = serde_json::json!({
            "model": config.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_content }
            ],
            "max_tokens": max_tokens,
            "temperature": 0
        });
        if !config.provider.contains("本地")
            && !base_url.contains("127.0.0.1")
            && !base_url.contains("localhost")
        {
            body["response_format"] = serde_json::json!({ "type": "json_object" });
            response_format = Some("json_object".to_owned());
        }
        send_with_retry(request_label, || {
            let request = client.post(&endpoint);
            let request = if config.api_key.trim().is_empty() {
                request
            } else {
                request.bearer_auth(config.api_key.trim())
            };
            let request = with_provider_headers(request, config, base_url);
            request.json(&body)
        })
        .await?
    };

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let diagnostics = build_ai_call_diagnostics(
            config,
            &endpoint,
            max_tokens,
            response_format,
            Some(status.as_u16()),
            &body,
            None,
            None,
            None,
        );
        return Err(AiCallError::new(
            format!(
                "{request_label}请求失败：{}：{}",
                status,
                truncate_text(body, 500)
            ),
            Some(diagnostics),
        )
        .into());
    }

    if config.provider == "Claude" || base_url.contains("anthropic.com") {
        let value: serde_json::Value = serde_json::from_str(&body)?;
        let text = value
            .get("content")
            .and_then(|content| content.as_array())
            .and_then(|items| {
                items
                    .iter()
                    .find_map(|item| item.get("text").and_then(|text| text.as_str()))
            })
            .unwrap_or_default();
        let diagnostics = build_ai_call_diagnostics(
            config,
            &endpoint,
            max_tokens,
            response_format,
            Some(status.as_u16()),
            &body,
            Some(text),
            value.get("stop_reason").and_then(|reason| reason.as_str()),
            value.get("usage").cloned(),
        );
        return Ok(AiRequestOutput {
            content: text.to_owned(),
            diagnostics,
        });
    }

    let value: serde_json::Value = serde_json::from_str(&body)?;
    let choice = value
        .get("choices")
        .and_then(|choices| choices.as_array())
        .and_then(|choices| choices.first());
    let message = choice.and_then(|choice| choice.get("message"));
    let content = message
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .unwrap_or_default();
    let diagnostics = build_ai_call_diagnostics(
        config,
        &endpoint,
        max_tokens,
        response_format,
        Some(status.as_u16()),
        &body,
        Some(content),
        choice
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(|reason| reason.as_str()),
        value.get("usage").cloned(),
    );
    Ok(AiRequestOutput {
        content: content.to_owned(),
        diagnostics: diagnostics.with_reasoning_from_message(message),
    })
}

impl AiCallDiagnostics {
    fn with_reasoning_from_message(mut self, message: Option<&serde_json::Value>) -> Self {
        let reasoning = message
            .and_then(|message| message.get("reasoning_content"))
            .and_then(|reasoning| reasoning.as_str());
        self.reasoning_content_present = reasoning
            .map(|reasoning| !reasoning.trim().is_empty())
            .unwrap_or(false);
        self.reasoning_content_length = reasoning.map(char_count).unwrap_or_default();
        self
    }
}

fn build_ai_call_diagnostics(
    config: &AiConfig,
    endpoint: &str,
    max_tokens: u32,
    response_format: Option<String>,
    http_status: Option<u16>,
    body: &str,
    content: Option<&str>,
    finish_reason: Option<&str>,
    usage: Option<serde_json::Value>,
) -> AiCallDiagnostics {
    let content = content.unwrap_or_default();
    AiCallDiagnostics {
        provider: config.provider.clone(),
        model: config.model.clone(),
        endpoint: endpoint.to_owned(),
        max_tokens,
        response_format,
        http_status,
        finish_reason: finish_reason.map(ToOwned::to_owned),
        content_empty: content.trim().is_empty(),
        content_length: char_count(content),
        content_snippet: diagnostic_snippet(content, 600),
        reasoning_content_present: false,
        reasoning_content_length: 0,
        response_body_length: char_count(body),
        response_body_snippet: if content.trim().is_empty() {
            diagnostic_snippet(body, 600)
        } else {
            None
        },
        usage,
    }
}

fn diagnostic_snippet(value: &str, limit: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_text(sanitize_log(trimmed), limit))
}

fn char_count(value: &str) -> usize {
    value.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostics_with_finish_reason(finish_reason: Option<&str>) -> AiCallDiagnostics {
        AiCallDiagnostics {
            provider: "火山方舟".to_owned(),
            model: "doubao-seed".to_owned(),
            endpoint: "https://ark.cn-beijing.volces.com/api/v3/chat/completions".to_owned(),
            max_tokens: 2048,
            response_format: Some("json_object".to_owned()),
            http_status: Some(200),
            finish_reason: finish_reason.map(ToOwned::to_owned),
            content_empty: false,
            content_length: 128,
            content_snippet: Some("1.1078212560161072".to_owned()),
            reasoning_content_present: true,
            reasoning_content_length: 512,
            response_body_length: 1024,
            response_body_snippet: None,
            usage: None,
        }
    }

    #[test]
    fn analysis_parse_retries_when_provider_stops_at_length() {
        let err = AiCallError::new(
            "AI返回内容不是JSON",
            Some(diagnostics_with_finish_reason(Some("length"))),
        );

        assert!(should_retry_analysis_parse(
            &err,
            &diagnostics_with_finish_reason(Some("length")),
        ));
    }

    #[test]
    fn analysis_retry_prompt_requires_compact_complete_json() {
        let prompt = analysis_retry_prompt("原始提示");

        assert!(prompt.contains("顶层只能包含 actionItems"));
        assert!(prompt.contains("{\"actionItems\":[]}"));
        assert!(prompt.contains("必须一次性闭合完整 JSON"));
    }
}

async fn send_with_retry<F>(
    request_label: &str,
    mut build_request: F,
) -> anyhow::Result<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let not_sent_prefix = format!("{request_label}请求未发出");
    let mut last_error = None;
    for attempt in 0..3 {
        match build_request().send().await {
            Ok(response) => return Ok(response),
            Err(err) if is_retryable_request_error(&err) && attempt < 2 => {
                last_error = Some(err);
                tokio::time::sleep(std::time::Duration::from_millis(600 * (attempt + 1) as u64))
                    .await;
            }
            Err(err) => {
                return Err(anyhow::anyhow!(describe_request_error(
                    &not_sent_prefix,
                    err
                )));
            }
        }
    }

    Err(anyhow::anyhow!(describe_request_error(
        &not_sent_prefix,
        last_error.expect("retry loop stores the last request error")
    )))
}
