use std::time::Instant;

use crate::security::sanitize_log;

use super::{BridgeEnvelope, BridgeError};

pub(super) fn classify_wecom_cli_error(detail: &str) -> Option<BridgeError> {
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
    "macOS终端"
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
    let app_configured = has_non_empty_string_field(raw, &["appId", "app_id", "brand"]);
    let verified = has_true_bool_field(raw, &["verified"]) || has_ready_user_identity(raw);
    // lark-cli 的 auth status 输出结构会随版本变化；新版可能把 tokenStatus/scope
    // 放进 result/data/identities 等嵌套对象里。这里只把明确失效的 tokenStatus 判为失败，
    // 字段缺失时仍以 auth status 的退出码和 verified/identity 状态为准。
    let token_statuses = string_values_for_keys(raw, &["tokenStatus", "token_status"]);
    let token_valid = token_statuses.is_empty()
        || token_statuses.iter().any(|value| {
            // 新版 lark-cli 会在 access token 到期但 refresh token 仍有效时返回 needs_refresh，
            // 并说明下一次 user API 调用会自动刷新；这不是授权失效，不能提示用户重新授权。
            value.eq_ignore_ascii_case("valid") || value.eq_ignore_ascii_case("needs_refresh")
        });
    let granted_scopes = feishu_scope_values(raw);
    let required_scopes = [
        "search:message",
        "im:chat:read",
        "im:message:readonly",
        "im:message.reactions:read",
        "im:message.p2p_msg:get_as_user",
        "im:message.group_msg:get_as_user",
        "contact:user.base:readonly",
        "contact:user.basic_profile:readonly",
    ];
    let has_required_scopes = required_scopes
        .iter()
        .all(|required| granted_scopes.iter().any(|granted| granted == *required));
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

fn string_values_for_keys(raw: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    collect_string_values_for_keys(raw, keys, &mut values);
    values
}

fn collect_string_values_for_keys(
    raw: &serde_json::Value,
    keys: &[&str],
    values: &mut Vec<String>,
) {
    match raw {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if keys.iter().any(|candidate| key == candidate) {
                    if let Some(text) = value
                        .as_str()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        values.push(text.to_owned());
                    }
                }
                collect_string_values_for_keys(value, keys, values);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_string_values_for_keys(item, keys, values);
            }
        }
        _ => {}
    }
}

fn has_non_empty_string_field(raw: &serde_json::Value, keys: &[&str]) -> bool {
    !string_values_for_keys(raw, keys).is_empty()
}

fn has_true_bool_field(raw: &serde_json::Value, keys: &[&str]) -> bool {
    match raw {
        serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
            (keys.iter().any(|candidate| key == candidate) && value.as_bool() == Some(true))
                || has_true_bool_field(value, keys)
        }),
        serde_json::Value::Array(items) => items.iter().any(|item| has_true_bool_field(item, keys)),
        _ => false,
    }
}

fn has_ready_user_identity(raw: &serde_json::Value) -> bool {
    match raw {
        serde_json::Value::Object(map) => {
            if let Some(user) = map
                .get("identities")
                .and_then(|value| value.get("user"))
                .and_then(|value| value.as_object())
            {
                let status = user
                    .get("status")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let available = user.get("available").and_then(|value| value.as_bool());
                if status == "ready" || available == Some(true) {
                    return true;
                }
            }
            let identity = map
                .get("identity")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let status = map
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let available = map.get("available").and_then(|value| value.as_bool());
            if identity == "user" && (status == "ready" || available == Some(true)) {
                return true;
            }
            map.values().any(has_ready_user_identity)
        }
        serde_json::Value::Array(items) => items.iter().any(has_ready_user_identity),
        _ => false,
    }
}

fn feishu_scope_values(raw: &serde_json::Value) -> Vec<String> {
    let mut scopes = Vec::new();
    collect_feishu_scope_values(raw, &mut scopes);
    scopes.sort();
    scopes.dedup();
    scopes
}

fn collect_feishu_scope_values(raw: &serde_json::Value, scopes: &mut Vec<String>) {
    match raw {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if key == "scope" || key == "scopes" {
                    collect_scope_value(value, scopes);
                }
                collect_feishu_scope_values(value, scopes);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_feishu_scope_values(item, scopes);
            }
        }
        _ => {}
    }
}

fn collect_scope_value(raw: &serde_json::Value, scopes: &mut Vec<String>) {
    match raw {
        serde_json::Value::String(text) => scopes.extend(
            text.split_whitespace()
                .map(str::trim)
                .filter(|scope| !scope.is_empty())
                .map(str::to_owned),
        ),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_scope_value(item, scopes);
            }
        }
        _ => {}
    }
}

