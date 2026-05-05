use super::{
    default_sessions_ready_message, no_prepare_profile_sync_access, official_cli_fetch_concurrency,
    ConnectorAdapter, ConnectorKind, ProfileSyncLane, ProfileSyncMode, SessionDiscoveryStep,
};
use crate::storage::models::ImProfile;

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "dingtalk",
    kind: ConnectorKind::Dingtalk,
    is_available,
    sync_lane,
    sync_mode,
    session_discovery_steps,
    prepare_profile_sync_access: no_prepare_profile_sync_access,
    empty_session_warning,
    sessions_ready_message: default_sessions_ready_message,
    fetch_concurrency,
    should_silence_message_error,
    fallback_group_search_queries,
};

fn is_available() -> bool {
    true
}

fn sync_lane() -> ProfileSyncLane {
    ProfileSyncLane::Concurrent
}

fn sync_mode() -> ProfileSyncMode {
    ProfileSyncMode::WindowMessagesWithGroupFallback
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

fn should_silence_message_error(code: &str) -> bool {
    // 钉钉官方/机器人/组织通知类会话可能能被检索到，但不开放消息读取。
    // 这类会话不影响其它普通会话同步，按不可读对象静默跳过。
    code == "DINGTALK_MESSAGE_PERMISSION_MISSING"
}

fn fallback_group_search_queries(profile: &ImProfile) -> Vec<String> {
    let mut queries = Vec::new();
    if let Some(items) = profile
        .config_json
        .get("syncSearchQueries")
        .and_then(|value| value.as_array())
    {
        for item in items {
            if let Some(query) = item.as_str() {
                push_search_query(&mut queries, query);
            }
        }
    }
    if let Some(query) = profile
        .config_json
        .get("syncSearchQuery")
        .and_then(|value| value.as_str())
    {
        push_search_query(&mut queries, query);
    }
    if let Some(identity) = profile.config_json.get("accountIdentity") {
        for key in ["orgName", "corpName", "tenantName", "userName"] {
            if let Some(query) = identity.get(key).and_then(|value| value.as_str()) {
                push_search_query(&mut queries, query);
            }
        }
    }
    if let Some(remark) = profile
        .config_json
        .get("remark")
        .and_then(|value| value.as_str())
    {
        push_search_query(&mut queries, remark);
    }
    if profile.label != "钉钉" {
        push_search_query(&mut queries, &profile.label);
    }
    queries
}

fn push_search_query(queries: &mut Vec<String>, query: &str) {
    let query = query.trim();
    if query.is_empty() || query == "钉钉" {
        return;
    }
    if !queries.iter().any(|item| item == query) {
        queries.push(query.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile() -> ImProfile {
        ImProfile {
            id: "profile-1".to_owned(),
            platform: "dingtalk".to_owned(),
            label: "钉钉测试".to_owned(),
            enabled: true,
            config_json: serde_json::json!({
                "remark": "销售群",
                "syncSearchQuery": "销售群",
                "syncSearchQueries": ["项目A", "钉钉", ""],
                "accountIdentity": {
                    "orgName": "组织",
                    "userName": "张三"
                }
            }),
            status: "normal".to_owned(),
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn dingtalk_message_permission_errors_are_silent_in_sync() {
        assert!(should_silence_message_error(
            "DINGTALK_MESSAGE_PERMISSION_MISSING"
        ));
        assert!(!should_silence_message_error("DINGTALK_NOT_AUTHENTICATED"));
    }

    #[test]
    fn fallback_group_search_queries_are_deduped_and_trimmed() {
        assert_eq!(
            fallback_group_search_queries(&test_profile()),
            vec![
                "项目A".to_owned(),
                "销售群".to_owned(),
                "组织".to_owned(),
                "张三".to_owned(),
                "钉钉测试".to_owned()
            ]
        );
    }
}
