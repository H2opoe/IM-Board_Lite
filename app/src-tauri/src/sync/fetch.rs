use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use futures::future::join_all;
use tauri::State;

use crate::ai;
use crate::analysis::orchestrator::{
    emit_sync_progress, ensure_sync_not_cancelled, is_sync_cancelled_message,
};
use crate::bridge_runner::BridgeRequest;
use crate::connectors::{self, ConnectorAdapter, ProfileSyncMode, SessionDiscoveryStep};
use crate::messages::normalizer::{
    bool_value, dedupe_sessions, first_string, normalize_message, platform_label, profile_remark,
    session_last_message_timestamp, should_skip_chat_history, should_sync_session, value_array,
};
use crate::messages::repository::{insert_messages, latest_saved_message_timestamps};
use crate::storage::models::ImProfile;
use crate::storage::AppState;
use crate::sync::orchestrator::run_sync_bridge;

const CONCURRENT_PROFILE_SYNC_CONCURRENCY: usize = 4;

#[derive(Debug, Clone)]
struct FetchJob {
    index: usize,
    chat_id: String,
    chat_name: String,
    is_group: bool,
    args: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct MessageImportWindow {
    pub(crate) day: String,
    pub(crate) start_timestamp: i64,
    pub(crate) end_timestamp: i64,
}

impl MessageImportWindow {
    pub(crate) fn from_dashboard_day(day: &crate::daily_cache::DashboardDay) -> Self {
        Self {
            day: day.day.clone(),
            start_timestamp: day.day_start_timestamp,
            end_timestamp: day.sync_end_timestamp,
        }
    }

    pub(crate) fn natural_day(day: &str) -> Option<Self> {
        let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
        let start = Local
            .with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0)
            .single()?;
        let end = start + Duration::days(1) - Duration::seconds(1);
        Some(Self {
            day: day.to_owned(),
            start_timestamp: start.timestamp(),
            end_timestamp: end.timestamp(),
        })
    }

    pub(crate) fn contains(&self, timestamp: i64) -> bool {
        timestamp >= self.start_timestamp && timestamp <= self.end_timestamp
    }
}

#[derive(Debug)]
pub(crate) struct ProfileSyncOutcome {
    pub(crate) inserted_messages: i64,
    pub(crate) warnings: Vec<String>,
}

pub(crate) async fn sync_target_profiles_messages(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    target_profiles: &[ImProfile],
    window: &MessageImportWindow,
    day_start: i64,
    day_start_text: &str,
    sync_end_text: &str,
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
) -> Result<Vec<ProfileSyncOutcome>, String> {
    sync_concurrent_profiles_messages(
        app,
        state,
        target_profiles.to_vec(),
        window,
        day_start,
        day_start_text,
        sync_end_text,
        resource_dir,
        cache_dir,
    )
    .await
}

fn connector_for_profile(profile: &ImProfile) -> Result<ConnectorAdapter, String> {
    connectors::find(&profile.platform)
        .ok_or_else(|| format!("Lite版暂不支持{}账号同步。", profile.label))
}

async fn sync_concurrent_profiles_messages(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profiles: Vec<ImProfile>,
    window: &MessageImportWindow,
    day_start: i64,
    day_start_text: &str,
    sync_end_text: &str,
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
) -> Result<Vec<ProfileSyncOutcome>, String> {
    let mut outcomes = Vec::new();
    for chunk in profiles.chunks(CONCURRENT_PROFILE_SYNC_CONCURRENCY) {
        ensure_sync_not_cancelled(state)?;
        let futures = chunk.iter().cloned().map(|profile| {
            sync_profile_messages_with_notice(
                app,
                state,
                profile,
                window,
                day_start,
                day_start_text,
                sync_end_text,
                resource_dir.clone(),
                cache_dir.clone(),
            )
        });
        let results = join_all(futures).await;
        for result in results {
            outcomes.push(result?);
        }
    }
    Ok(outcomes)
}

