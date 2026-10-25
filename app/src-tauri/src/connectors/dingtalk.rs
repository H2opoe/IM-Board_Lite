use super::{
    default_sessions_ready_message, default_should_run_session_list,
    no_prepare_profile_sync_access, official_cli_fetch_concurrency, ConnectorAdapter,
    ConnectorCapabilities, ConnectorKind, ProfileSyncMode, SessionDiscoveryStep,
};
use crate::storage::models::ImProfile;

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "dingtalk",
    kind: ConnectorKind::Dingtalk,
    is_available,
    sync_mode,
    session_discovery_steps,
    should_run_session_list: default_should_run_session_list,
    prepare_profile_sync_access: no_prepare_profile_sync_access,
    empty_session_warning,
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

fn empty_session_warning(_profile: &ImProfile) -> Option<String> {
    None
}

fn fetch_concurrency() -> usize {
    // Windows 版 DWS 把授权 token 固定写入当前用户注册表，运行前需要按 profile 导入 token。
    // 同一 profile 内也串行读取，避免多个 DWS 子进程抢同一个注册表 token。
    if cfg!(windows) {
        1
    } else {
        official_cli_fetch_concurrency()
    }
}
