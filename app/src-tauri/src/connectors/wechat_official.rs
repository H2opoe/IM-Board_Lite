use super::{
    default_session_discovery_steps, default_sessions_ready_message, no_empty_session_warning,
    no_fallback_group_search_queries, no_prepare_profile_sync_access, no_silent_message_error,
    serial_fetch_concurrency, session_history_mode, ConnectorAdapter, ConnectorKind,
    ProfileSyncLane,
};

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "wechat",
    kind: ConnectorKind::WechatOfficial,
    is_available,
    sync_lane,
    sync_mode: session_history_mode,
    session_discovery_steps: default_session_discovery_steps,
    prepare_profile_sync_access: no_prepare_profile_sync_access,
    empty_session_warning: no_empty_session_warning,
    sessions_ready_message: default_sessions_ready_message,
    fetch_concurrency: serial_fetch_concurrency,
    should_silence_message_error: no_silent_message_error,
    fallback_group_search_queries: no_fallback_group_search_queries,
};

fn sync_lane() -> ProfileSyncLane {
    ProfileSyncLane::Serial
}

#[cfg(windows)]
fn is_available() -> bool {
    true
}

#[cfg(not(windows))]
fn is_available() -> bool {
    false
}
