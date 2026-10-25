use super::*;

#[test]
fn missing_local_runtime_has_specific_user_guidance() {
    let event = local_ai_error_event(
        "start_local_runtime",
        "bundled runtime missing",
        serde_json::json!({}),
    );
    assert_eq!(
        event.user_message,
        "本地推理运行时缺失，请重新准备内置资源后再测试。"
    );
}
