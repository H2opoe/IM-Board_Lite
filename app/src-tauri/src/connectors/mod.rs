pub mod dingtalk;
pub mod feishu;
pub mod wecom;

use crate::storage::models::ImProfile;
use serde::Serialize;

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
    pub should_run_session_list: fn() -> bool,
    pub prepare_profile_sync_access: fn(&ImProfile),
    pub empty_session_warning: fn(&ImProfile) -> Option<String>,
    pub sessions_ready_message: fn(&ImProfile, i64) -> String,
    pub fetch_concurrency: fn() -> usize,
    pub should_silence_message_error: fn(&str) -> bool,
    pub fallback_group_search_queries: fn(&ImProfile) -> Vec<String>,
    pub capabilities: ConnectorCapabilities,
}

#[derive(Clone, Copy)]
pub struct ConnectorCapabilities {
    pub label: &'static str,
    pub auth_description: &'static str,
    pub runtime_dependency: &'static str,
    pub permissions: &'static [&'static str],
    pub commands: &'static [&'static str],
    pub health_check: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorCapabilityDescriptor {
    pub platform: &'static str,
    pub label: &'static str,
    pub auth_description: &'static str,
    pub runtime_dependency: &'static str,
    pub permissions: &'static [&'static str],
    pub commands: &'static [&'static str],
    pub health_check: &'static str,
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

pub fn capability_descriptors() -> Vec<ConnectorCapabilityDescriptor> {
    let mut platforms = std::collections::HashSet::new();
    CONNECTOR_REGISTRY
        .iter()
        .copied()
        .filter(|adapter| (adapter.is_available)() && platforms.insert(adapter.platform))
        .map(|adapter| ConnectorCapabilityDescriptor {
            platform: adapter.platform,
            label: adapter.capabilities.label,
            auth_description: adapter.capabilities.auth_description,
            runtime_dependency: adapter.capabilities.runtime_dependency,
            permissions: adapter.capabilities.permissions,
            commands: adapter.capabilities.commands,
            health_check: adapter.capabilities.health_check,
        })
        .collect()
}

pub fn default_should_run_session_list() -> bool {
    true
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

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn capability_descriptors_have_one_entry_per_platform() {
        let descriptors = capability_descriptors();
        let platforms = descriptors
            .iter()
            .map(|descriptor| descriptor.platform)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(platforms.len(), descriptors.len());
    }
}
