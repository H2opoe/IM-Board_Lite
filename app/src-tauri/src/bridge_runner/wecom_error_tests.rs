use super::errors::classify_wecom_cli_error;

#[test]
fn explicit_authentication_errors_are_recognized() {
    for text in [
        "Not authenticated",
        "Error: not authenticated; please login",
        "HTTP 401",
        "HTTP/1.1 401 Unauthorized",
        "Request failed with status code 401",
        "status code: 401",
        "401 Unauthorized",
    ] {
        let error =
            classify_wecom_cli_error(text).unwrap_or_else(|| panic!("not classified: {text}"));
        assert_eq!(error.code, "WECOM_NOT_AUTHENTICATED", "{text}");
        assert!(error.recoverable);
    }
}

#[test]
fn unrelated_digits_do_not_turn_errors_into_authentication_failures() {
    for text in [
        "request_id=4012: timeout",
        "connection refused on port 401",
        "API error 1401",
        "status code 4010",
        "HTTP 4010",
        "trace=401 Unauthorized request failed",
        "not authenticatedness",
        "error: file /tmp/401/data missing",
    ] {
        assert!(
            classify_wecom_cli_error(text).is_none(),
            "misclassified: {text}"
        );
    }
    let error = classify_wecom_cli_error("request_id=4012: 暂不支持授权机器人读取消息").unwrap();
    assert_eq!(error.code, "WECOM_MESSAGE_PERMISSION_UNSUPPORTED");
}
