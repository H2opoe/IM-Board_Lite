//! Only structured, explicitly allowed metadata may enter persistent diagnostics.
//! Provider error bodies are useful for classification in memory, never for export.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};

pub fn diagnostic_metadata(value: Value) -> Value {
    let Value::Object(object) = value else {
        return serde_json::json!({});
    };
    let mut clean = Map::new();
    for (key, value) in object {
        let allowed_number = matches!(
            key.as_str(),
            "durationMs"
                | "maxTokens"
                | "httpStatus"
                | "contentLength"
                | "reasoningContentLength"
                | "responseBodyLength"
                | "analysisInputTokenBudget"
                | "estimatedRequestTokens"
                | "estimatedMessageTokens"
                | "estimatedContextTokens"
                | "overlapMessageCount"
                | "inputMessageCount"
                | "batchIndex"
                | "totalBatches"
                | "prompt_tokens"
                | "completion_tokens"
                | "total_tokens"
                | "cached_tokens"
                | "reasoning_tokens"
                | "exitCode"
                | "retryCount"
                | "timeoutSeconds"
        );
        let allowed_bool = matches!(
            key.as_str(),
            "contentEmpty"
                | "reasoningContentPresent"
                | "partial"
                | "errorPresent"
                | "cleanupConfirmed"
        );
        if allowed_number && (value.is_number() || value.is_null())
            || allowed_bool && value.is_boolean()
        {
            clean.insert(key, value);
        } else if matches!(
            key.as_str(),
            "call" | "diagnostic" | "usage" | "prompt_tokens_details" | "completion_tokens_details"
        ) {
            clean.insert(key, diagnostic_metadata(value));
        } else if allowed_string(&key, &value) {
            clean.insert(key, value);
        }
    }
    Value::Object(clean)
}

fn allowed_string(key: &str, value: &Value) -> bool {
    match key {
        "finishReason" => value.as_str().is_some_and(|v| {
            matches!(
                v,
                "stop" | "length" | "tool_calls" | "content_filter" | "function_call"
            )
        }),
        "lifecyclePhase" => value
            .as_str()
            .is_some_and(|v| matches!(v, "exit_requested" | "exit")),
        "requestKind" => value
            .as_str()
            .is_some_and(|v| matches!(v, "analysis" | "summary" | "analyze" | "summarize")),
        _ => false,
    }
}

/// Scrub old diagnostic rows once, including previously retained failed bodies.
/// Does not touch user messages, tasks, credentials or account configuration.
pub fn scrub_legacy_diagnostics(conn: &mut Connection) -> anyhow::Result<()> {
    let done: Option<String> = conn
        .query_row(
            "select value from app_meta where key = 'diagnostic_privacy_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if done.as_deref() == Some("1") {
        return Ok(());
    }
    let tx = conn.transaction()?;
    for (table, column) in [
        ("ai_analysis_runs", "diagnostic_json"),
        ("ai_analysis_runs", "token_usage_json"),
        ("diagnostic_error_events", "raw_detail_json"),
        ("diagnostic_error_events", "context_json"),
    ] {
        let rows = {
            let mut stmt = tx.prepare(&format!(
                "select id, {column} from {table} where {column} is not null"
            ))?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for (id, raw) in rows {
            let value = diagnostic_metadata(serde_json::from_str(&raw).unwrap_or(Value::Null));
            tx.execute(
                &format!("update {table} set {column} = ?1 where id = ?2"),
                params![value.to_string(), id],
            )?;
        }
    }
    tx.execute("update ai_analysis_runs set error = '分析失败；历史响应详情已为隐私保护移除。' where error is not null", [])?;
    tx.execute("update diagnostic_error_events set user_message = '请参考错误分类；历史自由文本详情已移除。'", [])?;
    tx.execute("insert into app_meta(key,value,updated_at) values('diagnostic_privacy_version','1',datetime('now')) on conflict(key) do update set value='1',updated_at=excluded.updated_at", [])?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_uses_a_typed_allowlist_for_every_status() {
        let value = diagnostic_metadata(serde_json::json!({
            "contentSnippet":"PRIVATE_CHAT", "responseBodySnippet":"PRIVATE_BODY",
            "unknown":{"content":"PRIVATE_CHAT"}, "maxTokens":"PRIVATE_CHAT",
            "httpStatus":500, "contentEmpty":false, "finishReason":"PRIVATE_CHAT",
            "call":{"contentSnippet":"PRIVATE_CHAT","contentLength":42},
            "usage":{"total_tokens":12,"custom":"SECRET_KEY"}
        }));
        assert_eq!(
            value,
            serde_json::json!({"httpStatus":500,"contentEmpty":false,"call":{"contentLength":42},"usage":{"total_tokens":12}})
        );
    }
    #[test]
    fn exit_metadata_allows_only_typed_failure_status() {
        for phase in ["exit_requested", "exit"] {
            assert_eq!(
                diagnostic_metadata(serde_json::json!({
                    "lifecyclePhase": phase, "cleanupConfirmed": false,
                    "cleanup_error": "PRIVATE_ERROR_DETAIL"
                })),
                serde_json::json!({"lifecyclePhase": phase, "cleanupConfirmed": false})
            );
        }
        for rejected in [
            serde_json::json!({"lifecyclePhase": "PRIVATE_ERROR_DETAIL", "cleanupConfirmed": "false"}),
            serde_json::json!({"lifecyclePhase": false, "cleanupConfirmed": 0}),
            serde_json::json!({"lifecyclePhase": null, "cleanupConfirmed": null}),
        ] {
            assert_eq!(diagnostic_metadata(rejected), serde_json::json!({}));
        }
    }
    #[test]
    fn legacy_failed_rows_are_scrubbed_and_upgrade_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .unwrap();
        conn.execute("insert into ai_analysis_runs(id,day,profile_id,status,error,diagnostic_json,created_at) values('r','2026-09-29','p','failed','PRIVATE_CHAT',?1,datetime('now'))",params![r#"{"call":{"contentSnippet":"PRIVATE_CHAT","httpStatus":500}}"#]).unwrap();
        scrub_legacy_diagnostics(&mut conn).unwrap();
        scrub_legacy_diagnostics(&mut conn).unwrap();
        let (error, json): (String, String) = conn
            .query_row(
                "select error,diagnostic_json from ai_analysis_runs",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(!error.contains("PRIVATE_CHAT"));
        assert!(!json.contains("PRIVATE_CHAT"));
        assert!(json.contains("500"));
    }
}
