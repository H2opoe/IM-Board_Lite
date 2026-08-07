use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImProfile {
    pub id: String,
    pub platform: String,
    pub label: String,
    pub enabled: bool,
    pub config_json: serde_json::Value,
    pub status: String,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardMetric {
    pub key: String,
    pub label: String,
    pub value: i64,
    pub sources: Vec<SourceStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStat {
    pub profile_id: String,
    pub platform: String,
    pub platform_label: String,
    pub remark: String,
    pub label: String,
    pub count: i64,
    pub chats: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    pub id: String,
    pub item_type: String,
    pub status: String,
    pub priority: String,
    pub title: String,
    pub description: String,
    pub suggested_reply: Option<String>,
    pub profile_id: String,
    pub platform: String,
    pub platform_label: String,
    pub platform_remark: String,
    pub source_label: String,
    pub chat_id: String,
    pub chat_name: String,
    pub evidence_summary: String,
    pub context_incomplete: bool,
    pub carry_over: bool,
    pub source_message_at: String,
    pub last_updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardData {
    pub day: String,
    pub metrics: Vec<DashboardMetric>,
    pub replies: Vec<ActionItem>,
    pub tasks: Vec<ActionItem>,
    pub topics: Vec<serde_json::Value>,
    pub chat_rank: Vec<serde_json::Value>,
    pub speaker_top: Vec<serde_json::Value>,
    pub hourly_activity: Vec<serde_json::Value>,
    pub message_types: Vec<serde_json::Value>,
    pub keywords: Vec<serde_json::Value>,
    pub keyword_status: String,
    pub keyword_source: String,
    pub keyword_version: String,
    pub keyword_updated_at: String,
    pub ai_status: String,
    pub sync_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub provider: String,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub user_prompt: String,
    pub analysis_prompt: String,
    pub summary_prompt: String,
    #[serde(default)]
    pub analysis_prompt_custom: bool,
    #[serde(default)]
    pub summary_prompt_custom: bool,
    pub analysis_batch_size: i64,
    pub enabled: bool,
    pub test_status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfigView {
    pub provider: String,
    pub api_key_configured: bool,
    pub base_url: String,
    pub model: String,
    pub user_prompt: String,
    pub analysis_prompt: String,
    pub summary_prompt: String,
    pub analysis_prompt_custom: bool,
    pub summary_prompt_custom: bool,
    pub analysis_batch_size: i64,
    pub enabled: bool,
    pub test_status: String,
}

impl From<AiConfig> for AiConfigView {
    fn from(config: AiConfig) -> Self {
        Self {
            api_key_configured: !config.api_key.trim().is_empty(),
            provider: config.provider,
            base_url: config.base_url,
            model: config.model,
            user_prompt: config.user_prompt,
            analysis_prompt: config.analysis_prompt,
            summary_prompt: config.summary_prompt,
            analysis_prompt_custom: config.analysis_prompt_custom,
            summary_prompt_custom: config.summary_prompt_custom,
            analysis_batch_size: config.analysis_batch_size,
            enabled: config.enabled,
            test_status: config.test_status,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfigInput {
    pub provider: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub clear_api_key: bool,
    pub base_url: String,
    pub model: String,
    pub user_prompt: String,
    pub analysis_prompt: String,
    pub summary_prompt: String,
    #[serde(default)]
    pub analysis_prompt_custom: bool,
    #[serde(default)]
    pub summary_prompt_custom: bool,
    pub analysis_batch_size: i64,
    pub enabled: bool,
    pub test_status: String,
}

impl AiConfigInput {
    pub fn into_config(self, stored_api_key: String) -> AiConfig {
        let api_key = if self.clear_api_key {
            String::new()
        } else if self.api_key.trim().is_empty() {
            stored_api_key
        } else {
            self.api_key
        };
        AiConfig {
            provider: self.provider,
            api_key,
            base_url: self.base_url,
            model: self.model,
            user_prompt: self.user_prompt,
            analysis_prompt: self.analysis_prompt,
            summary_prompt: self.summary_prompt,
            analysis_prompt_custom: self.analysis_prompt_custom,
            summary_prompt_custom: self.summary_prompt_custom,
            analysis_batch_size: self.analysis_batch_size,
            enabled: self.enabled,
            test_status: self.test_status,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelStatus {
    pub provider: String,
    pub model: String,
    pub file_name: String,
    pub file_path: String,
    pub source_url: String,
    pub installed: bool,
    pub size_bytes: i64,
    pub expected_size_bytes: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelDownloadProgress {
    pub provider: String,
    pub model: String,
    pub status: String,
    pub downloaded_bytes: i64,
    pub total_bytes: i64,
    pub percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub profile_id: String,
    pub sync_status: String,
    pub ai_status: String,
    pub inserted_messages: i64,
    pub analyzed_messages: i64,
    pub warnings: Vec<String>,
    pub started_at: String,
    pub finished_at: String,
}

#[cfg(test)]
mod ai_config_security_tests {
    use super::{AiConfig, AiConfigInput, AiConfigView};

    fn config_with_key(api_key: &str) -> AiConfig {
        AiConfig {
            provider: "openai".to_owned(),
            api_key: api_key.to_owned(),
            base_url: "https://example.invalid/v1".to_owned(),
            model: "test-model".to_owned(),
            user_prompt: String::new(),
            analysis_prompt: String::new(),
            summary_prompt: String::new(),
            analysis_prompt_custom: false,
            summary_prompt_custom: false,
            analysis_batch_size: 100,
            enabled: true,
            test_status: "untested".to_owned(),
        }
    }

    fn input(api_key: &str, clear_api_key: bool) -> AiConfigInput {
        let config = config_with_key(api_key);
        AiConfigInput {
            provider: config.provider,
            api_key: config.api_key,
            clear_api_key,
            base_url: config.base_url,
            model: config.model,
            user_prompt: config.user_prompt,
            analysis_prompt: config.analysis_prompt,
            summary_prompt: config.summary_prompt,
            analysis_prompt_custom: config.analysis_prompt_custom,
            summary_prompt_custom: config.summary_prompt_custom,
            analysis_batch_size: config.analysis_batch_size,
            enabled: config.enabled,
            test_status: config.test_status,
        }
    }

    #[test]
    fn config_view_never_serializes_api_key() {
        let value =
            serde_json::to_value(AiConfigView::from(config_with_key("top-secret"))).unwrap();
        assert_eq!(value["apiKeyConfigured"], true);
        assert!(value.get("apiKey").is_none());
        assert!(!value.to_string().contains("top-secret"));
    }

    #[test]
    fn blank_input_preserves_stored_api_key() {
        assert_eq!(
            input("", false)
                .into_config("stored-secret".to_owned())
                .api_key,
            "stored-secret"
        );
    }

    #[test]
    fn explicit_clear_removes_stored_api_key() {
        assert!(input("", true)
            .into_config("stored-secret".to_owned())
            .api_key
            .is_empty());
    }

    #[test]
    fn non_blank_input_replaces_stored_api_key() {
        assert_eq!(
            input("new-secret", false)
                .into_config("stored-secret".to_owned())
                .api_key,
            "new-secret"
        );
    }
}
