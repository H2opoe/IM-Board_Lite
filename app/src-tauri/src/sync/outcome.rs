use crate::storage::models::ImProfile;

#[derive(Debug)]
pub(crate) struct ProfileSyncOutcome {
    pub(crate) profile_id: String,
    pub(crate) succeeded: bool,
    pub(crate) inserted_messages: i64,
    pub(crate) warnings: Vec<String>,
    pub(crate) error: Option<String>,
}

impl ProfileSyncOutcome {
    pub(super) fn from_reads(
        profile: &ImProfile,
        inserted_messages: i64,
        mut warnings: Vec<String>,
        errors: Vec<String>,
    ) -> Self {
        let error = (!errors.is_empty()).then(|| errors.join("；"));
        warnings.extend(errors);
        Self {
            profile_id: profile.id.clone(),
            succeeded: error.is_none(),
            inserted_messages,
            warnings,
            error,
        }
    }

    pub(super) fn failed(profile: &ImProfile, message: String) -> Self {
        Self {
            profile_id: profile.id.clone(),
            succeeded: false,
            inserted_messages: 0,
            warnings: vec![message.clone()],
            error: Some(message),
        }
    }
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    #[test]
    fn read_failures_preserve_import_count_without_advancing_success() {
        let profile = ImProfile {
            id: "test".into(),
            platform: "feishu".into(),
            label: "test".into(),
            enabled: true,
            config_json: serde_json::json!({}),
            status: "normal".into(),
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let failed =
            ProfileSyncOutcome::from_reads(&profile, 7, vec![], vec!["read failed".into()]);
        assert!(!failed.succeeded);
        assert_eq!(failed.inserted_messages, 7);
        assert_eq!(failed.error.as_deref(), Some("read failed"));
        assert_eq!(failed.warnings, vec!["read failed"]);
        let empty = ProfileSyncOutcome::from_reads(&profile, 0, vec![], vec![]);
        assert!(empty.succeeded);
        assert!(empty.error.is_none());
    }
}
