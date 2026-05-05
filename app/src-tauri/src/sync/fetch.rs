use crate::connectors::{
    self, ConnectorAdapter, ProfileSyncLane, ProfileSyncMode, SessionDiscoveryStep,
};

#[derive(Debug, Clone)]
struct FetchJob {
    index: usize,
    chat_id: String,
    chat_name: String,
    is_group: bool,
    args: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct MessageImportWindow {
    day: String,
    start_timestamp: i64,
    end_timestamp: i64,
}

impl MessageImportWindow {
    fn from_dashboard_day(day: &daily_cache::DashboardDay) -> Self {
        Self {
            day: day.day.clone(),
            start_timestamp: day.day_start_timestamp,
            end_timestamp: day.sync_end_timestamp,
        }
    }

    fn natural_day(day: &str) -> Option<Self> {
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

    fn contains(&self, timestamp: i64) -> bool {
        timestamp >= self.start_timestamp && timestamp <= self.end_timestamp
    }
}

#[derive(Debug)]
struct ProfileSyncOutcome {
    inserted_messages: i64,
    warnings: Vec<String>,
}

async fn sync_target_profiles_messages(
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
            "正在读取【{} · {}】最近会话...",
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
                        "正在读取【{} · {}】通讯录...",
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
                        "正在检索【{} · {}】今天消息...",
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
            "【{} · {}】同步完成，已读取 {} 条新消息。",
            platform_label(&profile.platform),
            profile_remark(profile),
            inserted_messages
        ),
        inserted_messages,
        inserted_messages,
    );
}

async fn fetch_dingtalk_window_messages(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile: &ImProfile,
    window: &MessageImportWindow,
    day_start_text: &str,
    sync_end_text: &str,
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    warnings: &mut Vec<String>,
) -> Result<(usize, i64), String> {
    emit_sync_progress(
        app,
        profile,
        "search_messages",
        format!(
            "正在读取【{} · {}】今天钉钉消息...",
            platform_label(&profile.platform),
            profile_remark(profile)
        ),
        0,
        0,
    );

    let mut args = HashMap::new();
    args.insert("start_time".to_owned(), day_start_text.to_owned());
    args.insert("end_time".to_owned(), sync_end_text.to_owned());
    args.insert("limit".to_owned(), "200".to_owned());
    let history = run_sync_bridge(
        state,
        BridgeRequest {
            platform: profile.platform.clone(),
            command: "search-messages".to_owned(),
            profile: Some(profile.clone()),
            args,
            stdin_secret: None,
        },
        resource_dir.clone(),
        cache_dir.clone(),
    )
    .await
    .map_err(|err| err.to_string())?;

    warnings.extend(history.warnings);
    if !history.ok {
        if let Some(error) = history.error {
            if !(connector_for_profile(profile)?.should_silence_message_error)(&error.code) {
                warnings.push(format!(
                    "【{} · {}】今天消息：{}",
                    platform_label(&profile.platform),
                    profile_remark(profile),
                    error.message
                ));
            }
        }
        return Ok((0, 0));
    }

    let mut messages = Vec::new();
    for message in value_array(&history.data) {
        if let Some(record) = normalize_message(profile, message, window, "", "钉钉", true) {
            messages.push(record);
        }
    }
    if messages.is_empty() {
        return fetch_dingtalk_discovered_group_messages(
            app,
            state,
            profile,
            window,
            day_start_text,
            sync_end_text,
            resource_dir,
            cache_dir,
            warnings,
        )
        .await;
    }
    let fetched = messages.len();
    let inserted = insert_messages(state, &messages).map_err(|err| err.to_string())?;
    if inserted > 0 {
        refresh_local_keyword_stats(state, profile, &window.day, warnings);
    }
    let chat_count = messages
        .iter()
        .map(|message| message.chat_id.as_str())
        .collect::<HashSet<_>>()
        .len();
    emit_sync_progress(
        app,
        profile,
        "fetch_messages_done",
        format!(
            "已读取【{} · {}】{} 条今天消息，涉及 {} 个会话。",
            platform_label(&profile.platform),
            profile_remark(profile),
            fetched,
            chat_count
        ),
        fetched as i64,
        fetched as i64,
    );
    Ok((fetched, inserted))
}

