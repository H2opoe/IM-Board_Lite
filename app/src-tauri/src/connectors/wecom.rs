use super::{
    default_should_run_session_list, no_prepare_profile_sync_access,
    official_cli_fetch_concurrency, platform_label, profile_remark, session_history_mode,
    ConnectorAdapter, ConnectorCapabilities, ConnectorKind, SessionDiscoveryStep,
};
use crate::storage::models::ImProfile;

const DISCOVERY_STEPS: &[SessionDiscoveryStep] = &[SessionDiscoveryStep::Contacts];

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "wecom",
    kind: ConnectorKind::Wecom,
    is_available,
    sync_mode: session_history_mode,
    session_discovery_steps,
    should_run_session_list: default_should_run_session_list,
    prepare_profile_sync_access: no_prepare_profile_sync_access,
    sessions_ready_message,
    fetch_concurrency,
    capabilities: ConnectorCapabilities {
        label: "企业微信",
        auth_description: "使用官方CLI授权",
        runtime_dependency: "wecom-cli",
        permissions: &["contacts", "messages"],
        commands: &[
            "auth-status",
            "account-identity",
            "list-contacts",
            "list-chats",
            "fetch-messages",
        ],
        health_check: "auth-status",
        editions: &["full", "lite"],
        operating_systems: &["macos", "windows"],
        architectures: &["aarch64", "x86_64"],
        compatible_runtime_version: "1.3.4",
    },
};

fn is_available() -> bool {
    true
}

fn session_discovery_steps() -> &'static [SessionDiscoveryStep] {
    DISCOVERY_STEPS
}

fn sessions_ready_message(profile: &ImProfile, total: i64) -> String {
    format!(
        "已准备检查【{} · {}】{}个通讯录成员/会话。",
        platform_label(&profile.platform),
        profile_remark(profile),
        total
    )
}

fn fetch_concurrency() -> usize {
    official_cli_fetch_concurrency()
}