pub(super) fn classify_feishu_cli_error(stdout: &str, stderr: &str) -> Option<BridgeError> {
    let detail = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let raw = serde_json::from_str::<serde_json::Value>(stdout).ok();
    if let Some(error) = raw
        .as_ref()
        .and_then(|value| classify_feishu_cli_structured_error(value, stderr))
    {
        return Some(error);
    }
    let normalized_detail = detail.to_ascii_lowercase();
    if normalized_detail.contains("b2c app not support")
        || normalized_detail.contains("app type is not supported")
    {
        return Some(BridgeError {
            code: "FEISHU_B2C_APP_UNSUPPORTED".to_owned(),
            message: "该会话是飞书应用/机器人会话，飞书官方接口返回231204（b2c app not support），当前CLI不能用用户身份读取这类会话历史；已跳过，不影响其他会话同步。".to_owned(),
            recoverable: true,
        });
    }
    if normalized_detail.contains("not_authenticated") {
        return Some(BridgeError {
            code: "FEISHU_NOT_AUTHENTICATED".to_owned(),
            message: feishu_user_auth_incomplete_message(),
            recoverable: true,
        });
    }
    if normalized_detail.contains("not configured")
        || normalized_detail.contains("not logged in")
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

pub(super) fn classify_feishu_cli_structured_error(
    raw: &serde_json::Value,
    stderr: &str,
) -> Option<BridgeError> {
    let Some(error) = raw.get("error") else {
        return None;
    };
    let reason = error
        .get("reason")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let error_type = error
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let error_code = error
        .get("code")
        .and_then(|value| {
            value
                .as_i64()
                .map(|code| code.to_string())
                .or_else(|| value.as_str().map(str::to_owned))
        })
        .unwrap_or_default();
    let error_message = error
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let missing_scopes = error
        .get("missing_scopes")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .filter(|item| !item.trim().is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let normalized_detail = format!("{error_message}\n{stderr}").to_ascii_lowercase();
    if error_type == "network"
        || error
            .get("subtype")
            .and_then(|value| value.as_str())
            .is_some_and(|value| value == "transport")
    {
        let mut hints = vec![
            "飞书官方 CLI 当前网络传输失败，授权本身不一定失效。请检查网络或代理后重试。"
                .to_owned(),
            "如果账号绑定页提示飞书 CLI 可更新，请先更新 CLI 后再同步。".to_owned(),
        ];
        if let Some(update_message) = feishu_cli_update_notice(raw) {
            hints.push(update_message);
        }
        if !error_message.trim().is_empty() {
            hints.push(format!("底层返回：{}", error_message.trim()));
        }
        return Some(BridgeError {
            code: "FEISHU_NETWORK_TRANSPORT".to_owned(),
            message: hints.join("\n"),
            recoverable: true,
        });
    }
    if error
        .get("subtype")
        .and_then(|value| value.as_str())
        .is_some_and(|value| value == "missing_scope")
        || !missing_scopes.is_empty()
        || normalized_detail.contains("missing required scope")
    {
        let scope_text = if missing_scopes.is_empty() {
            "飞书消息读取所需权限".to_owned()
        } else {
            missing_scopes.join("、")
        };
        return Some(BridgeError {
            code: "FEISHU_MISSING_SCOPE".to_owned(),
            message: format!(
                "飞书缺少授权权限：{scope_text}。请在平台管理中重新复制并运行飞书绑定命令，按提示补充授权后再同步。"
            ),
            recoverable: true,
        });
    }
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
    if reason == "not_authenticated" || normalized_detail.contains("not_authenticated") {
        return Some(BridgeError {
            code: "FEISHU_NOT_AUTHENTICATED".to_owned(),
            message: feishu_user_auth_incomplete_message(),
            recoverable: true,
        });
    }
    if error_message == "not configured"
        || error_type == "config"
        || normalized_detail.contains("not configured")
        || normalized_detail.contains("not logged in")
        || normalized_detail.contains("未登录")
    {
        return Some(BridgeError {
            code: "FEISHU_NOT_AUTHENTICATED".to_owned(),
            message: feishu_app_config_incomplete_message(),
            recoverable: true,
        });
    }
    None
}

fn feishu_cli_update_notice(raw: &serde_json::Value) -> Option<String> {
    let update = raw.get("_notice")?.get("update")?;
    let current = update
        .get("current")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let latest = update
        .get("latest")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if current.is_empty() || latest.is_empty() {
        return None;
    }
    Some(format!(
        "检测到飞书 CLI 可更新：当前 {current}，最新 {latest}。"
    ))
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
