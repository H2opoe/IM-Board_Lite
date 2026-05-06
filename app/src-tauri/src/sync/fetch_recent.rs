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
                "正在读取【{} · {}】{} 个会话消息（并发 {}）…",
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
                    "正在读取【{}】消息…（{}/{})",
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
                    if is_wechat_key_incomplete_error(profile, &error.code) {
                        return Err(error.message);
                    }
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
                refresh_local_keyword_stats(app, state, profile, &window.day, warnings);
            }
            emit_sync_progress(
                app,
                profile,
                "fetch_messages_done",
                format!("已读取【{}】{} 条今天的消息。", job.chat_name, fetched),
                job.index as i64 + 1,
                total,
            );
        }
    }

    Ok((fetched_messages, inserted_messages))
}

fn is_wechat_key_incomplete_error(profile: &ImProfile, code: &str) -> bool {
    profile.platform == "wechat" && code == "WECHAT_KEYS_INCOMPLETE"
}
