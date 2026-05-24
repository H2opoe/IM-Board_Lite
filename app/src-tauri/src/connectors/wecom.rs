use super::{
    default_should_run_session_list, no_fallback_group_search_queries,
    no_prepare_profile_sync_access, no_silent_message_error, official_cli_fetch_concurrency,
    platform_label, profile_remark, session_history_mode, ConnectorAdapter, ConnectorKind,
    SessionDiscoveryStep,
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
    empty_session_warning,
    sessions_ready_message,
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

fn empty_session_warning(_profile: &ImProfile) -> Option<String> {
    Some(
        "企业微信：通讯录和内部群聊列表均为空，无法自动拉取消息。请确认当前授权用户可见通讯录和最近7天内有可读消息。"
            .to_owned(),
    )
}

fn sessions_ready_message(profile: &ImProfile, total: i64) -> String {
    format!(
        "已准备检查【{} · {}】{}个通讯录成员/群聊。",
        platform_label(&profile.platform),
        profile_remark(profile),
        total
    )
}

fn fetch_concurrency() -> usize {
    official_cli_fetch_concurrency()
}
