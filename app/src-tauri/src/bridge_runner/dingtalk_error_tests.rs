use super::errors::classify_dingtalk_cli_error;

#[test]
fn classifies_dingtalk_group_chat_service_not_subscribed_error() {
    let stdout = r#"{
          "error": {
            "category": "api",
            "code": 1,
            "hint": "The API returned a business-level error. Check required parameters and values.",
            "message": "business error: success=false",
            "operation": "tools/call",
            "reason": "business_error",
            "server_error_code": "PARAM_ERROR",
            "server_key": "group-chat",
            "technical_detail": "调用 McpService.queryToolMeta 失败: <AioRpcError of RPC that terminated with:\n\tstatus = StatusCode.INTERNAL\n\tdetails = \"error invoking output binding hsf.consumer: [HSF_ILLEGALPARAMS] illegal params: service with name 'com.dingtalk.open.mcp.portal.api.McpService' is not subscribed\"\n\tdebug_error_string = \"UNKNOWN:Error received from peer ipv4:127.0.0.1:50001 {grpc_status:13, grpc_message:\"error invoking output binding hsf.consumer: [HSF_ILLEGALPARAMS] illegal params: service with name \\'com.dingtalk.open.mcp.portal.api.McpService\\' is not subscribed\"}\"\n>",
            "trace_id": "21030ead17782268666386874e0a34"
          }
        }"#;

    let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
    assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
    assert!(error.recoverable);
    assert!(error.message.contains("CLI数据访问权限"));
}

#[test]
fn missing_private_token_storage_reuses_the_existing_unfinished_authorization_prompt() {
    for message in [
        "resolve access token: secure data file: C:\\fixture\\.data: The system cannot find the file specified.",
        "[UNCLASSIFIED] resolve access token: read secure data file: open /fixture/dingtalk/.data: no such file or directory (operation: contact/get_current_user_profile)",
        "resolve access token: secure data file: C:\\fixture\\.data: 系统找不到指定的文件 (os error 2)",
    ] {
        let stdout=serde_json::json!({"error":{"category":"internal","code":5,"message":message}}).to_string();
        let error=classify_dingtalk_cli_error(&stdout,"").expect("auth classification");
        assert_eq!(error.code,"DINGTALK_NOT_AUTHENTICATED");assert!(error.message.contains("钉钉尚未完成授权"));assert!(!error.message.contains(".data"));assert!(!error.message.contains("UNCLASSIFIED"));
    }
}
#[test]
fn missing_non_auth_files_permissions_and_decryption_errors_do_not_request_login() {
    for message in [
        "open /fixture/tmp/.data: no such file or directory",
        "resolve access token: secure data file /fixture/.data: permission denied (os error 5)",
        "resolve access token: secure data file /fixture/.data: cipher authentication failed",
        "resolve access token: secure data file /fixture/.data: invalid argument (os error 22)",
    ] {
        let stdout =
            serde_json::json!({"error":{"category":"internal","code":5,"message":message}})
                .to_string();
        assert!(classify_dingtalk_cli_error(&stdout, "").is_none());
    }
}

#[test]
fn active_terminal_authorization_is_busy_not_missing_login_or_permission() {
    let error = classify_dingtalk_cli_error("", "IMBOARD_DWS_ACCOUNT_BUSY: fixture")
        .expect("busy classification");
    assert_eq!(error.code, "DINGTALK_AUTHORIZATION_BUSY");
    assert!(error.message.contains("终端"));
    assert!(error.recoverable);
}
