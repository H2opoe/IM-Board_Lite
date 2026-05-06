async fn fetch_context_history_for_targets_by_profile(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile_by_id: &HashMap<String, ImProfile>,
    day: &str,
    targets: &[ai::ContextBackfillTarget],
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
) -> Result<Vec<ai::HistoricalContextMessage>, String> {
    let mut grouped = HashMap::<String, Vec<ai::ContextBackfillTarget>>::new();
    for target in targets {
        grouped
            .entry(target.profile_id.clone())
            .or_default()
            .push(target.clone());
    }

    let mut history = Vec::new();
    for (profile_id, profile_targets) in grouped {
        let Some(profile) = profile_by_id.get(&profile_id) else {
            continue;
        };
        history.extend(
            fetch_context_history_for_targets(
                app,
                state,
                profile,
                day,
                &profile_targets,
                resource_dir.clone(),
                cache_dir.clone(),
            )
            .await?,
        );
    }
    Ok(history)
}

async fn backfill_context_for_requests_by_profile(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile_by_id: &HashMap<String, ImProfile>,
    day: &str,
    requests: &[ai::ContextBackfillRequest],
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
) -> Result<i64, String> {
    let mut grouped = HashMap::<String, Vec<ai::ContextBackfillRequest>>::new();
    for request in requests {
        grouped
            .entry(request.profile_id.clone())
            .or_default()
            .push(request.clone());
    }

    let mut completed = 0;
    for (profile_id, profile_requests) in grouped {
        let Some(profile) = profile_by_id.get(&profile_id) else {
            continue;
        };
        completed += backfill_context_for_requests(
            app,
            state,
            profile,
            day,
            &profile_requests,
            resource_dir.clone(),
            cache_dir.clone(),
        )
        .await?;
    }
    Ok(completed)
}

async fn backfill_context_for_requests(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile: &ImProfile,
    day: &str,
    requests: &[ai::ContextBackfillRequest],
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
) -> Result<i64, String> {
    let today = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|err| format!("无法解析补读日期：{err}"))?;
    let mut completed = 0;

    for request in requests {
        ensure_sync_not_cancelled(state)?;
        emit_sync_progress(
            app,
            profile,
            "context_backfill",
            format!("正在补读【{}】上下文证据…", request.chat_name),
            completed,
            requests.len() as i64,
        );

        let mut evidence = Vec::new();
        for offset in 1..=CONTEXT_BACKFILL_DAYS {
            if evidence.len() >= CONTEXT_EVIDENCE_LIMIT {
                break;
            }
            let target_day = today - Duration::days(offset);
            let target_day_text = target_day.format("%Y-%m-%d").to_string();
            let mut args = HashMap::new();
            args.insert("chat".to_owned(), request.chat_id.clone());
            args.insert("chat_name".to_owned(), request.chat_name.clone());
            args.insert(
                "limit".to_owned(),
                CONTEXT_BACKFILL_LIMIT_PER_DAY.to_string(),
            );
            args.insert(
                "start_time".to_owned(),
                format!("{target_day_text} 00:00:00"),
            );
            args.insert("end_time".to_owned(), format!("{target_day_text} 23:59:59"));

            let history = run_sync_bridge(
                state,
                BridgeRequest {
                    platform: profile.platform.clone(),
                    command: "fetch-messages".to_owned(),
                    profile: Some(profile.clone()),
                    args,
                    stdin_secret: None,
                },
                resource_dir.clone(),
                cache_dir.clone(),
            )
            .await
            .map_err(|err| err.to_string())?;

            if !history.ok {
                continue;
            }

            for message in value_array(&history.data) {
                if evidence.len() >= CONTEXT_EVIDENCE_LIMIT {
                    break;
                }
                if let Some(line) = normalize_context_evidence(
                    profile,
                    message,
                    &target_day_text,
                    &request.chat_id,
                    &request.chat_name,
                    request.is_group,
                ) {
                    evidence.push(line);
                }
            }
        }

        if evidence.is_empty() {
            continue;
        }
        apply_context_evidence(state, &request.action_id, &evidence)
            .map_err(|err| err.to_string())?;
        completed += 1;
    }

    Ok(completed)
}

