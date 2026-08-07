use rusqlite::{params, Connection};
use serde_json::json;
use uuid::Uuid;

use crate::bridge_runner::{BridgeEnvelope, BridgeRequest};
use crate::security::sanitize_log;
use crate::storage::{models::ImProfile, AppState};

const RAW_DETAIL_LIMIT: usize = 2_000;
const CONTEXT_LIMIT: usize = 1_200;
const DIAGNOSTIC_RETENTION_DAYS: i64 = 90;
const DIAGNOSTIC_MAX_EVENTS: i64 = 2_000;

#[derive(Debug, Clone)]
pub(crate) struct DiagnosticErrorEvent {
    pub source: &'static str,
    pub category: &'static str,
    pub severity: &'static str,
    pub profile_id: Option<String>,
    pub platform: Option<String>,
    pub operation: String,
    pub user_message: String,
    pub raw_detail: serde_json::Value,
    pub context: serde_json::Value,
}

pub(crate) fn record_error_event(state: &AppState, event: DiagnosticErrorEvent) {
    let Ok(conn) = state.db.lock() else {
        return;
    };
    let _ = record_error_event_conn(&conn, event);
}

pub(crate) fn record_error_event_conn(
    conn: &Connection,
    event: DiagnosticErrorEvent,
) -> rusqlite::Result<()> {
    let raw_detail_json = serde_json::to_string(&redact_diagnostic_value(event.raw_detail))
        .unwrap_or_else(|_| "{}".to_owned());
    let context_json = serde_json::to_string(&redact_diagnostic_value(event.context))
        .unwrap_or_else(|_| "{}".to_owned());
    conn.execute(
        "insert into diagnostic_error_events(
           id, source, category, severity, profile_id, platform, operation,
           user_message, raw_detail_json, context_json, created_at
         )
         values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'))",
        params![
            Uuid::new_v4().to_string(),
            event.source,
            event.category,
            event.severity,
            event.profile_id,
            event.platform,
            event.operation,
            event.user_message,
            raw_detail_json,
            context_json
        ],
    )?;
    prune_diagnostic_error_events(conn)?;
    Ok(())
}

fn prune_diagnostic_error_events(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "delete from diagnostic_error_events
         where created_at < datetime('now', ?1)",
        params![format!("-{DIAGNOSTIC_RETENTION_DAYS} days")],
    )?;
    conn.execute(
        "delete from diagnostic_error_events
         where id in (
           select id
           from diagnostic_error_events
           order by created_at desc, id desc
           limit -1 offset ?1
         )",
        params![DIAGNOSTIC_MAX_EVENTS],
    )?;
    Ok(())
}

pub(crate) fn record_bridge_envelope(
    state: &AppState,
    request: &BridgeRequest,
    envelope: &BridgeEnvelope,
) {
    if envelope.ok {
        return;
    }
    record_error_event(state, bridge_error_event(request, envelope));
}

pub(crate) fn record_bridge_failure(state: &AppState, request: &BridgeRequest, error: &str) {
    let platform = request.platform.clone();
    record_error_event(
        state,
        DiagnosticErrorEvent {
            source: "cli",
            category: "unknown",
            severity: "error",
            profile_id: request.profile.as_ref().map(|profile| profile.id.clone()),
            platform: Some(platform.clone()),
            operation: request.command.clone(),
            user_message: bridge_user_message(&platform, "BRIDGE_UNKNOWN", "unknown"),
            raw_detail: json!({ "error": error }),
            context: json!({
                "platform": platform,
                "command": request.command,
                "profile": request.profile.as_ref().map(profile_context),
            }),
        },
    );
}

pub(crate) fn record_cli_lifecycle_error(
    state: &AppState,
    platform: &str,
    profile_id: Option<String>,
    operation: &str,
    error: &str,
) {
    let category = if error.contains("尚未准备好") || error.contains("未找到") {
        "missing_runtime"
    } else if error.contains("下载") || error.contains("安装") || error.contains("更新") {
        "cli_failed"
    } else {
        "unknown"
    };
    record_error_event(
        state,
        DiagnosticErrorEvent {
            source: "cli",
            category,
            severity: "error",
            profile_id,
            platform: Some(platform.to_owned()),
            operation: operation.to_owned(),
            user_message: bridge_user_message(platform, "CLI_LIFECYCLE_FAILED", category),
            raw_detail: json!({ "error": error }),
            context: json!({ "platform": platform, "operation": operation }),
        },
    );
}

