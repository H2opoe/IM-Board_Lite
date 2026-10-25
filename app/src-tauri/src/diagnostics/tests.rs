use super::*;
use crate::security::diagnostic_metadata;

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
fn excludes_sensitive_and_free_text_raw_detail() {
    let value = diagnostic_metadata(
        json!({"apiKey":"sk-test", "message":"PRIVATE_CHAT", "httpStatus":500}),
    );
    assert_eq!(value, json!({"httpStatus":500}));
}

#[test]
fn classifies_actual_cli_lifecycle_missing_messages_before_persistence() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    for platform in ["feishu", "wecom", "dingtalk"] {
        for error in [
            "尚未准备好平台CLI。",
            "更新后未找到平台CLI。",
            "CLI runtime not found",
            "Missing CLI runtime",
        ] {
            let event =
                cli_lifecycle_error_event(platform, None, "check_platform_cli_update", error);
            assert_eq!(event.category, "missing_runtime", "{platform}: {error}");
            assert!(event.user_message.contains("请重新准备CLI"));
            record_error_event_conn(&conn, event).expect("persist event");
        }
    }
    let incorrect_rows: i64 = conn
        .query_row(
            "select count(*) from diagnostic_error_events where category <> 'missing_runtime'",
            [],
            |row| row.get(0),
        )
        .expect("query categories");
    assert_eq!(incorrect_rows, 0);
    assert_eq!(
        cli_lifecycle_error_event(
            "feishu",
            None,
            "deploy_platform_bridge",
            "unrelated failure"
        )
        .category,
        "unknown"
    );
}

#[test]
fn classifies_chinese_enterprise_transport_timeout_envelope() {
    let request = BridgeRequest {
        platform: "feishu".to_owned(),
        command: "messages".to_owned(),
        profile: None,
        args: std::collections::HashMap::new(),
        stdin_secret: None,
    };
    // Matches the FEISHU_NETWORK_TRANSPORT envelope from the official CLI error adapter.
    let envelope: BridgeEnvelope = serde_json::from_value(json!({
        "ok": false, "data": null, "warnings": [], "meta": {},
        "error": {
            "code": "FEISHU_NETWORK_TRANSPORT",
            "message": "飞书官方 CLI 当前网络传输失败，授权本身不一定失效。底层返回：MCP请求超时",
            "recoverable": true
        }
    }))
    .expect("error envelope");
    let event = bridge_error_event(&request, &envelope);
    assert_eq!(event.category, "network_timeout");
    assert_eq!(
        event.user_message,
        "飞书CLI请求超时，请检查网络、代理/VPN或稍后重试。"
    );
}

#[test]
fn local_model_download_events_preserve_network_category() {
    for (error, expected) in [
        ("本地DeepSeek模型下载中断：request timeout", "network_timeout"),
        ("本地DeepSeek模型下载请求未发出：error sending request for url (https://example.invalid/model.gguf)：原因：client error (Connect)", "network_connect"),
    ] {
        let event = local_ai_error_event("download_local_deepseek_model", error, json!({"provider": "本地DeepSeek"}));
        assert_eq!(event.category, expected, "{error}");
        assert_eq!(event.user_message, "本地DeepSeek模型下载失败，错误分类和结构化状态已写入诊断包。");
    }
    let event = local_ai_error_event("start_local_runtime", "运行时不可用", json!({}));
    assert_eq!(event.category, "local_runtime");
}
