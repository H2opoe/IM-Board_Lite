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

    let (status, body, response_format) = if config.provider == "Claude"
        || base_url.contains("anthropic.com")
    {
        endpoint = format!("{base_url}/messages");
        let response = send_with_retry(request_label, || {
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
        .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        (status, body, response_format)
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
        if should_disable_thinking(config, base_url) {
            body["thinking"] = serde_json::json!({ "type": "disabled" });
        }
        if !config.provider.contains("本地")
            && !base_url.contains("127.0.0.1")
            && !base_url.contains("localhost")
        {
            body["response_format"] = serde_json::json!({ "type": "json_object" });
            response_format = Some("json_object".to_owned());
        }
        let build_request = |body: &serde_json::Value| {
            let request = client.post(&endpoint);
            let request = if config.api_key.trim().is_empty() {
                request
            } else {
                request.bearer_auth(config.api_key.trim())
            };
            let request = with_provider_headers(request, config, base_url);
            request.json(&body)
        };
        let response = send_with_retry(request_label, || build_request(&body)).await?;
        let mut status = response.status();
        let mut response_body = response.text().await.unwrap_or_default();

        if !status.is_success()
            && response_format.is_some()
            && is_response_format_unsupported(&response_body)
        {
            // 部分 OpenAI 兼容模型不支持 response_format=json_object，但仍能按提示返回 JSON。
            // 这里降级重试一次，避免热门话题/待办识别因提供商兼容差异整批失败。
            if let Some(body) = body.as_object_mut() {
                body.remove("response_format");
            }
            response_format = None;
            let retry_response = send_with_retry(request_label, || build_request(&body)).await?;
            status = retry_response.status();
            response_body = retry_response.text().await.unwrap_or_default();
        }

        (status, response_body, response_format)
    };

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
fn is_response_format_unsupported(body: &str) -> bool {
    let normalized = body.to_ascii_lowercase();
    normalized.contains("response_format")
        && (normalized.contains("not supported")
            || normalized.contains("not valid")
            || normalized.contains("unsupported"))
}

fn should_disable_thinking(config: &AiConfig, base_url: &str) -> bool {
    let provider = config.provider.to_ascii_lowercase();
    let model = config.model.to_ascii_lowercase();
    let base_url = base_url.to_ascii_lowercase();
    provider.contains("火山")
        || provider.contains("volc")
        || base_url.contains("volces.com")
        || base_url.contains("volcengine.com")
        || model.contains("doubao-seed")
}
