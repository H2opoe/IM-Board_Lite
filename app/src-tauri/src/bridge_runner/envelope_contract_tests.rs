use super::*;
#[test]
fn success_and_failure_keep_nullable_error_and_camel_case_profile() {
    let success = BridgeEnvelope {
        ok: true,
        data: serde_json::json!({"items": []}),
        warnings: vec![],
        error: None,
        meta: serde_json::json!({}),
    };
    let value = serde_json::to_value(&success).unwrap();
    assert!(value["error"].is_null());
    assert_eq!(value.as_object().unwrap().len(), 5);
    let roundtrip: BridgeEnvelope = serde_json::from_value(value).unwrap();
    assert!(roundtrip.ok);
    let failure = BridgeEnvelope {
        ok: false,
        error: Some(BridgeError {
            code: "FIXTURE_FAILURE".into(),
            message: "synthetic failure".into(),
            recoverable: true,
        }),
        ..success
    };
    let value = serde_json::to_value(failure).unwrap();
    assert_eq!(value["error"]["recoverable"], true);
    assert_eq!(value["error"]["code"], "FIXTURE_FAILURE");
    let profile = ImProfile {
        id: "fixture".into(),
        platform: "feishu".into(),
        label: "fixture".into(),
        enabled: true,
        config_json: serde_json::json!({"authType":"cli_session"}),
        status: "ready".into(),
        sort_order: 0,
        created_at: "2026-01-01".into(),
        updated_at: "2026-01-01".into(),
    };
    let value = serde_json::to_value(profile).unwrap();
    assert_eq!(value["configJson"]["authType"], "cli_session");
    assert!(value.get("authType").is_none());
    assert!(value.get("createdAt").is_some());
    assert!(value.get("updatedAt").is_some());
    assert_eq!(value.as_object().unwrap().len(), 9);
}