async fn fetch_dingtalk_discovered_group_messages(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile: &ImProfile,
    window: &MessageImportWindow,
    day_start_text: &str,
    sync_end_text: &str,
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    warnings: &mut Vec<String>,
) -> Result<(usize, i64), String> {
    let queries = (connector_for_profile(profile)?.fallback_group_search_queries)(profile);
    if queries.is_empty() {
        return Ok((0, 0));
    }

    emit_sync_progress(
        app,
        profile,
        "search_groups",
        format!(
            "未在全量窗口读到钉钉消息，正在按账号信息查找【{} · {}】可读群聊...",
            platform_label(&profile.platform),
            profile_remark(profile)
        ),
        0,
        queries.len() as i64,
    );

    let mut chats = Vec::new();
    let mut seen = HashSet::new();
    for (index, query) in queries.iter().enumerate() {
        ensure_sync_not_cancelled(state)?;
        let mut args = HashMap::new();
        args.insert("query".to_owned(), query.clone());
        let searched = run_sync_bridge(
            state,
            BridgeRequest {
                platform: profile.platform.clone(),
                command: "search-groups".to_owned(),
                profile: Some(profile.clone()),
                args,
                stdin_secret: None,
            },
            resource_dir.clone(),
            cache_dir.clone(),
        )
        .await
        .map_err(|err| err.to_string())?;
        warnings.extend(searched.warnings);
        if !searched.ok {
            if let Some(error) = searched.error {
                if !(connector_for_profile(profile)?.should_silence_message_error)(&error.code) {
                    warnings.push(format!(
                        "{} 群聊检索「{}」：{}",
                        profile.label, query, error.message
                    ));
                }
            }
            continue;
        }
        for chat in value_array(&searched.data) {
            let Some(chat_id) = first_string(
                chat,
                &[
                    "chatId",
                    "chat_id",
                    "openConversationId",
                    "conversationId",
                    "id",
                ],
            ) else {
                continue;
            };
            if seen.insert(chat_id) {
                chats.push(chat.clone());
            }
        }
        emit_sync_progress(
            app,
            profile,
            "search_groups",
            format!("已按「{}」找到 {} 个钉钉可读会话。", query, chats.len()),
            index as i64 + 1,
            queries.len() as i64,
        );
    }

    if chats.is_empty() {
        return Ok((0, 0));
    }

    let chat_total = chats.len() as i64;
    fetch_recent_session_messages(
        app,
        state,
        profile,
        chats,
        window,
        day_start_text,
        sync_end_text,
        chat_total,
        resource_dir,
        cache_dir,
        warnings,
    )
    .await
}

