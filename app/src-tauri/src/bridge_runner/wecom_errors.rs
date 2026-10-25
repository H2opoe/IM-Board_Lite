use std::sync::OnceLock;

use super::errors::not_authenticated_message;
use super::BridgeError;

pub(super) fn classify_wecom_cli_error(detail: &str) -> Option<BridgeError> {
    static AUTH_ERROR: OnceLock<regex::Regex> = OnceLock::new();
    // A request ID, port, path or business error code containing 401 is not
    // evidence of an HTTP authentication failure.
    let auth_error = AUTH_ERROR.get_or_init(|| {
        regex::Regex::new(r"(?im)(?:\bnot authenticated\b|\bhttp(?:/\d+(?:\.\d+)?)?\s+401(?:\s|$)|\bstatus\s+code\s*:?\s*401(?:\s|$)|^\s*401\s+unauthorized\b)")
            .expect("constant authentication error pattern")
    });
    if auth_error.is_match(detail) {
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
