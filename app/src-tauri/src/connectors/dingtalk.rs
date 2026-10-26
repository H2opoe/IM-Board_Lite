use super::{
    default_sessions_ready_message, default_should_run_session_list,
    no_prepare_profile_sync_access, official_cli_fetch_concurrency, ConnectorAdapter,
    ConnectorCapabilities, ConnectorKind, ProfileSyncMode, SessionDiscoveryStep,
};

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "dingtalk",
    kind: ConnectorKind::Dingtalk,
    is_available,
    sync_mode,
    session_discovery_steps,
    should_run_session_list: default_should_run_session_list,
    prepare_profile_sync_access: no_prepare_profile_sync_access,
    sessions_ready_message: default_sessions_ready_message,
    fetch_concurrency,
    capabilities: ConnectorCapabilities {
        label: "钉钉",
        auth_description: "使用官方CLI授权",
        runtime_dependency: "dws",
        permissions: &["chat.list", "chat.message:list"],
        commands: &[
            "auth-status",
            "get-self",
            "list-chats",
            "search-groups",
            "search-messages",
            "fetch-messages",
        ],
        health_check: "auth-status",
        editions: &["full", "lite"],
        operating_systems: &["macos", "windows"],
        architectures: &["aarch64", "x86_64"],
        compatible_runtime_version: "1.0.65-imboard.1",
    },
};

fn is_available() -> bool {
    true
}

fn sync_mode() -> ProfileSyncMode {
    ProfileSyncMode::WindowMessages
}

fn session_discovery_steps() -> &'static [SessionDiscoveryStep] {
    &[]
}

fn fetch_concurrency() -> usize {
    official_cli_fetch_concurrency()
}
