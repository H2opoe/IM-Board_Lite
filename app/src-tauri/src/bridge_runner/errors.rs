use std::time::Instant;

use crate::security::sanitize_log;

use super::{BridgeEnvelope, BridgeError};

pub(super) fn classify_wecom_cli_error(detail: &str) -> Option<BridgeError> {
    let normalized = detail.to_ascii_lowercase();
    if normalized.contains("not authenticated") || detail.contains("401") {
        return Some(BridgeError {
            code: "WECOM_NOT_AUTHENTICATED".to_owned(),
            message: not_authenticated_message("企业微信"),
            recoverable: true,
        });
    }
    if detail.contains("暂不支持授权机器人") && detail.contains("消息") {
        return Some(BridgeError {
            code: "WECOM_MESSAGE_PERMISSION_UNSUPPORTED".to_owned(),
            message: "当前企业或授权机器人暂不支持企业微信「消息」权限；IM看板需要读取会话列表和聊天记录，因此无法同步企业微信消息。请在企业微信管理后台确认API模式智能机器人是否开放消息能力，或更换支持消息权限的企业/机器人后重新绑定。".to_owned(),
            recoverable: true,
        });
    }
    None
}

pub(super) fn not_authenticated_message(platform_label: &str) -> String {
    format!(
        "{platform_label}尚未完成授权。请复制{platform_label}绑定命令，在{}运行并完成扫码后再测试读取。",
        platform_command_shell_name()
    )
}

pub(super) fn platform_command_shell_name() -> &'static str {
    "Windows PowerShell"
}

pub(super) fn feishu_app_config_incomplete_message() -> String {
    format!(
        "飞书尚未完成授权。请复制飞书绑定命令，在{}运行，并按提示完成应用配置和用户授权后再测试读取。",
        platform_command_shell_name()
    )
}

pub(super) fn feishu_user_auth_incomplete_message() -> String {
    format!(
        "授权不完整，飞书存在2次授权（应用配置&用户授权），请留意{}的提示重试。",
        platform_command_shell_name()
    )
}

pub(super) fn validate_feishu_auth_status(raw: &serde_json::Value) -> Option<BridgeError> {
    let app_configured = raw
        .get("appId")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.trim().is_empty())
        || raw
            .get("brand")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty());
    let verified = raw
        .get("verified")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let token_valid = raw
        .get("tokenStatus")
        .and_then(|value| value.as_str())
        .is_some_and(|value| value == "valid");
    let scope = raw
        .get("scope")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let required_scopes = [
        "search:message",
        "im:chat:read",
        "im:message:readonly",
        "im:message.p2p_msg:get_as_user",
        "im:message.group_msg:get_as_user",
        "contact:user.base:readonly",
        "contact:user.basic_profile:readonly",
    ];
    let has_required_scopes = required_scopes
        .iter()
        .all(|required| scope.split_whitespace().any(|granted| granted == *required));
    if verified && token_valid && has_required_scopes {
        return None;
    }
    Some(BridgeError {
        code: "FEISHU_NOT_AUTHENTICATED".to_owned(),
        message: if app_configured {
            feishu_user_auth_incomplete_message()
        } else {
            feishu_app_config_incomplete_message()
        },
        recoverable: true,
    })
}

