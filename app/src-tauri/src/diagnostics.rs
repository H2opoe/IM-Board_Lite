use rusqlite::{params, Connection};
use serde_json::json;
use uuid::Uuid;

use crate::bridge_runner::{BridgeEnvelope, BridgeRequest};
use crate::security::sanitize_log;
use crate::storage::{models::ImProfile, AppState};

const RAW_DETAIL_LIMIT: usize = 2_000;
const CONTEXT_LIMIT: usize = 1_200;

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
            source: if platform == "wechat" {
                "bridge"
            } else {
                "cli"
            },
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
    let normalized = error.to_ascii_lowercase();
    let category = if normalized.contains("missing") || normalized.contains("not found") {
        "missing_runtime"
    } else if normalized.contains("download")
        || normalized.contains("install")
        || normalized.contains("update")
    {
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
        source: if platform == "wechat" {
            "bridge"
        } else {
            "cli"
        },
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
    if normalized.contains("timeout") || normalized.contains("timed out") {
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
    if code.contains("CRASHED") || code.contains("CLI_FAILED") || code.contains("DECRYPT_FAILED") {
        return "cli_failed";
    }
    "unknown"
}

fn bridge_user_message(platform: &str, code: &str, category: &str) -> String {
    let label = platform_label(platform);
    match code {
        "WECHAT_SIGN_PERMISSION_DENIED" | "WECHAT_SIGN_FAILED" => {
            "微信签名被macOS权限拦截，请到系统设置>隐私与安全性>App管理，允许IM-Board修改App。"
                .to_owned()
        }
        "WECHAT_KEYS_EMPTY" => "微信当前未登录，请先在微信窗口完成登录后重试。".to_owned(),
        "WECHAT_NOT_LOGGED_IN" => "微信读取不到消息，请确认电脑微信是否已登录后重试。".to_owned(),
        "WECHAT_RESTART_REQUIRED" => {
            "微信需要重启后才能继续绑定，请重新打开微信并登录后重试。".to_owned()
        }
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
        _ if category == "runtime_incomplete" => {
            format!("{label}CLI运行时不完整或版本不匹配，请重新准备CLI后再同步。")
        }
        "WECHAT_DECRYPT_FAILED" => {
            "微信数据库解密失败，请重新运行绑定命令刷新密钥后再同步。".to_owned()
        }
        _ if category == "launch_failed" => {
            format!("{label}CLI启动失败，真实启动错误已写入诊断包。")
        }
        _ if category == "api_http" => {
            format!("{label}接口返回错误，真实接口响应已写入诊断包。")
        }
        _ if category == "network_timeout" => {
            format!("{label}CLI请求超时，请检查网络、代理/VPN或稍后重试。")
        }
        _ if category == "cli_failed" => {
            format!("{label}CLI执行失败，真实返回内容已写入诊断包。")
        }
        _ => "发生未知错误，真实错误信息已写入诊断包。".to_owned(),
    }
}

pub(crate) fn ai_error_event(
    profile_id: Option<String>,
    operation: impl AsRef<str>,
    error: &str,
    diagnostic: Option<serde_json::Value>,
) -> DiagnosticErrorEvent {
    let category = ai_error_category(error, diagnostic.as_ref());
    let operation = operation.as_ref();
    DiagnosticErrorEvent {
        source: "ai",
        category,
        severity: "error",
        profile_id,
        platform: None,
        operation: operation.to_owned(),
        user_message: ai_user_message(category),
        raw_detail: json!({ "error": error, "diagnostic": diagnostic }),
        context: json!({ "operation": operation }),
    }
}

pub(crate) fn classify_ai_user_message(
    error: &str,
    diagnostic: Option<&serde_json::Value>,
) -> String {
    ai_user_message(ai_error_category(error, diagnostic))
}

pub(crate) fn local_ai_error_event(
    operation: &str,
    error: &str,
    diagnostic: serde_json::Value,
) -> DiagnosticErrorEvent {
    DiagnosticErrorEvent {
        source: "ai_local",
        category: "local_runtime",
        severity: "error",
        profile_id: None,
        platform: None,
        operation: operation.to_owned(),
        user_message: local_ai_user_message(error),
        raw_detail: json!({ "error": error, "diagnostic": diagnostic }),
        context: json!({ "operation": operation }),
    }
}

fn ai_error_category(error: &str, diagnostic: Option<&serde_json::Value>) -> &'static str {
    let normalized = error.to_ascii_lowercase();
    if diagnostic
        .and_then(|value| value.get("httpStatus"))
        .and_then(|value| value.as_i64())
        == Some(404)
        || normalized.contains("404")
    {
        return "model_unavailable";
    }
    if normalized.contains("timeout") || normalized.contains("timed out") {
        return "network_timeout";
    }
    if normalized.contains("tls") || normalized.contains("close_notify") {
        return "tls_closed_early";
    }
    if normalized.contains("401")
        || normalized.contains("unauthorized")
        || normalized.contains("api key")
    {
        return "auth";
    }
    if normalized.contains("429") || normalized.contains("rate limit") {
        return "rate_limited";
    }
    "unknown"
}

fn ai_user_message(category: &str) -> String {
    match category {
        "model_unavailable" => "当前模型不可用，请检查模型名称或账号是否有调用权限。".to_owned(),
        "network_timeout" => "AI请求超时，请检查网络、代理或服务商响应速度。".to_owned(),
        "tls_closed_early" => {
            "AI连接被对端或中间网络提前断开，可稍后重试或切换网络/代理。".to_owned()
        }
        "auth" => "AI服务鉴权失败，请检查API Key、模型权限或账号额度。".to_owned(),
        "rate_limited" => "AI请求触发限流，请稍后重试。".to_owned(),
        _ => "AI请求失败，真实错误信息已写入诊断包。".to_owned(),
    }
}

fn local_ai_user_message(error: &str) -> String {
    if error.to_ascii_lowercase().contains("missing") {
        "本地推理运行时缺失，请重新准备内置资源后再测试。".to_owned()
    } else {
        "本地推理运行时不可用，真实错误信息已写入诊断包。".to_owned()
    }
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
        "wechat" => "微信",
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
    fn classifies_wechat_import_error_as_runtime_incomplete() {
        assert_eq!(
            bridge_error_category(
                "WECHAT_DECRYPT_FAILED",
                "ImportError: cannot import name 'stats' from 'wechat_cli.commands.stats'"
            ),
            "runtime_incomplete"
        );
        assert_eq!(
            bridge_user_message("wechat", "WECHAT_DECRYPT_FAILED", "runtime_incomplete"),
            "微信CLI运行时不完整或版本不匹配，请重新准备CLI后再同步。"
        );
    }

    #[test]
    fn classifies_wechat_not_logged_in_as_auth() {
        assert_eq!(
            bridge_error_category("WECHAT_NOT_LOGGED_IN", "微信未登录"),
            "auth"
        );
        assert_eq!(
            bridge_user_message("wechat", "WECHAT_NOT_LOGGED_IN", "auth"),
            "微信读取不到消息，请确认电脑微信是否已登录后重试。"
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
    fn redacts_sensitive_raw_detail() {
        let value = redact_diagnostic_value(json!({
            "apiKey": "sk-test",
            "message": "真实错误",
        }));
        assert_eq!(value["apiKey"], "***");
        assert_eq!(value["message"], "真实错误");
    }
}