fn bridge_error_event(request: &BridgeRequest, envelope: &BridgeEnvelope) -> DiagnosticErrorEvent {
    let profile = request.profile.as_ref();
    let code = envelope
        .error
        .as_ref()
        .map(|error| error.code.as_str())
        .unwrap_or("BRIDGE_UNKNOWN");
    let detail = bridge_error_detail(envelope);
    let category = bridge_error_category(code, &detail);
    let platform = request.platform.clone();
    DiagnosticErrorEvent {
        source: "cli",
        category,
        severity: "error",
        profile_id: profile.map(|profile| profile.id.clone()),
        platform: Some(platform.clone()),
        operation: request.command.clone(),
        user_message: bridge_user_message(&platform, code, category),
        raw_detail: json!({
            "code": code,
            "message": envelope.error.as_ref().map(|error| error.message.clone()),
            "warnings": envelope.warnings,
        }),
        context: json!({
            "platform": platform,
            "command": request.command,
            "profile": profile.map(profile_context),
            "meta": envelope.meta,
        }),
    }
}

fn bridge_error_detail(envelope: &BridgeEnvelope) -> String {
    let message = envelope
        .error
        .as_ref()
        .map(|error| error.message.as_str())
        .unwrap_or_default();
    [message, &envelope.warnings.join("\n")]
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn bridge_error_category(code: &str, detail: &str) -> &'static str {
    let normalized = detail.to_ascii_lowercase();
    if normalized.contains("tls handshake timeout")
        || normalized.contains("mcp请求超时")
        || normalized.contains("timeout")
        || normalized.contains("超时")
        || normalized.contains("timed out")
    {
        return "network_timeout";
    }
    if normalized.contains("importerror")
        || normalized.contains("modulenotfounderror")
        || normalized.contains("cannot import name")
        || normalized.contains("no module named")
    {
        return "runtime_incomplete";
    }
    if code.contains("NOT_AUTHENTICATED")
        || code.contains("NOT_LOGGED_IN")
        || code.contains("KEYS_EMPTY")
    {
        return "auth";
    }
    if code.contains("PERMISSION") || code.contains("UNSUPPORTED") {
        return "permission";
    }
    if code.contains("MISSING") {
        return "missing_runtime";
    }
    if code.contains("SPAWN") {
        return "launch_failed";
    }
    if code.contains("API_ERROR") {
        return "api_http";
    }
    if code.contains("CRASHED") || code.contains("CLI_FAILED") {
        return "cli_failed";
    }
    if code.contains("DECRYPT_FAILED") {
        return "cli_failed";
    }
    "unknown"
}

fn bridge_user_message(platform: &str, code: &str, category: &str) -> String {
    let label = platform_label(platform);
    match code {
        "WECOM_MESSAGE_PERMISSION_UNSUPPORTED" => {
            "当前企业或授权机器人暂不支持企业微信消息读取，无法同步企业微信消息。".to_owned()
        }
        "FEISHU_B2C_APP_UNSUPPORTED" => {
            "飞书应用或机器人会话暂不支持读取，已跳过，不影响其他会话同步。".to_owned()
        }
        "DINGTALK_MESSAGE_PERMISSION_MISSING" => {
            "钉钉缺少chat.message:list或组织未开启CLI数据访问权限，请重新授权或联系管理员开通。"
                .to_owned()
        }
        _ if category == "auth" => {
            format!("{label}尚未完成授权，请重新运行绑定命令并完成登录/扫码后再同步。")
        }
        _ if category == "permission" => {
            format!("{label}缺少消息读取权限，请检查应用权限或联系管理员开通后再同步。")
        }
        _ if category == "missing_runtime" => {
            format!("{label}CLI不可用，可能下载不完整或本地缓存损坏，请重新准备CLI。")
        }
        _ if category == "launch_failed" => {
            format!("{label}CLI启动失败，真实启动错误已写入诊断包。")
        }
        _ if category == "network_timeout" => {
            format!("{label}CLI请求超时，请检查网络、代理/VPN或稍后重试。")
        }
        _ if category == "runtime_incomplete" => {
            format!("{label}CLI运行时不完整或版本不匹配，请重新准备CLI后再同步。")
        }
        _ if category == "api_http" => format!("{label}接口返回错误，真实接口响应已写入诊断包。"),
        _ if category == "cli_failed" => format!("{label}CLI执行失败，真实返回内容已写入诊断包。"),
        _ => "发生未知错误，真实错误信息已写入诊断包。".to_owned(),
    }
}

