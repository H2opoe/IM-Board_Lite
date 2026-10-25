mod repository;
pub(crate) use repository::{record_error_event, record_error_event_conn, try_record_error_event};
mod bridge_classification;
use bridge_classification::{bridge_error_category, bridge_user_message};
mod ai_classification;
use ai_classification::{
    ai_error_category, ai_user_message_for_error, references_local_ai_endpoint,
};
use serde_json::json;

use crate::bridge_runner::{BridgeEnvelope, BridgeRequest};
use crate::storage::{models::ImProfile, AppState};

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
    record_error_event(
        state,
        cli_lifecycle_error_event(platform, profile_id, operation, error),
    );
}

fn cli_lifecycle_error_event(
    platform: &str,
    profile_id: Option<String>,
    operation: &str,
    error: &str,
) -> DiagnosticErrorEvent {
    let normalized = error.to_ascii_lowercase();
    let category = if error.contains("尚未准备好")
        || error.contains("未找到")
        || normalized.contains("missing")
        || normalized.contains("not found")
    {
        "missing_runtime"
    } else if error.contains("下载")
        || error.contains("安装")
        || error.contains("更新")
        || normalized.contains("download")
        || normalized.contains("install")
        || normalized.contains("update")
    {
        "cli_failed"
    } else {
        "unknown"
    };
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
    }
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

fn local_ai_user_message(error: &str) -> String {
    if error.to_ascii_lowercase().contains("missing") {
        return "本地推理运行时缺失，请重新准备内置资源后再测试。".to_owned();
    }
    if error.contains("下载") {
        return "本地DeepSeek模型下载失败，错误分类和结构化状态已写入诊断包。".to_owned();
    }
    if error.contains("启动") {
        return "本地DeepSeek推理服务启动失败，错误分类和结构化状态已写入诊断包。".to_owned();
    }
    if error.contains("退出") || error.contains("崩溃") {
        return "本地DeepSeek推理服务异常退出，错误分类和结构化状态已写入诊断包。".to_owned();
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

#[cfg(test)]
#[path = "diagnostics/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "diagnostics/runtime_missing_tests.rs"]
mod runtime_missing_tests;
