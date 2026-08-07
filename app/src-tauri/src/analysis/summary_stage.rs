async fn run_summary_stage(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    day: &str,
    target_profiles: &[ImProfile],
    ai_config: &crate::storage::models::AiConfig,
    ai_configured: bool,
    analysis_message_count: usize,
    analysis_scope_label: &str,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let keyword_refine_plan: Option<ai::KeywordRefinePlan> = if ai_configured {
        let profile_ids = target_profiles
            .iter()
            .map(|profile| profile.id.clone())
            .collect::<Vec<_>>();
        let plan = {
            let conn = state.db.lock().map_err(|err| err.to_string())?;
            ai::build_keyword_refine_plan(&conn, day, &profile_ids, analysis_message_count)
                .map_err(|err| err.to_string())?
        };
        if let Some(plan) = &plan {
            let conn = state.db.lock().map_err(|err| err.to_string())?;
            ai::mark_keyword_refine_pending(&conn, day, plan).map_err(|err| err.to_string())?;
        }
        plan
    } else {
        None
    };
    if !ai_configured || target_profiles.is_empty() {
        return Ok(());
    }

    let summary_profile_id = analysis_scope_profile_id(target_profiles);
    ensure_sync_not_cancelled(state)?;
    emit_summary_progress_for_scope(
        app,
        target_profiles,
        "summary",
        format!("正在识别{}热门话题和关键词…", analysis_scope_label),
        0,
        0,
    );
    let profile_ids = target_profiles
        .iter()
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();
    let summary_candidates = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        if target_profiles.len() > 1 {
            ai::load_summary_candidates_for_profiles(&conn, day, &profile_ids)
        } else {
            ai::load_summary_candidates(&conn, day, &summary_profile_id)
        }
        .map_err(|err| err.to_string())?
    };
    if summary_candidates.is_empty() && keyword_refine_plan.is_none() {
        emit_summary_progress_for_scope(
            app,
            target_profiles,
            "summary_done",
            format!(
                "{}暂无新的热门话题和关键词候选，正在刷新看板…",
                analysis_scope_label
            ),
            0,
            0,
        );
        return Ok(());
    }

    let existing_topics = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        ai::load_existing_summary_topics(&conn, day, &summary_profile_id)
            .map_err(|err| err.to_string())?
    };
    let mut summary_batches = ai::split_summary_candidate_batches(summary_candidates);
    if summary_batches.is_empty() {
        summary_batches.push(Vec::new());
    }
    let total_summary_batches = summary_batches.len();
    let mut rolling_topics = existing_topics;
    for (summary_batch_index, summary_batch) in summary_batches.into_iter().enumerate() {
        ensure_sync_not_cancelled(state)?;
        let is_last_summary_batch = summary_batch_index + 1 == total_summary_batches;
        let keyword_refine_for_batch = keyword_refine_plan
            .as_ref()
            .filter(|_| is_last_summary_batch);
        emit_summary_progress_for_scope(
            app,
            target_profiles,
            "summary",
            format!(
                "正在识别{}热门话题和关键词第{}/{}批…",
                analysis_scope_label,
                summary_batch_index + 1,
                total_summary_batches
            ),
            (summary_batch_index + 1) as i64,
            total_summary_batches as i64,
        );
        let summary_run_id = start_ai_analysis_run(
            state,
            day,
            &summary_profile_id,
            "summary",
            Vec::new(),
            summary_batch
                .iter()
                .map(|candidate| candidate.source_message_count())
                .sum::<usize>()
                + keyword_refine_for_batch
                    .map(keyword_refine_message_count)
                    .unwrap_or_default(),
            ai_config,
            Some(summary_batch_index + 1),
            Some(total_summary_batches),
            0,
            0,
            0,
            0,
        );
        let summary = match await_ai_call_with_runtime_recovery(app, state, ai_config, |config| {
            let config = config.clone();
            let day = day.to_owned();
            let profile_id = summary_profile_id.clone();
            let topics = rolling_topics.clone();
            let candidates = summary_batch.clone();
            let keyword_refine = keyword_refine_for_batch.map(|plan| plan.payload.clone());
            Box::pin(async move {
                ai::request_profile_summary(
                    &config,
                    &day,
                    &profile_id,
                    &topics,
                    &candidates,
                    keyword_refine.as_ref(),
                )
                .await
            })
        })
        .await
        {
            Ok(result) => {
                finish_ai_analysis_run(
                    state,
                    summary_run_id,
                    "done",
                    None,
                    serde_json::to_value(&result.diagnostics).ok(),
                );
                result.summary
            }
            Err(err) => {
                let user_message = crate::diagnostics::classify_ai_user_message(
                    &err.message,
                    err.diagnostic.as_ref(),
                );
                if let Some(plan) = keyword_refine_for_batch {
                    if let Ok(conn) = state.db.lock() {
                        let _ = ai::mark_keyword_refine_failed(&conn, day, plan);
                    }
                }
                finish_ai_analysis_run(
                    state,
                    summary_run_id,
                    "failed",
                    Some(&err.message),
                    err.diagnostic,
                );
                warnings.push(format!(
                    "{}热门话题和关键词识别失败（第{}/{}批）：{}",
                    analysis_scope_label,
                    summary_batch_index + 1,
                    total_summary_batches,
                    user_message
                ));
                return Ok(());
            }
        };
        rolling_topics = ai::summary_topic_contexts_from_summary(&summary);

        let persist_result = {
            let conn = state.db.lock().map_err(|err| err.to_string())?;
            if let Some(plan) = keyword_refine_for_batch {
                match ai::persist_refined_keywords_from_analysis(
                    &conn,
                    day,
                    plan,
                    &summary.keywords,
                ) {
                    Ok(0) => {
                        let _ = ai::mark_keyword_refine_failed(&conn, day, plan);
                    }
                    Ok(_) => {}
                    Err(err) => {
                        let _ = ai::mark_keyword_refine_failed(&conn, day, plan);
                        warnings.push(format!("关键词AI识别结果保存失败：{}", err));
                    }
                }
            }
            ai::persist_summary_stats(
                &conn,
                day,
                &summary_profile_id,
                summary.clone(),
                &summary_batch,
            )
        };
        if let Err(err) = persist_result {
            warnings.push(format!(
                "{}热门话题和关键词保存失败（第{}/{}批）：{}",
                analysis_scope_label,
                summary_batch_index + 1,
                total_summary_batches,
                err
            ));
        } else {
            emit_summary_progress_for_scope(
                app,
                target_profiles,
                "summary_done",
                format!(
                    "已更新{}热门话题和关键词第{}/{}批，正在刷新看板…",
                    analysis_scope_label,
                    summary_batch_index + 1,
                    total_summary_batches
                ),
                (summary_batch_index + 1) as i64,
                total_summary_batches as i64,
            );
        }
    }
    Ok(())
}