pub(crate) fn ai_error_event(
    profile_id: Option<String>,
    operation: impl Into<String>,
    error: &str,
    diagnostic: Option<serde_json::Value>,
) -> DiagnosticErrorEvent {
    let operation = operation.into();
    let category = ai_error_category(error, diagnostic.as_ref());
    let user_message = ai_user_message_for_error(category, error, diagnostic.as_ref());
    DiagnosticErrorEvent {
        source: "ai",
        category,
        severity: "error",
        profile_id,
        platform: None,
        operation,
        user_message,
        raw_detail: json!({
            "error": error,
            "diagnostic": diagnostic,
        }),
        context: json!({}),
    }
}

pub(crate) fn classify_ai_user_message(
    error: &str,
    diagnostic: Option<&serde_json::Value>,
) -> String {
    let category = ai_error_category(error, diagnostic);
    ai_user_message_for_error(category, error, diagnostic)
}

pub(crate) fn is_local_ai_connection_failure(
    error: &str,
    diagnostic: Option<&serde_json::Value>,
) -> bool {
    ai_error_category(error, diagnostic) == "network_connect"
        && references_local_ai_endpoint(error, diagnostic)
}

pub(crate) fn local_ai_error_event(
    operation: impl Into<String>,
    error: &str,
    context: serde_json::Value,
) -> DiagnosticErrorEvent {
    let operation = operation.into();
    let category = ai_error_category(error, None);
    DiagnosticErrorEvent {
        source: "local_ai_runtime",
        category: if category == "unknown" {
            "local_runtime"
        } else {
            category
        },
        severity: "error",
        profile_id: None,
        platform: None,
        operation,
        user_message: local_ai_user_message(error),
        raw_detail: json!({ "error": error }),
        context,
    }
}

