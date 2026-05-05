use super::{
    default_session_discovery_steps, default_sessions_ready_message, no_empty_session_warning,
    no_fallback_group_search_queries, no_silent_message_error, serial_fetch_concurrency,
    session_history_mode, ConnectorAdapter, ConnectorKind, ProfileSyncLane,
};
use crate::macos_permissions;
use crate::storage::models::ImProfile;

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "wechat",
    kind: ConnectorKind::WechatLocal,
    is_available,
    sync_lane,
    sync_mode: session_history_mode,
    session_discovery_steps: default_session_discovery_steps,
    prepare_profile_sync_access,
    empty_session_warning: no_empty_session_warning,
    sessions_ready_message: default_sessions_ready_message,
    fetch_concurrency: serial_fetch_concurrency,
    should_silence_message_error: no_silent_message_error,
    fallback_group_search_queries: no_fallback_group_search_queries,
};

fn sync_lane() -> ProfileSyncLane {
    ProfileSyncLane::Serial
}

fn prepare_profile_sync_access(profile: &ImProfile) {
    // macOS 的 TCC 授权会记录到主应用身份上。同步前先由主应用触发一次读取权限注册，
    // 避免后续每个 Python bridge 子进程在读取微信容器或外置卷资源时分别弹窗。
    macos_permissions::prepare_wechat_sync_access(&profile.config_json);
}

#[cfg(windows)]
fn is_available() -> bool {
    false
}

#[cfg(not(windows))]
fn is_available() -> bool {
    true
}
