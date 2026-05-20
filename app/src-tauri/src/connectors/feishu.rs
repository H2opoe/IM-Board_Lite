use super::{
    default_sessions_ready_message, no_fallback_group_search_queries,
    no_prepare_profile_sync_access, no_silent_message_error, official_cli_fetch_concurrency,
    session_history_mode, ConnectorAdapter, ConnectorKind, SessionDiscoveryStep,
};
use crate::storage::models::ImProfile;

const DISCOVERY_STEPS: &[SessionDiscoveryStep] = &[SessionDiscoveryStep::WindowSearch];

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "feishu",
    kind: ConnectorKind::Feishu,
    is_available,
    sync_mode: session_history_mode,
    session_discovery_steps,
    should_run_session_list,
    prepare_profile_sync_access: no_prepare_profile_sync_access,
    empty_session_warning,
    sessions_ready_message: default_sessions_ready_message,
    fetch_concurrency,
    should_silence_message_error: no_silent_message_error,
    fallback_group_search_queries: no_fallback_group_search_queries,
};

fn is_available() -> bool {
    true
}

fn session_discovery_steps() -> &'static [SessionDiscoveryStep] {
    DISCOVERY_STEPS
}

fn should_run_session_list() -> bool {
    // 飞书官方 CLI 1.0.x 没有可用的 im chats list 群列表命令；
    // 今天会话只能通过消息检索结果发现，再逐会话读取历史。
    false
}

fn empty_session_warning(_profile: &ImProfile) -> Option<String> {
    Some(
        "飞书：当前授权未返回今天可见会话。请确认已授权 search:message，且今天有可检索消息。"
            .to_owned(),
    )
}

fn fetch_concurrency() -> usize {
    official_cli_fetch_concurrency()
}
