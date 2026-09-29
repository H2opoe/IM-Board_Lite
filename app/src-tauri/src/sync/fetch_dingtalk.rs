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
) -> Result<(usize, i64, Vec<String>), String> {
    emit_sync_progress(
        app,
        profile,
        "search_messages",
        format!(
            "正在读取【{} · {}】今天的钉钉消息…",
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
        let message = history
            .error
            .map(|error| error.message)
            .unwrap_or_else(|| "钉钉消息读取失败，平台未返回错误详情。".to_owned());
        return Ok((0, 0, vec![message]));
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
        refresh_local_keyword_stats(app, state, profile, &window.day, warnings);
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
            "已读取【{} · {}】{}条今天的消息，涉及{}个会话。",
            platform_label(&profile.platform),
            profile_remark(profile),
            fetched,
            chat_count
        ),
        fetched as i64,
        fetched as i64,
    );
    Ok((fetched, inserted, Vec::new()))
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
) -> Result<(usize, i64, Vec<String>), String> {
    let queries = (connector_for_profile(profile)?.fallback_group_search_queries)(profile);
    if queries.is_empty() {
        return Ok((0, 0, Vec::new()));
    }

    emit_sync_progress(
        app,
        profile,
        "search_groups",
        format!(
            "未读到钉钉消息，正在按账号信息查找【{} · {}】可读群聊…",
            platform_label(&profile.platform),
            profile_remark(profile)
        ),
        0,
        queries.len() as i64,
    );

    let mut errors = Vec::new();
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
            errors.push(
                searched
                    .error
                    .map(|error| error.message)
                    .unwrap_or_else(|| "钉钉群聊检索失败。".to_owned()),
            );
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
            format!("已按「{}」找到{}个钉钉可读会话。", query, chats.len()),
            index as i64 + 1,
            queries.len() as i64,
        );
    }

    if chats.is_empty() {
        return Ok((0, 0, errors));
    }

    let chat_total = chats.len() as i64;
    let (fetched, inserted, read_errors) = fetch_recent_session_messages(
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
    .await?;
    errors.extend(read_errors);
    Ok((fetched, inserted, errors))
}