async fn fetch_recent_session_messages(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile: &ImProfile,
    recent_sessions: Vec<serde_json::Value>,
    window: &MessageImportWindow,
    day_start_text: &str,
    sync_end_text: &str,
    total: i64,
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    warnings: &mut Vec<String>,
) -> Result<(usize, i64), String> {
    let local_latest_by_chat = latest_saved_message_timestamps(state, profile, &window.day)
        .map_err(|err| err.to_string())?;
    let mut jobs = Vec::new();
    for (index, session) in recent_sessions.into_iter().enumerate() {
        ensure_sync_not_cancelled(state)?;
        let Some(chat_id) = first_string(
            &session,
            &["chatId", "chat_id", "username", "userName", "talker", "id"],
        ) else {
            continue;
        };
        let chat_name = first_string(
            &session,
            &[
                "chatName",
                "chat_name",
                "nickname",
                "nickName",
                "remark",
                "displayName",
                "name",
                "chat",
            ],
        )
        .unwrap_or_else(|| chat_id.clone());
        let is_group = chat_id.ends_with("@chatroom")
            || bool_value(&session, &["isGroup", "is_group", "group"]).unwrap_or(false);
        if should_skip_chat_history(
            &local_latest_by_chat,
            &chat_id,
            session_last_message_timestamp(&session),
        ) {
            emit_sync_progress(
                app,
                profile,
                "fetch_messages_skipped",
                format!("【{}】没有新消息，已跳过历史拉取。", chat_name),
                index as i64 + 1,
                total,
            );
            continue;
        }

        let mut args = HashMap::new();
        args.insert("chat".to_owned(), chat_id.clone());
        args.insert("chat_name".to_owned(), chat_name.clone());
        args.insert(
            "chat_type".to_owned(),
            if is_group { "2" } else { "1" }.to_owned(),
        );
        args.insert("limit".to_owned(), "300".to_owned());
        args.insert("start_time".to_owned(), day_start_text.to_owned());
        args.insert("end_time".to_owned(), sync_end_text.to_owned());
        jobs.push(FetchJob {
            index,
            chat_id,
            chat_name,
            is_group,
            args,
        });
    }

    if jobs.is_empty() {
        return Ok((0, 0));
    }

    let connector = connector_for_profile(profile)?;
    let concurrency = (connector.fetch_concurrency)();
    if concurrency > 1 {
        emit_sync_progress(
            app,
            profile,
            "fetch_messages_parallel",
            format!(
                "正在读取【{} · {}】{} 个会话消息（并发 {}）...",
                platform_label(&profile.platform),
                profile_remark(profile),
                jobs.len(),
                concurrency
            ),
            0,
            total,
        );
    }

    let mut fetched_messages = 0usize;
    let mut inserted_messages = 0i64;
    for chunk in jobs.chunks(concurrency) {
        ensure_sync_not_cancelled(state)?;
        let futures = chunk.iter().map(|job| {
            emit_sync_progress(
                app,
                profile,
                "fetch_messages",
                format!(
                    "正在读取【{}】消息...（{}/{})",
                    job.chat_name,
                    job.index + 1,
                    total
                ),
                job.index as i64 + 1,
                total,
            );
            run_sync_bridge(
                state,
                BridgeRequest {
                    platform: profile.platform.clone(),
                    command: "fetch-messages".to_owned(),
                    profile: Some(profile.clone()),
                    args: job.args.clone(),
                    stdin_secret: None,
                },
                resource_dir.clone(),
                cache_dir.clone(),
            )
        });

        let histories = join_all(futures).await;
        for (job, history) in chunk.iter().zip(histories) {
            ensure_sync_not_cancelled(state)?;
            let history = history.map_err(|err| err.to_string())?;
            warnings.extend(history.warnings);
            if !history.ok {
                if let Some(error) = history.error {
                    if !(connector.should_silence_message_error)(&error.code) {
                        warnings.push(format!(
                            "{} / {}：{}",
                            profile.label, job.chat_name, error.message
                        ));
                    }
                }
                continue;
            }

            let mut chat_messages = Vec::new();
            for message in value_array(&history.data) {
                if let Some(record) = normalize_message(
                    profile,
                    message,
                    window,
                    &job.chat_id,
                    &job.chat_name,
                    job.is_group,
                ) {
                    chat_messages.push(record);
                }
            }
            let fetched = chat_messages.len();
            fetched_messages += fetched;
            let inserted = insert_messages(state, &chat_messages).map_err(|err| err.to_string())?;
            inserted_messages += inserted;
            if inserted > 0 {
                refresh_local_keyword_stats(state, profile, &window.day, warnings);
            }
            emit_sync_progress(
                app,
                profile,
                "fetch_messages_done",
                format!("已读取【{}】{} 条今天消息。", job.chat_name, fetched),
                job.index as i64 + 1,
                total,
            );
        }
    }

    Ok((fetched_messages, inserted_messages))
}

fn refresh_local_keyword_stats(
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
            ai::persist_local_keyword_stats(&conn, day, &profile.id).map_err(|err| err.to_string())
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
