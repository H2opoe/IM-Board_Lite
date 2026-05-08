pub mod dingtalk;
pub mod feishu;
pub mod wecom;

use crate::storage::models::ImProfile;

pub const OFFICIAL_CLI_FETCH_CONCURRENCY: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectorKind {
    Wecom,
    Feishu,
    Dingtalk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileSyncMode {
    SessionHistory,
    WindowMessagesWithGroupFallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionDiscoveryStep {
    Contacts,
    WindowSearch,
}

#[derive(Clone, Copy)]
pub struct ConnectorAdapter {
    pub platform: &'static str,
    pub kind: ConnectorKind,
    pub is_available: fn() -> bool,
    pub sync_mode: fn() -> ProfileSyncMode,
    pub session_discovery_steps: fn() -> &'static [SessionDiscoveryStep],
    pub prepare_profile_sync_access: fn(&ImProfile),
    pub empty_session_warning: fn(&ImProfile) -> Option<String>,
    pub sessions_ready_message: fn(&ImProfile, i64) -> String,
    pub fetch_concurrency: fn() -> usize,
    pub should_silence_message_error: fn(&str) -> bool,
    pub fallback_group_search_queries: fn(&ImProfile) -> Vec<String>,
}

impl ConnectorAdapter {
    fn matches(self, platform: &str) -> bool {
        (self.is_available)() && self.platform == platform
    }
}

const CONNECTOR_REGISTRY: &[ConnectorAdapter] =
    &[wecom::ADAPTER, feishu::ADAPTER, dingtalk::ADAPTER];

pub fn find(platform: &str) -> Option<ConnectorAdapter> {
    CONNECTOR_REGISTRY
        .iter()
        .copied()
        .find(|adapter| adapter.matches(platform))
}

pub fn no_prepare_profile_sync_access(_profile: &ImProfile) {}

pub fn default_sessions_ready_message(profile: &ImProfile, total: i64) -> String {
    format!(
        "已发现【{} · {}】{}个今天会话。",
        platform_label(&profile.platform),
        profile_remark(profile),
        total
    )
}

pub fn official_cli_fetch_concurrency() -> usize {
    OFFICIAL_CLI_FETCH_CONCURRENCY
}

pub fn no_silent_message_error(_code: &str) -> bool {
    false
}

pub fn no_fallback_group_search_queries(_profile: &ImProfile) -> Vec<String> {
    Vec::new()
}

pub fn session_history_mode() -> ProfileSyncMode {
    ProfileSyncMode::SessionHistory
}

pub fn platform_label(platform: &str) -> &str {
    match platform {
        "wechat" => "微信",
        "wecom" => "企业微信",
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        _ => platform,
    }
}

pub fn profile_remark(profile: &ImProfile) -> String {
    profile
        .config_json
        .get("remark")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&profile.label)
        .to_owned()
}
