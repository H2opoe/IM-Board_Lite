use super::*;
#[test]
fn every_edition_rejects_profile_paths_and_reserved_names_before_launch() {
    for id in ["../escape", "a/b", "a\\b", "a:b", " leading", "NUL", "COM1"] {
        let request = BridgeRequest {
            platform: "wecom".into(),
            command: "auth-status".into(),
            profile: Some(crate::storage::models::ImProfile {
                id: id.into(),
                platform: "wecom".into(),
                label: "fixture".into(),
                enabled: true,
                config_json: serde_json::json!({}),
                status: "normal".into(),
                sort_order: 0,
                created_at: "".into(),
                updated_at: "".into(),
            }),
            args: std::collections::HashMap::new(),
            stdin_secret: None,
        };
        assert!(
            validate_bridge_request(&request).is_err(),
            "invalid profile accepted"
        );
    }
}
