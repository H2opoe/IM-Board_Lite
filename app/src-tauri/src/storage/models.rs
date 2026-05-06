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