async fn fetch_context_history_for_targets(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    profile: &ImProfile,
    day: &str,
    targets: &[ai::ContextBackfillTarget],
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
) -> Result<Vec<ai::HistoricalContextMessage>, String> {
    let today = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|err| format!("无法解析补读日期：{err}"))?;
    let mut history = Vec::new();

    for target in targets {
        ensure_sync_not_cancelled(state)?;
        emit_sync_progress(
            app,
            profile,
            "context_backfill",
            format!("正在补读【{}】历史上下文…", target.chat_name),
            history.len() as i64,
            CONTEXT_EVIDENCE_LIMIT as i64,
        );

        for offset in 1..=CONTEXT_BACKFILL_DAYS {
            if history.len() >= CONTEXT_EVIDENCE_LIMIT {
                break;
            }
            let target_day = today - Duration::days(offset);
            let target_day_text = target_day.format("%Y-%m-%d").to_string();
            let mut args = HashMap::new();
            args.insert("chat".to_owned(), target.chat_id.clone());
            args.insert("chat_name".to_owned(), target.chat_name.clone());
            args.insert(
                "limit".to_owned(),
                CONTEXT_BACKFILL_LIMIT_PER_DAY.to_string(),
            );
            args.insert(
                "start_time".to_owned(),
                format!("{target_day_text} 00:00:00"),
            );
            args.insert("end_time".to_owned(), format!("{target_day_text} 23:59:59"));

            let result = run_sync_bridge(
                state,
                BridgeRequest {
                    platform: profile.platform.clone(),
                    command: "fetch-messages".to_owned(),
                    profile: Some(profile.clone()),
                    args,
                    stdin_secret: None,
                },
                resource_dir.clone(),
                cache_dir.clone(),
            )
            .await
            .map_err(|err| err.to_string())?;

            if !result.ok {
                continue;
            }

            for message in value_array(&result.data) {
                if history.len() >= CONTEXT_EVIDENCE_LIMIT {
                    break;
                }
                if let Some(context_message) = normalize_historical_context_message(
                    profile,
                    message,
                    &target_day_text,
                    &target.chat_id,
                    &target.chat_name,
                    target.is_group,
                ) {
                    history.push(context_message);
                }
            }
        }
    }

    Ok(history)
}

fn normalize_context_evidence(
    profile: &ImProfile,
    value: &serde_json::Value,
    expected_day: &str,
    fallback_chat_id: &str,
    fallback_chat_name: &str,
    fallback_is_group: bool,
) -> Option<String> {
    let window = MessageImportWindow::natural_day(expected_day)?;
    let message = normalize_message(
        profile,
        value,
        &window,
        fallback_chat_id,
        fallback_chat_name,
        fallback_is_group,
    )?;
    Some(format!(
        "[{} {}] {}: {}",
        message.day,
        message.time_text,
        truncate_context_text(&message.sender_name, 24),
        truncate_context_text(&message.content, 120)
    ))
}

fn normalize_historical_context_message(
    profile: &ImProfile,
    value: &serde_json::Value,
    expected_day: &str,
    fallback_chat_id: &str,
    fallback_chat_name: &str,
    fallback_is_group: bool,
) -> Option<ai::HistoricalContextMessage> {
    let window = MessageImportWindow::natural_day(expected_day)?;
    let message = normalize_message(
        profile,
        value,
        &window,
        fallback_chat_id,
        fallback_chat_name,
        fallback_is_group,
    )?;
    Some(ai::HistoricalContextMessage::new(
        message.chat_id,
        message.chat_name,
        message.day,
        message.time_text,
        message.sender_name,
        message.content,
    ))
}

fn apply_context_evidence(
    state: &State<'_, AppState>,
    action_id: &str,
    evidence: &[String],
) -> anyhow::Result<()> {
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    apply_context_evidence_conn(&conn, action_id, evidence)
}

pub(crate) fn apply_context_evidence_conn(
    conn: &rusqlite::Connection,
    action_id: &str,
    evidence: &[String],
) -> anyhow::Result<()> {
    let existing = conn
        .query_row(
            "select evidence_summary from action_items where id = ?1",
            params![action_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_default();
    let context_text = format!("上下文补读：{}", evidence.join(" / "));
    let merged = merge_context_summary(&existing, &context_text, 1200);
    conn.execute(
        "update action_items
         set evidence_summary = ?1,
             context_incomplete = 0,
             last_updated_at = datetime('now')
         where id = ?2",
        params![merged, action_id],
    )?;
    Ok(())
}

fn merge_context_summary(existing: &str, incoming: &str, max_chars: usize) -> String {
    let existing = existing.trim();
    let incoming = incoming.trim();
    let merged = if existing.is_empty() {
        incoming.to_owned()
    } else if existing.contains(incoming) {
        existing.to_owned()
    } else {
        format!("{existing}；{incoming}")
    };
    truncate_context_text(&merged, max_chars)
}

fn truncate_context_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
