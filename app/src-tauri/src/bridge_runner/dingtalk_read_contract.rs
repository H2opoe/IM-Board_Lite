//! Pinned DWS 1.0.65 read contract. All time-window reads avoid name discovery.
use super::super::normalizers::dingtalk_time_arg;
use super::super::read_result::ReadProblem;
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn read_args(kind: &str, args: &HashMap<String, String>) -> Result<Vec<String>, String> {
    let mut result = vec![
        "chat".into(),
        match kind {
            "list-chats" => "+recent-conversations",
            "fetch-messages" => "+chat-messages",
            "search-messages" => "+search-msg",
            _ => return Err("不支持的钉钉读取命令。".into()),
        }
        .into(),
        "--format".into(),
        "json".into(),
    ];
    for (input, flag) in [("start_time", "--start"), ("end_time", "--end")] {
        let time = args
            .get(input)
            .and_then(|value| dingtalk_time_arg(value))
            .ok_or_else(|| {
                format!("钉钉读取缺少有效的{input}，已停止，避免查询错误的时间范围。")
            })?;
        result.extend([flag.into(), time]);
    }
    let limit = args
        .get("limit")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(100)
        .clamp(1, 100);
    result.extend([
        "--limit".into(),
        limit.to_string(),
        "--page-limit".into(),
        "40".into(),
    ]);
    if kind != "list-chats" {
        result.extend(["--page-all".into(), "--order".into(), "asc".into()]);
    }
    if kind == "fetch-messages" {
        let id = args
            .get("chat")
            .filter(|id| !id.trim().is_empty())
            .ok_or("钉钉读取缺少会话ID。")?;
        let direct = args
            .get("chat_type")
            .is_some_and(|v| matches!(v.as_str(), "direct" | "single" | "p2p" | "1"));
        // A conversation ID is not a user ID. Scoped message search supports either
        // conversation type without resolving a contact by name.
        if direct {
            return Err("单聊请使用按时间直接读取消息，不能把会话ID当作用户ID。".into());
        }
        result.extend(["--group".into(), id.clone()]);
    }
    if let Some(cursor) = args.get("cursor").filter(|v| !v.trim().is_empty()) {
        result.extend(["--cursor".into(), cursor.clone()]);
    }
    Ok(result)
}

pub(super) fn complete_payload<'a>(raw: &'a Value, kind: &str) -> Result<&'a Value, ReadProblem> {
    let payload = raw.get("data").filter(|v| v.is_object()).unwrap_or(raw);
    let field = if kind == "list-chats" {
        "conversations"
    } else {
        "messages"
    };
    super::super::read_result::named_collection(payload, field)
        .and_then(|_| super::super::read_result::ensure_complete(payload))?;
    if payload.get("complete").and_then(Value::as_bool) != Some(true) {
        return Err(ReadProblem::Incomplete);
    }
    Ok(payload)
}

pub(super) fn failure_summary_from_output(stdout: &str, stderr: &str) -> String {
    // DWS writes API error envelopes to stderr, including after a partial stdout.
    for output in [stderr, stdout] {
        if serde_json::from_str::<Value>(output)
            .ok()
            .is_some_and(|raw| raw.get("error").is_some_and(|error| error.is_object()))
        {
            return failure_summary(output);
        }
    }
    failure_summary("")
}

pub(super) fn failure_summary(stdout: &str) -> String {
    serde_json::from_str::<Value>(stdout)
        .ok()
        .and_then(|raw| raw.get("error").cloned())
        .map(|error| {
            let fields = [
                "category",
                "code",
                "message",
                "reason",
                "server_error_code",
                "server_key",
                "technical_detail",
                "friendly_hint",
                "action_url",
            ]
            .into_iter()
            .filter_map(|key| {
                error
                    .get(key)
                    .filter(|value| value.is_string() || value.is_number())
                    .map(|value| (key.to_owned(), value.clone()))
            })
            .collect::<serde_json::Map<String, Value>>();
            if fields.is_empty() {
                return String::new();
            }
            crate::security::sanitize_log(&serde_json::json!({"error": fields}).to_string())
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            "钉钉读取失败或结果不完整；未导入本次结果，请检查账号权限或重试。".to_owned()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_does_not_include_partial_message_bodies() {
        let text = failure_summary(
            r#"{"error":{"code":"partial","message":"fixture page failed"},"data":{"messages":[{"text":"private body fixture"}]}}"#,
        );
        assert!(text.contains("fixture page failed"));
        assert!(!text.contains("private body fixture"));
        assert!(!failure_summary("private body fixture").contains("private body fixture"));
    }
    #[test]
    fn permission_error_metadata_survives_read_failure_summary() {
        let raw = serde_json::json!({
            "error": {"category":"api", "code":1, "reason":"business_error",
                "server_key":"group-chat", "server_error_code":"1001",
                "message":"forbidden request", "details":{"partialResult":{"messages":[{"text":"private body"}]}}},
            "data":{"messages":[{"text":"private body"}]}
        });
        let summary = failure_summary_from_output("", &raw.to_string());
        let error = super::super::super::errors::classify_dingtalk_cli_error(&summary, "").unwrap();
        assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
        assert!(!summary.contains("private body"));
        let partial = r#"{"messages":[{"text":"private body"}]}"#;
        assert_eq!(
            failure_summary_from_output(partial, &raw.to_string()),
            summary
        );
        assert!(failure_summary(r#"{"error":{"category":"api","code":1,"message":"下游缺少明确的集合字段，无法判定结果为空"}}"#).contains("下游缺少明确的集合字段"));
    }
    #[test]
    fn direct_search_needs_no_contacts_or_group_names_and_uses_rfc3339() {
        let args = HashMap::from([
            ("start_time".into(), "2026-10-24T00:00:00+08:00".into()),
            ("end_time".into(), "2026-10-25T00:00:00+08:00".into()),
            ("limit".into(), "200".into()),
        ]);
        let built = read_args("search-messages", &args).unwrap();
        assert_eq!(built[1], "+search-msg");
        assert!(built.contains(&"--page-all".into()));
        assert!(built.windows(2).any(|pair| pair == ["--limit", "100"]));
        assert!(!built
            .iter()
            .any(|arg| matches!(arg.as_str(), "--query" | "--group" | "--senders")));
        let start = &built[built.iter().position(|arg| arg == "--start").unwrap() + 1];
        assert!(chrono::DateTime::parse_from_rfc3339(start).is_ok());
    }
    #[test]
    fn missing_time_never_silently_uses_a_default_window() {
        assert!(read_args("search-messages", &HashMap::new()).is_err());
    }
    #[test]
    fn empty_complete_is_valid_but_missing_partial_and_failed_ledgers_are_rejected() {
        assert!(complete_payload(
            &serde_json::json!({"data":{"messages":[],"complete":true,"hasMore":false}}),
            "search-messages"
        )
        .is_ok());
        for value in [
            serde_json::json!({}),
            serde_json::json!({"messages":[],"complete":false}),
            serde_json::json!({"messages":[],"complete":true,"hasMore":true}),
            serde_json::json!({"messages":[],"complete":true,"failures":[{"stage":"enrich"}]}),
            serde_json::json!({"messages":[],"complete":true,"decryptFailedCount":1}),
        ] {
            assert!(complete_payload(&value, "search-messages").is_err());
        }
    }
}
