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