async fn sync_profile_messages_with_notice(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile: ImProfile,
    window: &MessageImportWindow,
    day_start: i64,
    day_start_text: &str,
    sync_end_text: &str,
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
) -> Result<ProfileSyncOutcome, String> {
    match sync_profile_messages(
        app,
        state,
        profile.clone(),
        window,
        day_start,
        day_start_text,
        sync_end_text,
        resource_dir,
        cache_dir,
    )
    .await
    {
        Ok(outcome) => Ok(outcome),
        Err(error) if is_sync_cancelled_message(&error) => Err(error),
        Err(error) => {
            let message = profile_sync_failure_message(&profile, &error);
            emit_sync_progress(app, &profile, "profile_sync_failed", message.clone(), 0, 0);
            Ok(ProfileSyncOutcome {
                inserted_messages: 0,
                warnings: vec![message],
            })
        }
    }
}

fn profile_sync_failure_message(profile: &ImProfile, error: &str) -> String {
    format!(
        "【{} · {}】同步失败：{}",
        platform_label(&profile.platform),
        profile_remark(profile),
        error
    )
}

fn wechat_read_not_logged_in_message() -> &'static str {
    "微信读取不到消息，请确认电脑微信是否已登录后重试。"
}

fn is_wechat_empty_session_sync(profile: &ImProfile, session_count: usize) -> bool {
    profile.platform == "wechat" && session_count == 0
}

fn is_wechat_zero_message_sync(
    profile: &ImProfile,
    fetched_messages: usize,
    inserted_messages: i64,
) -> bool {
    profile.platform == "wechat" && fetched_messages == 0 && inserted_messages == 0
}

