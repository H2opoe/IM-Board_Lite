use super::{
    default_sessions_ready_message, no_prepare_profile_sync_access, official_cli_fetch_concurrency,
    session_history_mode, ConnectorAdapter, ConnectorCapabilities, ConnectorKind,
    SessionDiscoveryStep,
};

const DISCOVERY_STEPS: &[SessionDiscoveryStep] = &[SessionDiscoveryStep::WindowSearch];

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "feishu",
    kind: ConnectorKind::Feishu,
    is_available,
    sync_mode: session_history_mode,
    session_discovery_steps,
    should_run_session_list,
    prepare_profile_sync_access: no_prepare_profile_sync_access,
    sessions_ready_message: default_sessions_ready_message,
    fetch_concurrency,
    capabilities: ConnectorCapabilities {
        label: "飞书",
        auth_description: "使用官方CLI授权",
        runtime_dependency: "lark-cli",
        permissions: &["search:message", "im:chat"],
        commands: &[
            "auth-status",
            "search-messages",
            "list-chats",
            "fetch-messages",
        ],
        health_check: "auth-status",
        editions: &["full", "lite"],
        operating_systems: &["macos", "windows"],
        architectures: &["aarch64", "x86_64"],
        compatible_runtime_version: "1.0.98",
    },
};

fn is_available() -> bool {
    true
}

fn session_discovery_steps() -> &'static [SessionDiscoveryStep] {
    DISCOVERY_STEPS
}

fn should_run_session_list() -> bool {
    // 飞书同步仍优先通过消息检索发现当天活跃会话，避免群列表缺少活跃时间时扩大读取范围。
    // 手动读测需要列群时由 bridge_runner 调用官方新版 `im +chat-list`。
    false
}

fn fetch_concurrency() -> usize {
    official_cli_fetch_concurrency()
}
