// Kept explicit until the shared context migration reaches the fallback module.
#[allow(clippy::too_many_arguments)]
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
        return Err(history
            .error
            .map(|error| error.message)
            .unwrap_or_else(|| "钉钉消息读取失败，平台未返回错误详情。".to_owned()));
    }
    ensure_sync_not_cancelled(state)?;
    if value_array(&history.data).is_empty() {
        // Identity lookup is only needed to classify imported messages. An empty
        // successful day must not produce an unrelated identity warning.
        emit_sync_progress(
            app,
            profile,
            "fetch_messages_done",
            format!(
                "已读取【{} · {}】0条今天的消息，涉及0个会话。",
                platform_label(&profile.platform),
                profile_remark(profile)
            ),
            0,
            0,
        );
        return Ok((0, 0, Vec::new()));
    }
    let identity = run_sync_bridge(
        state,
        BridgeRequest {
            platform: "dingtalk".into(),
            command: "get-self".into(),
            profile: Some(profile.clone()),
            args: HashMap::new(),
            stdin_secret: None,
        },
        resource_dir,
        cache_dir,
    )
    .await
    .map_err(|err| err.to_string())?;
    ensure_sync_not_cancelled(state)?;
    warnings.extend(identity.warnings);
    let self_ids = if identity.ok {
        identity
            .meta
            .get("selfIds")
            .and_then(serde_json::Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| id.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_else(|| {
                crate::messages::account_identity::dingtalk_self_ids(
                    &identity.data,
                    &profile.config_json,
                )
            })
    } else {
        Vec::new()
    };
    if self_ids.is_empty() {
        warnings.push(
            "钉钉：未确认当前账号的消息身份标识，排行榜暂按已有身份统计，请检查授权后重试。".into(),
        );
    } else {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        crate::messages::account_identity::record_dingtalk_self_identity(
            &conn,
            &profile.id,
            &self_ids,
        )
        .map_err(|err| err.to_string())?;
    }
    let mut messages = Vec::new();
    for message in value_array(&history.data) {
        if let Some(record) = normalize_dingtalk_window_message(profile, message, window) {
            messages.push(record);
        }
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

fn normalize_dingtalk_window_message(
    profile: &ImProfile,
    message: &serde_json::Value,
    window: &MessageImportWindow,
) -> Option<crate::messages::normalizer::DailyMessage> {
    let is_group = bool_value(message, &["isGroup", "is_group"]).unwrap_or(false);
    normalize_message(profile, message, window, "", "钉钉", is_group)
}
#[cfg(test)]
mod dingtalk_window_projection_tests {
    use super::*;
    #[test]
    fn group_and_direct_flags_survive_the_window_import_path() {
        let profile = ImProfile {
            id: "fixture".into(),
            platform: "dingtalk".into(),
            label: "fixture".into(),
            enabled: true,
            config_json: serde_json::json!({}),
            status: "normal".into(),
            sort_order: 0,
            created_at: "".into(),
            updated_at: "".into(),
        };
        let window = MessageImportWindow {
            day: "2026-10-25".into(),
            start_timestamp: 1,
            end_timestamp: 100,
        };
        for group in [true, false] {
            let message = serde_json::json!({"timestamp":50,"chatId":"chat","senderId":"D_peer","senderName":"同事","isGroup":group,"content":"项目已确认"});
            let record = normalize_dingtalk_window_message(&profile, &message, &window).unwrap();
            assert_eq!(record.is_group, group);
            assert_eq!(record.sender_id, "D_peer");
        }
    }
}
