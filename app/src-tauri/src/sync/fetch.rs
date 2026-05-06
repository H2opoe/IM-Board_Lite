use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use futures::future::join_all;
use tauri::State;

use crate::ai;
use crate::analysis::orchestrator::{
    emit_sync_progress, ensure_sync_not_cancelled, is_sync_cancelled_message,
};
use crate::bridge_runner::BridgeRequest;
use crate::connectors::{
    self, ConnectorAdapter, ProfileSyncLane, ProfileSyncMode, SessionDiscoveryStep,
};
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
    let mut serial_profiles = Vec::new();
    let mut concurrent_profiles = Vec::new();

    for profile in target_profiles {
        let connector = connector_for_profile(profile)?;
        match (connector.sync_lane)() {
            ProfileSyncLane::Serial => serial_profiles.push(profile.clone()),
            ProfileSyncLane::Concurrent => concurrent_profiles.push(profile.clone()),
        }
    }

    // connector 决定账号进入串行或并发车道；同步编排不再直接关心具体平台名。
    let (serial_outcomes, concurrent_outcomes) = tokio::join!(
        sync_serial_profiles_messages(
            app,
            state,
            serial_profiles,
            window,
            day_start,
            day_start_text,
            sync_end_text,
            resource_dir.clone(),
            cache_dir.clone(),
        ),
        sync_concurrent_profiles_messages(
            app,
            state,
            concurrent_profiles,
            window,
            day_start,
            day_start_text,
            sync_end_text,
            resource_dir,
            cache_dir,
        )
    );

    let mut outcomes = serial_outcomes?;
    outcomes.extend(concurrent_outcomes?);
    Ok(outcomes)
}

fn connector_for_profile(profile: &ImProfile) -> Result<ConnectorAdapter, String> {
    connectors::find(&profile.platform)
        .ok_or_else(|| format!("暂不支持{}账号同步。", profile.label))
}

async fn sync_serial_profiles_messages(
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
    for profile in profiles {
        ensure_sync_not_cancelled(state)?;
        outcomes.push(
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
            .await?,
        );
    }
    Ok(outcomes)
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
            let message = format!(
                "【{} · {}】同步失败：{}",
                platform_label(&profile.platform),
                profile_remark(&profile),
                error
            );
            emit_sync_progress(app, &profile, "profile_sync_failed", message.clone(), 0, 0);
            Err(message)
        }
    }
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
            warnings.push(format!("{}：{}", profile.label, error.message));
        }
        emit_profile_sync_done(app, &profile, inserted_messages);
        return Ok(ProfileSyncOutcome {
            inserted_messages,
            warnings,
        });
    }

    let mut all_sessions = contact_sessions;
    all_sessions.extend(value_array(&sessions.data).into_iter().cloned());
    let all_sessions = dedupe_sessions(all_sessions);
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

    let (_, profile_inserted_messages) = fetch_recent_session_messages(
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
            "已同步【{} · {}】，读取 {} 条新消息。",
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
