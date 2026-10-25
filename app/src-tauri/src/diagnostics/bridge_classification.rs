//! Pure error classification; no database, process, or network access.
use super::platform_label;

pub(super) fn bridge_error_category(code: &str, detail: &str) -> &'static str {
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

pub(super) fn bridge_user_message(platform: &str, code: &str, category: &str) -> String {
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
            format!("{label}CLI启动失败，错误分类和结构化状态已写入诊断包。")
        }
        _ if category == "network_timeout" => {
            format!("{label}CLI请求超时，请检查网络、代理/VPN或稍后重试。")
        }
        _ if category == "runtime_incomplete" => {
            format!("{label}CLI运行时不完整或版本不匹配，请重新准备CLI后再同步。")
        }
        _ if category == "api_http" => {
            format!("{label}接口返回错误，错误分类和结构化状态已写入诊断包。")
        }
        _ if category == "cli_failed" => {
            format!("{label}CLI执行失败，错误分类和结构化状态已写入诊断包。")
        }
        _ => "发生未知错误，错误分类和结构化状态已写入诊断包。".to_owned(),
    }
}