pub(super) fn classify_feishu_cli_error(stdout: &str, stderr: &str) -> Option<BridgeError> {
    let detail = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let raw = serde_json::from_str::<serde_json::Value>(stdout).ok();
    let reason = raw
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("reason"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let error_type = raw
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("type"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let error_code = raw
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("code"))
        .and_then(|value| {
            value
                .as_i64()
                .map(|code| code.to_string())
                .or_else(|| value.as_str().map(str::to_owned))
        })
        .unwrap_or_default();
    let error_message = raw
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("message"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let normalized_detail = format!("{detail}\n{error_message}").to_ascii_lowercase();
    if error_code == "231204"
        || normalized_detail.contains("b2c app not support")
        || normalized_detail.contains("app type is not supported")
    {
        return Some(BridgeError {
            code: "FEISHU_B2C_APP_UNSUPPORTED".to_owned(),
            message: "该会话是飞书应用/机器人会话，飞书官方接口返回231204（b2c app not support），当前CLI不能用用户身份读取这类会话历史；已跳过，不影响其他会话同步。".to_owned(),
            recoverable: true,
        });
    }
    if reason == "not_authenticated"
        || error_message == "not configured"
        || error_type == "config"
        || detail.contains("not_authenticated")
        || detail.contains("not configured")
        || detail.contains("not logged in")
        || detail.contains("未登录")
    {
        return Some(BridgeError {
            code: "FEISHU_NOT_AUTHENTICATED".to_owned(),
            message: feishu_app_config_incomplete_message(),
            recoverable: true,
        });
    }
    None
}

pub(super) fn parse_feishu_cli_json(stdout: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(stdout.trim())
        .ok()
        .or_else(|| {
            let cleaned = strip_feishu_cli_progress_lines(stdout);
            serde_json::from_str::<serde_json::Value>(cleaned.trim()).ok()
        })
}

pub(super) fn sanitize_feishu_cli_output(value: &str) -> String {
    strip_feishu_cli_progress_lines(&sanitize_log(value))
}

pub(super) fn strip_feishu_cli_progress_lines(value: &str) -> String {
    value
        .lines()
        .filter(|line| !is_feishu_cli_progress_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn is_feishu_cli_progress_line(line: &str) -> bool {
    let Some(rest) = line.trim().strip_prefix("[page ") else {
        return false;
    };
    let Some((page, message)) = rest.split_once(']') else {
        return false;
    };
    let page = page.trim();
    if page.is_empty() || !page.chars().all(|value| value.is_ascii_digit()) {
        return false;
    }
    let message = message.trim().to_ascii_lowercase();
    matches!(message.as_str(), "fetching..." | "fetching") || message.starts_with("fetched ")
}

pub(super) fn classify_dingtalk_cli_error(stdout: &str, stderr: &str) -> Option<BridgeError> {
    let detail = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let compact_detail = detail.split_whitespace().collect::<String>();
    let raw = serde_json::from_str::<serde_json::Value>(stdout).ok();
    let error = raw.as_ref().and_then(|value| value.get("error"));
    let reason = raw
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("reason"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let code = error
        .and_then(|value| value.get("code"))
        .or_else(|| raw.as_ref().and_then(|value| value.get("code")));
    let code_text = code
        .and_then(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .or_else(|| value.as_i64().map(|number| number.to_string()))
        })
        .unwrap_or_default();
    let category = error
        .and_then(|value| value.get("category"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let server_error_code = error
        .and_then(|value| value.get("server_error_code"))
        .and_then(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .or_else(|| value.as_i64().map(|number| number.to_string()))
        })
        .unwrap_or_default();
    let server_key = error
        .and_then(|value| value.get("server_key"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let action_url = error
        .and_then(|value| value.get("action_url"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let nested_code = raw
        .as_ref()
        .and_then(|value| value.pointer("/error/data/code"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if reason == "not_authenticated"
        || detail.contains("not_authenticated")
        || detail.contains("未登录")
    {
        return Some(BridgeError {
            code: "DINGTALK_NOT_AUTHENTICATED".to_owned(),
            message: not_authenticated_message("钉钉"),
            recoverable: true,
        });
    }
    if code_text == "PAT_MEDIUM_RISK_NO_PERMISSION"
        || nested_code == "PAT_MEDIUM_RISK_NO_PERMISSION"
        || detail.contains("PAT_MEDIUM_RISK_NO_PERMISSION")
        || detail.contains("chat.message:list")
        || detail.contains("该组织尚未开启CLI数据访问权限")
        || detail.contains("该组织尚未开启 CLI数据访问权限")
        || compact_detail.contains("CLI数据访问权限")
        || detail.contains("TOKEN_VERIFIED_FAILED")
        || (detail.contains("business_error")
            && detail.contains("group-chat")
            && (detail.contains("developerSettings")
                || detail.contains("developersSettings")
                || compact_detail.contains("CLI数据访问权限")))
        || (reason == "business_error"
            && category == "api"
            && server_key == "group-chat"
            && (server_error_code == "1001" || message.contains("forbidden request")))
        || (code_text == "1"
            && category == "api"
            && (action_url.contains("developerSettings")
                || action_url.contains("developersSettings")
                || message.contains("developerSettings")
                || message.contains("developersSettings")
                || detail.contains("该组织尚未开启CLI数据访问权限")
                || detail.contains("该组织尚未开启 CLI数据访问权限")
                || detail.contains("TOKEN_VERIFIED_FAILED")))
    {
        return Some(BridgeError {
            code: "DINGTALK_MESSAGE_PERMISSION_MISSING".to_owned(),
            message: "钉钉当前账号或组织没有开通消息读取权限，可能缺少chat.message:list授权，或组织尚未开启CLI数据访问权限。请重新授权钉钉官方CLI，或联系组织主管理员开启后再同步。".to_owned(),
            recoverable: true,
        });
    }
    None
}

pub(super) fn bridge_error(
    code: &str,
    message: &str,
    recoverable: bool,
    started_at: Instant,
) -> BridgeEnvelope {
    bridge_error_for(
        "wecom",
        "https://github.com/WecomTeam/wecom-cli",
        code,
        message,
        recoverable,
        started_at,
    )
}

pub(super) fn bridge_error_for(
    platform: &str,
    source: &str,
    code: &str,
    message: &str,
    recoverable: bool,
    started_at: Instant,
) -> BridgeEnvelope {
    BridgeEnvelope {
        ok: false,
        data: serde_json::Value::Null,
        warnings: Vec::new(),
        error: Some(BridgeError {
            code: code.to_owned(),
            message: message.to_owned(),
            recoverable,
        }),
        meta: serde_json::json!({
            "platform": platform,
            "source": source,
            "duration_ms": started_at.elapsed().as_millis()
        }),
    }
}