async fn sync_profile_messages(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile: ImProfile,
    window: &MessageImportWindow,
    day_start: i64,
    day_start_text: &str,
    sync_end_text: &str,
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
) -> Result<ProfileSyncOutcome, String> {
    let connector = connector_for_profile(&profile)?;
    let mut warnings = Vec::new();
    let mut inserted_messages = 0;

    ensure_sync_not_cancelled(state)?;
    (connector.prepare_profile_sync_access)(&profile);
    emit_sync_progress(
        app,
        &profile,
        "list_chats",
        format!(
            "正在读取【{} · {}】最近会话…",
            platform_label(&profile.platform),
            profile_remark(&profile)
        ),
        0,
        0,
    );

    if (connector.sync_mode)() == ProfileSyncMode::WindowMessagesWithGroupFallback {
        let (_fetched, inserted) = fetch_dingtalk_window_messages(
            app,
            state,
            &profile,
            window,
            day_start_text,
            sync_end_text,
            resource_dir,
            cache_dir,
            &mut warnings,
        )
        .await?;
        inserted_messages += inserted;
        emit_profile_sync_done(app, &profile, inserted_messages);
        return Ok(ProfileSyncOutcome {
            inserted_messages,
            warnings,
        });
    }

    let contact_sessions = collect_session_discovery_steps(
        app,
        state,
        &profile,
        connector,
        day_start_text,
        sync_end_text,
        resource_dir.clone(),
        cache_dir.clone(),
        &mut warnings,
    )
    .await?;

    let mut all_sessions = contact_sessions;
    if (connector.should_run_session_list)() {
        let mut session_args = HashMap::new();
        session_args.insert("limit".to_owned(), "200".to_owned());
        session_args.insert("start_time".to_owned(), day_start_text.to_owned());
        session_args.insert("end_time".to_owned(), sync_end_text.to_owned());
        let sessions = run_sync_bridge(
            state,
            BridgeRequest {
                platform: profile.platform.clone(),
                command: "list-chats".to_owned(),
                profile: Some(profile.clone()),
                args: session_args,
                stdin_secret: None,
            },
            resource_dir.clone(),
            cache_dir.clone(),
        )
        .await
        .map_err(|err| err.to_string())?;

        warnings.extend(sessions.warnings);
        if !sessions.ok {
            if let Some(error) = sessions.error {
                if is_wechat_not_logged_in_error(&profile, &error.code) {
                    return Err(wechat_read_not_logged_in_message().to_owned());
                }
                warnings.push(format!("{}：{}", profile.label, error.message));
            }
            emit_profile_sync_done(app, &profile, inserted_messages);
            return Ok(ProfileSyncOutcome {
                inserted_messages,
                warnings,
            });
        }
        all_sessions.extend(value_array(&sessions.data).into_iter().cloned());
    }
    let all_sessions = dedupe_sessions(all_sessions);
    if is_wechat_empty_session_sync(&profile, all_sessions.len()) {
        return Err(wechat_read_not_logged_in_message().to_owned());
    }
    if all_sessions.is_empty() {
        if let Some(warning) = (connector.empty_session_warning)(&profile) {
            warnings.push(warning);
        }
    }

    let recent_sessions = all_sessions
        .into_iter()
        .filter(should_sync_session)
        .filter(|session| {
            session_last_message_timestamp(session)
                .map(|timestamp| timestamp >= day_start)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    let total = recent_sessions.len() as i64;

    emit_sync_progress(
        app,
        &profile,
        "list_chats_done",
        (connector.sessions_ready_message)(&profile, total),
        0,
        total,
    );

    let (profile_fetched_messages, profile_inserted_messages) = fetch_recent_session_messages(
        app,
        state,
        &profile,
        recent_sessions,
        window,
        day_start_text,
        sync_end_text,
        total,
        resource_dir,
        cache_dir,
        &mut warnings,
    )
    .await?;
    if is_wechat_zero_message_sync(
        &profile,
        profile_fetched_messages,
        profile_inserted_messages,
    ) {
        return Err(wechat_read_not_logged_in_message().to_owned());
    }
    inserted_messages += profile_inserted_messages;
    emit_profile_sync_done(app, &profile, inserted_messages);

    Ok(ProfileSyncOutcome {
        inserted_messages,
        warnings,
    })
}

async fn collect_session_discovery_steps(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile: &ImProfile,
    connector: ConnectorAdapter,
    day_start_text: &str,
    sync_end_text: &str,
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    warnings: &mut Vec<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut sessions = Vec::new();
    for step in (connector.session_discovery_steps)() {
        match step {
            SessionDiscoveryStep::Contacts => {
                emit_sync_progress(
                    app,
                    profile,
                    "list_contacts",
                    format!(
                        "正在读取【{} · {}】通讯录…",
                        platform_label(&profile.platform),
                        profile_remark(profile)
                    ),
                    0,
                    0,
                );
                let contacts = run_sync_bridge(
                    state,
                    BridgeRequest {
                        platform: profile.platform.clone(),
                        command: "list-contacts".to_owned(),
                        profile: Some(profile.clone()),
                        args: HashMap::new(),
                        stdin_secret: None,
                    },
                    resource_dir.clone(),
                    cache_dir.clone(),
                )
                .await
                .map_err(|err| err.to_string())?;
                warnings.extend(contacts.warnings);
                if contacts.ok {
                    sessions.extend(value_array(&contacts.data).into_iter().cloned());
                } else if let Some(error) = contacts.error {
                    warnings.push(format!("{} 通讯录：{}", profile.label, error.message));
                }
            }
            SessionDiscoveryStep::WindowSearch => {
                emit_sync_progress(
                    app,
                    profile,
                    "search_messages",
                    format!(
                        "正在检索【{} · {}】今天的消息…",
                        platform_label(&profile.platform),
                        profile_remark(profile)
                    ),
                    0,
                    0,
                );
                let mut search_args = HashMap::new();
                search_args.insert("start_time".to_owned(), day_start_text.to_owned());
                search_args.insert("end_time".to_owned(), sync_end_text.to_owned());
                let searched_sessions = run_sync_bridge(
                    state,
                    BridgeRequest {
                        platform: profile.platform.clone(),
                        command: "search-messages".to_owned(),
                        profile: Some(profile.clone()),
                        args: search_args,
                        stdin_secret: None,
                    },
                    resource_dir.clone(),
                    cache_dir.clone(),
                )
                .await
                .map_err(|err| err.to_string())?;
                warnings.extend(searched_sessions.warnings);
                if searched_sessions.ok {
                    sessions.extend(value_array(&searched_sessions.data).into_iter().cloned());
                } else if let Some(error) = searched_sessions.error {
                    warnings.push(format!("{} 消息检索：{}", profile.label, error.message));
                }
            }
        }
    }
    Ok(sessions)
}

fn emit_profile_sync_done(app: &tauri::AppHandle, profile: &ImProfile, inserted_messages: i64) {
    emit_sync_progress(
        app,
        profile,
        "profile_sync_done",
        format!(
            "已同步【{} · {}】，读取{}条新消息。",
            platform_label(&profile.platform),
            profile_remark(profile),
            inserted_messages
        ),
        inserted_messages,
        inserted_messages,
    );
}

include!("fetch_dingtalk.rs");

include!("fetch_recent.rs");
pub(crate) fn refresh_local_keyword_stats(
    _app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile: &ImProfile,
    day: &str,
    warnings: &mut Vec<String>,
) {
    let result = state
        .db
        .lock()
        .map_err(|err| err.to_string())
        .and_then(|conn| {
            ai::persist_local_keyword_stats_with_status(&conn, day, &profile.id, "local_final")
                .map_err(|err| err.to_string())
        });
    if let Err(err) = result {
        warnings.push(format!(
            "【{} · {}】关键词更新失败：{}",
            platform_label(&profile.platform),
            profile_remark(profile),
            err
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn profile_sync_failure_message_keeps_platform_and_remark() {
        let profile = ImProfile {
            id: "wechat-1".to_owned(),
            platform: "wechat".to_owned(),
            label: "微信账号".to_owned(),
            enabled: true,
            config_json: json!({ "remark": "工作号" }),
            status: "active".to_owned(),
            sort_order: 0,
            created_at: "2026-05-07T00:00:00+08:00".to_owned(),
            updated_at: "2026-05-07T00:00:00+08:00".to_owned(),
        };

        assert_eq!(
            profile_sync_failure_message(&profile, "Bridge进程执行失败"),
            "【微信 · 工作号】同步失败：Bridge进程执行失败"
        );
    }

    #[test]
    fn profile_sync_failure_message_guides_wechat_login() {
        let profile = ImProfile {
            id: "wechat-1".to_owned(),
            platform: "wechat".to_owned(),
            label: "微信账号".to_owned(),
            enabled: true,
            config_json: json!({ "remark": "工作号" }),
            status: "active".to_owned(),
            sort_order: 0,
            created_at: "2026-05-07T00:00:00+08:00".to_owned(),
            updated_at: "2026-05-07T00:00:00+08:00".to_owned(),
        };

        assert_eq!(
            profile_sync_failure_message(&profile, wechat_read_not_logged_in_message()),
            "【微信 · 工作号】同步失败：微信读取不到消息，请确认电脑微信是否已登录后重试。"
        );
    }

    #[test]
    fn empty_wechat_sessions_require_login_check() {
        let wechat = ImProfile {
            id: "wechat-1".to_owned(),
            platform: "wechat".to_owned(),
            label: "微信账号".to_owned(),
            enabled: true,
            config_json: json!({ "remark": "工作号" }),
            status: "active".to_owned(),
            sort_order: 0,
            created_at: "2026-05-07T00:00:00+08:00".to_owned(),
            updated_at: "2026-05-07T00:00:00+08:00".to_owned(),
        };
        let feishu = ImProfile {
            platform: "feishu".to_owned(),
            ..wechat.clone()
        };

        assert!(is_wechat_empty_session_sync(&wechat, 0));
        assert!(!is_wechat_empty_session_sync(&wechat, 1));
        assert!(!is_wechat_empty_session_sync(&feishu, 0));
    }

    #[test]
    fn zero_wechat_messages_require_login_check() {
        let wechat = ImProfile {
            id: "wechat-1".to_owned(),
            platform: "wechat".to_owned(),
            label: "微信账号".to_owned(),
            enabled: true,
            config_json: json!({ "remark": "工作号" }),
            status: "active".to_owned(),
            sort_order: 0,
            created_at: "2026-05-07T00:00:00+08:00".to_owned(),
            updated_at: "2026-05-07T00:00:00+08:00".to_owned(),
        };
        let feishu = ImProfile {
            platform: "feishu".to_owned(),
            ..wechat.clone()
        };

        assert!(is_wechat_zero_message_sync(&wechat, 0, 0));
        assert!(!is_wechat_zero_message_sync(&wechat, 1, 0));
        assert!(!is_wechat_zero_message_sync(&wechat, 0, 1));
        assert!(!is_wechat_zero_message_sync(&feishu, 0, 0));
    }
}
