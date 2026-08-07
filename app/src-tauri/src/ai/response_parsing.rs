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
