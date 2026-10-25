pub(super) fn ai_error_category(
    error: &str,
    diagnostic: Option<&serde_json::Value>,
) -> &'static str {
    let normalized = error.to_ascii_lowercase();
    if is_response_format_compat_error(&normalized, diagnostic) {
        return "response_format_unsupported";
    }
    match diagnostic
        .and_then(|d| d.get("httpStatus"))
        .and_then(|s| s.as_u64())
    {
        Some(401 | 403) => return "auth",
        Some(429) => return "rate_limited",
        Some(404) => return "model_limit",
        Some(500..=599) => return "api_http",
        _ => {}
    }
    if normalized.contains("api key") || normalized.contains("401") || normalized.contains("403") {
        return "auth";
    }
    if normalized.contains("429") || normalized.contains("rate limit") {
        return "rate_limited";
    }
    if normalized.contains("404") || normalized.contains("model") && normalized.contains("not") {
        return "model_limit";
    }
    if normalized.contains("timeout") || normalized.contains("超时") {
        return "network_timeout";
    }
    if normalized.contains("tls")
        || normalized.contains("unexpected-eof")
        || normalized.contains("close_notify")
    {
        return "tls_closed_early";
    }
    if normalized.contains("dns")
        || normalized.contains("connect")
        || normalized.contains("连接失败")
    {
        return "network_connect";
    }
    if normalized.contains("json") || normalized.contains("解析失败") {
        return "response_parse";
    }
    if normalized.contains("为空") || normalized.contains("empty") {
        return "response_empty";
    }
    if let Some(diagnostic) = diagnostic {
        if diagnostic
            .get("contentEmpty")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return "response_empty";
        }
        if diagnostic
            .get("finishReason")
            .and_then(|value| value.as_str())
            .is_some_and(|reason| reason == "length")
        {
            return "model_limit";
        }
        if let Some(status) = diagnostic
            .get("httpStatus")
            .and_then(|value| value.as_u64())
        {
            return match status {
                401 | 403 => "auth",
                429 | 500..=599 => "api_http",
                _ => "api_http",
            };
        }
    }
    "unknown"
}

fn is_response_format_compat_error(
    normalized_error: &str,
    diagnostic: Option<&serde_json::Value>,
) -> bool {
    if normalized_error.contains("response_format")
        && (normalized_error.contains("not supported")
            || normalized_error.contains("not valid")
            || normalized_error.contains("unsupported"))
    {
        return true;
    }
    diagnostic.is_some_and(|diagnostic| {
        let response_format = diagnostic
            .get("responseFormat")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let body = diagnostic
            .get("responseBodySnippet")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        response_format == "json_object"
            && body.contains("response_format")
            && (body.contains("not supported")
                || body.contains("not valid")
                || body.contains("unsupported"))
    })
}

fn ai_user_message(category: &str) -> String {
    match category {
        "auth" => "AI服务鉴权失败，请检查API Key、模型权限或账号额度。",
        "model_limit" => "当前模型不可用，请检查模型名称或账号是否有调用权限。",
        "response_format_unsupported" => {
            "当前模型不支持强制JSON输出，已改用兼容模式重试；如仍失败请更换支持JSON输出的模型。"
        }
        "rate_limited" => "AI请求触发限流，请稍后重试。",
        "api_http" => "AI服务返回错误，错误分类和结构化状态已写入诊断包。",
        "network_timeout" => "AI请求超时，请检查网络、代理或服务商响应速度。",
        "network_connect" => "AI服务连接失败，请检查DNS、代理/VPN、防火墙或公司网络策略。",
        "tls_closed_early" => "AI连接被对端或中间网络提前断开，可稍后重试或切换网络/代理。",
        "response_empty" => "AI返回内容为空，请检查模型输出限制或服务商兼容性。",
        "response_parse" => "AI返回内容不是可解析的JSON，错误分类和结构化状态已写入诊断包。",
        _ => "发生未知错误，错误分类和结构化状态已写入诊断包。",
    }
    .to_owned()
}

pub(super) fn ai_user_message_for_error(
    category: &str,
    error: &str,
    diagnostic: Option<&serde_json::Value>,
) -> String {
    if category == "network_connect" && references_local_ai_endpoint(error, diagnostic) {
        return "本地DeepSeek推理服务已断开；系统已尝试自动恢复，如仍失败请检查可用内存并导出诊断包。"
            .to_owned();
    }
    ai_user_message(category)
}

pub(super) fn references_local_ai_endpoint(
    error: &str,
    diagnostic: Option<&serde_json::Value>,
) -> bool {
    let error = error.to_ascii_lowercase();
    if error.contains("127.0.0.1:11434") || error.contains("localhost:11434") {
        return true;
    }
    diagnostic.is_some_and(|diagnostic| {
        diagnostic
            .get("endpoint")
            .and_then(|value| value.as_str())
            .is_some_and(|endpoint| {
                let endpoint = endpoint.to_ascii_lowercase();
                endpoint.contains("127.0.0.1:11434") || endpoint.contains("localhost:11434")
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_http_and_format_failures_before_empty_content() {
        for (status, expected) in [
            (401, "auth"),
            (403, "auth"),
            (404, "model_limit"),
            (429, "rate_limited"),
            (500, "api_http"),
        ] {
            assert_eq!(
                ai_error_category(
                    "",
                    Some(&serde_json::json!({"httpStatus":status,"contentEmpty":true}))
                ),
                expected
            );
        }
        assert_eq!(
            ai_error_category(
                "",
                Some(
                    &serde_json::json!({"httpStatus":400,"responseFormat":"json_object","responseBodySnippet":"response_format json_object is not supported"})
                )
            ),
            "response_format_unsupported"
        );
    }
}