fn ai_error_category(error: &str, diagnostic: Option<&serde_json::Value>) -> &'static str {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("api key") || normalized.contains("401") || normalized.contains("403") {
        return "auth";
    }
    if normalized.contains("429") || normalized.contains("rate limit") {
        return "api_http";
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

fn ai_user_message(category: &str) -> String {
    match category {
        "auth" => "AI服务鉴权失败，请检查API Key、模型权限或账号额度。",
        "model_limit" => "当前模型不可用，请检查模型名称或账号是否有调用权限。",
        "api_http" => "AI服务返回错误，真实响应内容已写入诊断包。",
        "network_timeout" => "AI请求超时，请检查网络、代理或服务商响应速度。",
        "network_connect" => "AI服务连接失败，请检查DNS、代理/VPN、防火墙或公司网络策略。",
        "tls_closed_early" => "AI连接被对端或中间网络提前断开，可稍后重试或切换网络/代理。",
        "response_empty" => "AI返回内容为空，请检查模型输出限制或服务商兼容性。",
        "response_parse" => "AI返回内容不是可解析的JSON，真实返回片段已写入诊断包。",
        _ => "发生未知错误，真实错误信息已写入诊断包。",
    }
    .to_owned()
}

fn ai_user_message_for_error(
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

fn references_local_ai_endpoint(error: &str, diagnostic: Option<&serde_json::Value>) -> bool {
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

fn local_ai_user_message(error: &str) -> String {
    if error.contains("下载") {
        return "本地DeepSeek模型下载失败，真实下载错误已写入诊断包。".to_owned();
    }
    if error.contains("启动") {
        return "本地DeepSeek推理服务启动失败，真实启动错误已写入诊断包。".to_owned();
    }
    if error.contains("退出") || error.contains("崩溃") {
        return "本地DeepSeek推理服务异常退出，退出状态和运行日志已写入诊断包。".to_owned();
    }
    "本地推理运行时不可用，请重新准备运行时后再测试。".to_owned()
}

fn profile_context(profile: &ImProfile) -> serde_json::Value {
    json!({
        "id": profile.id,
        "platform": profile.platform,
        "label": profile.label,
        "status": profile.status,
    })
}

fn platform_label(platform: &str) -> &'static str {
    match platform {
        "wecom" => "企业微信",
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        _ => "平台",
    }
}

fn redact_diagnostic_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(redact_diagnostic_value).collect())
        }
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    if is_sensitive_key(&key) {
                        (key, json!("***"))
                    } else {
                        (key, redact_diagnostic_value(value))
                    }
                })
                .collect(),
        ),
        serde_json::Value::String(text) => {
            serde_json::Value::String(truncate_sanitized(&text, RAW_DETAIL_LIMIT))
        }
        other => other,
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "authorization",
        "password",
        "cookie",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn truncate_sanitized(value: &str, limit: usize) -> String {
    let sanitized = sanitize_log(value);
    let limit = limit.min(CONTEXT_LIMIT.max(RAW_DETAIL_LIMIT));
    if sanitized.chars().count() <= limit {
        return sanitized;
    }
    let mut output = sanitized.chars().take(limit).collect::<String>();
    output.push_str("...");
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_retention_removes_expired_and_overflow_events() {
        let conn = Connection::open_in_memory().expect("open memory db");
        conn.execute_batch(include_str!("../migrations/001_init.sql"))
            .expect("create schema");
        conn.execute(
            "insert into diagnostic_error_events(
               id, source, category, severity, operation, user_message, created_at
             ) values('expired', 'sync', 'test', 'warning', 'test', 'expired', datetime('now', '-91 days'))",
            [],
        )
        .expect("insert expired event");
        for index in 0..=DIAGNOSTIC_MAX_EVENTS {
            conn.execute(
                "insert into diagnostic_error_events(
                   id, source, category, severity, operation, user_message, created_at
                 ) values(?1, 'sync', 'test', 'warning', 'test', 'recent', datetime('now', ?2))",
                params![
                    format!("recent-{index:04}"),
                    format!("-{} seconds", DIAGNOSTIC_MAX_EVENTS - index)
                ],
            )
            .expect("insert recent event");
        }

        prune_diagnostic_error_events(&conn).expect("prune diagnostic events");

        let count: i64 = conn
            .query_row("select count(*) from diagnostic_error_events", [], |row| {
                row.get(0)
            })
            .expect("count events");
        let expired_count: i64 = conn
            .query_row(
                "select count(*) from diagnostic_error_events where id = 'expired'",
                [],
                |row| row.get(0),
            )
            .expect("count expired event");
        assert_eq!(count, DIAGNOSTIC_MAX_EVENTS);
        assert_eq!(expired_count, 0);
    }

    #[test]
    fn classifies_dingtalk_permission_without_spacing() {
        let message = bridge_user_message(
            "dingtalk",
            "DINGTALK_MESSAGE_PERMISSION_MISSING",
            "permission",
        );
        assert_eq!(
            message,
            "钉钉缺少chat.message:list或组织未开启CLI数据访问权限，请重新授权或联系管理员开通。"
        );
    }

    #[test]
    fn classifies_cli_tls_timeout_from_raw_detail() {
        assert_eq!(
            bridge_error_category(
                "DINGTALK_CLI_FAILED",
                "request failed: net/http: TLS handshake timeout"
            ),
            "network_timeout"
        );
        assert_eq!(
            bridge_user_message("dingtalk", "DINGTALK_CLI_FAILED", "network_timeout"),
            "钉钉CLI请求超时，请检查网络、代理/VPN或稍后重试。"
        );
    }

    #[test]
    fn classifies_ai_tls_early_close() {
        let event = ai_error_event(
            None,
            "analysis",
            "API请求未发出：peer closed connection without sending TLS close_notify",
            None,
        );
        assert_eq!(event.category, "tls_closed_early");
        assert_eq!(
            event.user_message,
            "AI连接被对端或中间网络提前断开，可稍后重试或切换网络/代理。"
        );
    }

    #[test]
    fn classifies_ai_404_as_model_unavailable() {
        let diagnostic = json!({
            "httpStatus": 404,
            "responseBodySnippet": "404 Not Found",
        });
        assert_eq!(
            classify_ai_user_message(
                "API返回错误：404 Not Found：404 Not Found",
                Some(&diagnostic)
            ),
            "当前模型不可用，请检查模型名称或账号是否有调用权限。"
        );
    }

    #[test]
    fn classifies_local_ai_connection_failure_without_network_advice() {
        let error = "待回复和待办事项识别请求未发出：error sending request for url (http://127.0.0.1:11434/v1/chat/completions)：原因：client error (Connect)";
        assert!(is_local_ai_connection_failure(error, None));
        assert_eq!(
            classify_ai_user_message(error, None),
            "本地DeepSeek推理服务已断开；系统已尝试自动恢复，如仍失败请检查可用内存并导出诊断包。"
        );
    }

    #[test]
    fn keeps_remote_ai_connection_failure_network_advice() {
        let error = "API请求未发出：error sending request for url (https://api.example.com/v1/chat/completions)：原因：client error (Connect)";
        assert!(!is_local_ai_connection_failure(error, None));
        assert_eq!(
            classify_ai_user_message(error, None),
            "AI服务连接失败，请检查DNS、代理/VPN、防火墙或公司网络策略。"
        );
    }

    #[test]
    fn redacts_sensitive_raw_detail() {
        let value = redact_diagnostic_value(json!({
            "apiKey": "sk-test",
            "message": "真实错误",
        }));
        assert_eq!(value["apiKey"], "***");
        assert_eq!(value["message"], "真实错误");
    }
}
