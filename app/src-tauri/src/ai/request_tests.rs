#[cfg(test)]
mod tests {
    use super::*;

    fn ai_config(provider: &str, base_url: &str, model: &str) -> AiConfig {
        AiConfig {
            provider: provider.to_owned(),
            api_key: String::new(),
            base_url: base_url.to_owned(),
            model: model.to_owned(),
            user_prompt: String::new(),
            analysis_prompt: DEFAULT_ANALYSIS_PROMPT.to_owned(),
            summary_prompt: DEFAULT_SUMMARY_PROMPT.to_owned(),
            analysis_prompt_custom: false,
            summary_prompt_custom: false,
            analysis_batch_size: OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE,
            enabled: true,
            test_status: "untested".to_owned(),
        }
    }

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

    #[test]
    fn detects_response_format_compatibility_error() {
        let body = r#"{"error":{"message":"The parameter `response_format.type` specified in the request are not valid: `json_object` is not supported by this model."}}"#;

        assert!(is_response_format_unsupported(body));
        assert!(!is_response_format_unsupported(
            r#"{"error":{"message":"The model does not exist"}}"#
        ));
    }

    #[test]
    fn disables_thinking_for_volcengine_doubao_models() {
        assert!(should_disable_thinking(
            &ai_config(
                "火山方舟",
                "https://ark.cn-beijing.volces.com/api/v3",
                "doubao-seed-1-6-flash-250828"
            ),
            "https://ark.cn-beijing.volces.com/api/v3"
        ));
        assert!(should_disable_thinking(
            &ai_config(
                "OpenAI Compatible",
                "https://example.com/v1",
                "doubao-seed-1-6"
            ),
            "https://example.com/v1"
        ));
    }

    #[test]
    fn keeps_thinking_field_off_for_unrelated_openai_compatible_models() {
        assert!(!should_disable_thinking(
            &ai_config("OpenAI", "https://api.openai.com/v1", "gpt-4.1-mini"),
            "https://api.openai.com/v1"
        ));
    }
}
