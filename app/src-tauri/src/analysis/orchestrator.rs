use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::Ordering;
use std::time::{Duration as StdDuration, Instant};

use chrono::Duration;
use rusqlite::params;
use tauri::{Emitter, State};
use uuid::Uuid;

use crate::ai;
use crate::bridge_runner::BridgeRequest;
use crate::commands::ai as ai_commands;
use crate::messages::normalizer::{normalize_message, platform_label, profile_remark, value_array};
use crate::storage::models::ImProfile;
use crate::storage::AppState;
use crate::sync::fetch::{refresh_local_keyword_stats, MessageImportWindow};
use crate::sync::job::SyncProgress;
use crate::sync::orchestrator::run_sync_bridge;

struct AiCallFailure {
    message: String,
    diagnostic: Option<serde_json::Value>,
}

struct AiRunTracker {
    id: String,
    started_at: Instant,
    diagnostic: serde_json::Value,
}

const CONTEXT_BACKFILL_DAYS: i64 = 7;
const CONTEXT_BACKFILL_LIMIT_PER_DAY: usize = 80;
const CONTEXT_EVIDENCE_LIMIT: usize = 12;
const SYNC_CANCELLED_MESSAGE: &str = "同步已终止。";

pub(crate) async fn analyze_pending_messages(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    day: &str,
    target_profiles: &[ImProfile],
    resource_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    warnings: &mut Vec<String>,
) -> Result<(String, i64), String> {
    let (ai_config, analysis_messages) = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        let config = ai::get_config(&conn).map_err(|err| err.to_string())?;
        let messages = if ai_commands::is_configured_for_current_runtime(&config, state) {
            let profile_ids = target_profiles
                .iter()
                .map(|profile| profile.id.clone())
                .collect::<Vec<_>>();
            ai::load_analysis_messages_for_profiles(&conn, day, &profile_ids)
                .map_err(|err| err.to_string())?
        } else {
            Vec::new()
        };
        (config, messages)
    };

    let ai_configured = ai_commands::is_configured_for_current_runtime(&ai_config, state);
    let mut analyzed_messages = 0;
    let mut ai_status = if ai_configured {
        "ready".to_owned()
    } else {
        "not_configured".to_owned()
    };

    for profile in target_profiles {
        ensure_sync_not_cancelled(state)?;
        refresh_local_keyword_stats(app, state, profile, day, warnings);
    }

    let analysis_message_count = analysis_messages.len();
    let analysis_prompt = ai::analysis_system_prompt(&ai_config);
    let fixed_request_tokens = ai::estimate_text_for_request(&analysis_prompt)
        + ai::estimate_text_for_request(ai_config.user_prompt.trim());
    let initial_analysis_batches = ai::split_analysis_batch_plans(
        analysis_messages,
        ai_config.analysis_batch_size,
        ai::analysis_batch_token_budget(&ai_config),
        fixed_request_tokens,
    );
    let analysis_batches = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        rebalance_analysis_batches_for_context(
            &conn,
            initial_analysis_batches,
            &ai_config,
            &analysis_prompt,
            fixed_request_tokens,
        )?
    };

    let profile_by_id = target_profiles
        .iter()
        .cloned()
        .map(|profile| (profile.id.clone(), profile))
        .collect::<HashMap<_, _>>();
    let analysis_profile_id = analysis_scope_profile_id(target_profiles);
    let analysis_scope_label = analysis_scope_label(target_profiles);
    let total_batches = analysis_batches.len();
    for (batch_index, plan) in analysis_batches.into_iter().enumerate() {
        ensure_sync_not_cancelled(state)?;
        let primary_messages = plan.primary_messages().to_vec();
        let estimated_message_tokens = plan.estimated_input_tokens;
        let overlap_message_count = plan.overlap_message_count;
        let messages = plan.messages;
        emit_analysis_progress_for_scope(app, target_profiles, batch_index + 1, total_batches);
        let mut context = {
            let conn = state.db.lock().map_err(|err| err.to_string())?;
            ai::load_analysis_context_for_messages(&conn, &messages)
                .map_err(|err| err.to_string())?
        };
        let estimated_request_tokens = ai::estimate_analysis_request_tokens(
            &analysis_prompt,
            ai_config.user_prompt.trim(),
            &messages,
            &context,
        );
        let run_id = start_ai_analysis_run(
            state,
            day,
            &analysis_profile_id,
            "analysis",
            messages
                .iter()
                .map(|message| message.id().to_owned())
                .collect(),
            primary_messages.len(),
            &ai_config,
            Some(batch_index + 1),
            Some(total_batches),
            estimated_request_tokens,
            ai::estimate_analysis_context_tokens(&context),
            estimated_message_tokens,
            overlap_message_count,
        );
        let analysis_result = await_ai_call_or_cancel(
            state,
            ai::request_profile_analysis(
                &ai_config,
                day,
                &analysis_profile_id,
                &messages,
                &context,
            ),
        )
        .await;
        match analysis_result {
            Ok(analysis_result) => {
                finish_ai_analysis_run(
                    state,
                    run_id,
                    "done",
                    None,
                    serde_json::to_value(&analysis_result.diagnostics).ok(),
                );
                let mut analysis = analysis_result.analysis;
                let context_targets = {
                    let conn = state.db.lock().map_err(|err| err.to_string())?;
                    ai::context_backfill_targets_for_messages(&conn, day, &messages, &analysis)
                        .map_err(|err| err.to_string())?
                };
                // 本地模型优先保证看板刷新速度：历史证据补读放到保存后的事项级补全里处理，
                // 避免同一批消息因为二次 AI 精修阻塞后续批次。
                let should_inline_context_refine =
                    !context_targets.is_empty() && !ai::is_local_provider(&ai_config);
                if should_inline_context_refine {
                    match fetch_context_history_for_targets_by_profile(
                        app,
                        state,
                        &profile_by_id,
                        day,
                        &context_targets,
                        resource_dir.clone(),
                        cache_dir.clone(),
                    )
                    .await
                    {
                        Ok(history) if !history.is_empty() => {
                            let history_count = history.len();
                            ai::extend_analysis_context_with_history(&mut context, history);
                            let refine_run_id = start_ai_analysis_run(
                                state,
                                day,
                                &analysis_profile_id,
                                "analysis_context_refine",
                                messages
                                    .iter()
                                    .map(|message| message.id().to_owned())
                                    .collect(),
                                primary_messages.len(),
                                &ai_config,
                                Some(batch_index + 1),
                                Some(total_batches),
                                ai::estimate_analysis_request_tokens(
                                    &analysis_prompt,
                                    ai_config.user_prompt.trim(),
                                    &messages,
                                    &context,
                                ),
                                ai::estimate_analysis_context_tokens(&context),
                                estimated_message_tokens,
                                overlap_message_count,
                            );
                            match await_ai_call_or_cancel(
                                state,
                                ai::request_profile_analysis(
                                    &ai_config,
                                    day,
                                    &analysis_profile_id,
                                    &messages,
                                    &context,
                                ),
                            )
                            .await
                            {
                                Ok(refined_result) => {
                                    finish_ai_analysis_run(
                                        state,
                                        refine_run_id,
                                        "done",
                                        None,
                                        serde_json::to_value(&refined_result.diagnostics).ok(),
                                    );
                                    analysis = refined_result.analysis;
                                    warnings.push(format!(
                                        "{}已补充 {} 条历史上下文并重新分析。",
                                        analysis_scope_label, history_count
                                    ));
                                }
                                Err(err) => {
                                    finish_ai_analysis_run(
                                        state,
                                        refine_run_id,
                                        "failed",
                                        Some(&err.message),
                                        err.diagnostic,
                                    );
                                    warnings.push(format!(
                                        "{}历史上下文重新分析失败，已保留首次AI结果：{}",
                                        analysis_scope_label, err.message
                                    ));
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(err) => warnings.push(format!(
                            "{}历史上下文补读失败，已保留首次AI结果：{}",
                            analysis_scope_label, err
                        )),
                    }
                }
                let persist_result = {
                    let conn = state.db.lock().map_err(|err| err.to_string())?;
                    ai::persist_analysis_for_messages(&conn, day, &primary_messages, analysis)
                };
                match persist_result {
                    Ok(persisted) => {
                        analyzed_messages += persisted.affected_action_items;
                        emit_analysis_done_for_scope(
                            app,
                            target_profiles,
                            batch_index + 1,
                            total_batches,
                        );
                        if !persisted.context_requests.is_empty() {
                            match backfill_context_for_requests_by_profile(
                                app,
                                state,
                                &profile_by_id,
                                day,
                                &persisted.context_requests,
                                resource_dir.clone(),
                                cache_dir.clone(),
                            )
                            .await
                            {
                                Ok(0) => {}
                                Ok(count) => warnings.push(format!(
                                    "{}已为 {} 个事项补读历史证据。",
                                    analysis_scope_label, count
                                )),
                                Err(err) => warnings.push(format!(
                                    "{}上下文补读失败：{}",
                                    analysis_scope_label, err
                                )),
                            }
                        }
                    }
                    Err(err) => {
                        if let Ok(conn) = state.db.lock() {
                            let _ = ai::keep_analysis_pending_for_messages(
                                &conn,
                                day,
                                &primary_messages,
                            );
                        }
                        ai_status = "failed".to_owned();
                        warnings.push(format!(
                            "{}待回复和待办事项识别结果保存失败（第 {}/{} 批）：{}",
                            analysis_scope_label,
                            batch_index + 1,
                            total_batches,
                            err
                        ));
                    }
                }
            }
            Err(err) => {
                finish_ai_analysis_run(state, run_id, "failed", Some(&err.message), err.diagnostic);
                if let Ok(conn) = state.db.lock() {
                    let _ = ai::keep_analysis_pending_for_messages(&conn, day, &primary_messages);
                }
                ai_status = "failed".to_owned();
                warnings.push(format!(
                    "{}待回复和待办事项识别失败（第 {}/{} 批）：{}",
                    analysis_scope_label,
                    batch_index + 1,
                    total_batches,
                    err.message
                ));
            }
        }
    }

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
    if ai_configured {
        let summary_profile_id = analysis_scope_profile_id(target_profiles);
        if !target_profiles.is_empty() {
            ensure_sync_not_cancelled(state)?;
            emit_summary_progress_for_scope(
                app,
                target_profiles,
                "summary",
                format!("正在识别{}热门话题和关键词...", analysis_scope_label),
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
                        "{}暂无新的热门话题和关键词候选，正在刷新看板...",
                        analysis_scope_label
                    ),
                    0,
                    0,
                );
                return Ok((ai_status, analyzed_messages));
            }
            let existing_topics = {
                let conn = state.db.lock().map_err(|err| err.to_string())?;
                ai::load_existing_summary_topics(&conn, day, &summary_profile_id)
                    .map_err(|err| err.to_string())?
            };
            let mut summary_batches =
                ai::split_summary_candidate_batches(summary_candidates.clone());
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
                        "正在识别{}热门话题和关键词第 {}/{} 批...",
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
                    &ai_config,
                    Some(summary_batch_index + 1),
                    Some(total_summary_batches),
                    0,
                    0,
                    0,
                    0,
                );
                let summary = match await_ai_call_or_cancel(
                    state,
                    ai::request_profile_summary(
                        &ai_config,
                        day,
                        &summary_profile_id,
                        &rolling_topics,
                        &summary_batch,
                        keyword_refine_for_batch.map(|plan| &plan.payload),
                    ),
                )
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
                            "{}热门话题和关键词识别失败（第 {}/{} 批）：{}",
                            analysis_scope_label,
                            summary_batch_index + 1,
                            total_summary_batches,
                            err.message
                        ));
                        return Ok((ai_status, analyzed_messages));
                    }
                };
                // 每批汇总结果都会作为下一批 existingTopics，确保跨批话题继续在 AI 层合并而不是各算各的。
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
                                warnings.push(format!("关键词 AI 识别结果保存失败：{}", err));
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
                        "{}热门话题和关键词保存失败（第 {}/{} 批）：{}",
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
                            "已更新{}热门话题和关键词第 {}/{} 批，正在刷新看板...",
                            analysis_scope_label,
                            summary_batch_index + 1,
                            total_summary_batches
                        ),
                        (summary_batch_index + 1) as i64,
                        total_summary_batches as i64,
                    );
                }
            }
        }
    }

    Ok((ai_status, analyzed_messages))
}

fn analysis_scope_profile_id(target_profiles: &[ImProfile]) -> String {
    if target_profiles.len() == 1 {
        target_profiles[0].id.clone()
    } else {
        "aggregate".to_owned()
    }
}

fn keyword_refine_message_count(plan: &ai::KeywordRefinePlan) -> usize {
    plan.payload
        .get("messages")
        .and_then(|value| value.as_array())
        .map(Vec::len)
        .unwrap_or_default()
}

fn rebalance_analysis_batches_for_context(
    conn: &rusqlite::Connection,
    batches: Vec<ai::AnalysisBatchPlan>,
    ai_config: &crate::storage::models::AiConfig,
    analysis_prompt: &str,
    fixed_request_tokens: usize,
) -> Result<Vec<ai::AnalysisBatchPlan>, String> {
    let token_budget = ai::analysis_batch_token_budget(ai_config);
    let mut balanced = Vec::new();
    for batch in batches {
        let context = ai::load_analysis_context_for_messages(conn, &batch.messages)
            .map_err(|err| err.to_string())?;
        let estimated_request_tokens = ai::estimate_analysis_request_tokens(
            analysis_prompt,
            ai_config.user_prompt.trim(),
            &batch.messages,
            &context,
        );
        if estimated_request_tokens <= token_budget || batch.primary_message_count() <= 1 {
            balanced.push(batch);
            continue;
        }
        let context_tokens = ai::estimate_analysis_context_tokens(&context);
        let mut smaller_batches = ai::split_analysis_batch_plans(
            batch.messages,
            ai_config.analysis_batch_size,
            token_budget,
            fixed_request_tokens + context_tokens,
        );
        if smaller_batches.len() <= 1 {
            balanced.extend(smaller_batches);
            continue;
        }
        // 上下文较重时再做一次装箱，让每批按完整请求预算而不是只按消息正文均衡。
        balanced.append(&mut smaller_batches);
    }
    Ok(balanced)
}

fn analysis_scope_label(target_profiles: &[ImProfile]) -> String {
    if target_profiles.len() == 1 {
        format!(
            "【{} · {}】",
            platform_label(&target_profiles[0].platform),
            profile_remark(&target_profiles[0])
        )
    } else {
        "全平台".to_owned()
    }
}

fn emit_analysis_progress_for_scope(
    app: &tauri::AppHandle,
    target_profiles: &[ImProfile],
    batch_index: usize,
    total_batches: usize,
) {
    if target_profiles.len() == 1 {
        emit_analysis_progress(app, &target_profiles[0], batch_index, total_batches);
        return;
    }
    let _ = app.emit(
        "sync-progress",
        SyncProgress::new(
            "aggregate".to_owned(),
            "全平台".to_owned(),
            "analysis",
            format!(
                "正在识别全平台待回复和待办事项第 {}/{} 批...",
                batch_index, total_batches
            ),
            batch_index as i64,
            total_batches as i64,
        ),
    );
}

fn emit_analysis_done_for_scope(
    app: &tauri::AppHandle,
    target_profiles: &[ImProfile],
    batch_index: usize,
    total_batches: usize,
) {
    if target_profiles.len() == 1 {
        emit_sync_progress(
            app,
            &target_profiles[0],
            "analysis_done",
            format!(
                "已完成【{} · {}】待回复和待办事项识别第 {}/{} 批，正在更新看板...",
                platform_label(&target_profiles[0].platform),
                profile_remark(&target_profiles[0]),
                batch_index,
                total_batches
            ),
            batch_index as i64,
            total_batches as i64,
        );
        return;
    }
    let _ = app.emit(
        "sync-progress",
        SyncProgress::new(
            "aggregate".to_owned(),
            "全平台".to_owned(),
            "analysis_done",
            format!(
                "已完成全平台待回复和待办事项识别第 {}/{} 批，正在更新看板...",
                batch_index, total_batches
            ),
            batch_index as i64,
            total_batches as i64,
        ),
    );
}

fn emit_summary_progress_for_scope(
    app: &tauri::AppHandle,
    target_profiles: &[ImProfile],
    phase: &str,
    message: String,
    current: i64,
    total: i64,
) {
    if target_profiles.len() == 1 {
        emit_sync_progress(app, &target_profiles[0], phase, message, current, total);
        return;
    }
    let _ = app.emit(
        "sync-progress",
        SyncProgress::new(
            "aggregate".to_owned(),
            "全平台".to_owned(),
            phase,
            message,
            current,
            total,
        ),
    );
}

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
            format!("正在补读【{}】上下文证据...", request.chat_name),
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
            format!("正在补读【{}】历史上下文...", target.chat_name),
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

pub(crate) fn ensure_sync_not_cancelled(state: &State<'_, AppState>) -> Result<(), String> {
    if state.sync_cancel_requested.load(Ordering::SeqCst) {
        Err(SYNC_CANCELLED_MESSAGE.to_owned())
    } else {
        Ok(())
    }
}

pub(crate) fn is_sync_cancelled_message(message: &str) -> bool {
    message == SYNC_CANCELLED_MESSAGE
}

async fn await_ai_call_or_cancel<T, F>(
    state: &State<'_, AppState>,
    future: F,
) -> Result<T, AiCallFailure>
where
    F: Future<Output = anyhow::Result<T>>,
{
    tokio::pin!(future);
    loop {
        tokio::select! {
            result = &mut future => {
                return result.map_err(|err| {
                    let diagnostic = err
                        .downcast_ref::<ai::AiCallError>()
                        .and_then(|error| error.diagnostic_json());
                    AiCallFailure {
                        message: err.to_string(),
                        diagnostic,
                    }
                });
            },
            _ = tokio::time::sleep(StdDuration::from_millis(100)) => {
                if let Err(message) = ensure_sync_not_cancelled(state) {
                    return Err(AiCallFailure {
                        message,
                        diagnostic: None,
                    });
                }
            }
        }
    }
}

fn start_ai_analysis_run(
    state: &State<'_, AppState>,
    day: &str,
    profile_id: &str,
    request_kind: &str,
    input_message_ids: Vec<String>,
    input_message_count: usize,
    ai_config: &crate::storage::models::AiConfig,
    batch_index: Option<usize>,
    total_batches: Option<usize>,
    estimated_request_tokens: usize,
    estimated_context_tokens: usize,
    estimated_message_tokens: usize,
    overlap_message_count: usize,
) -> Option<AiRunTracker> {
    let run_id = Uuid::new_v4().to_string();
    let diagnostic = serde_json::json!({
        "requestKind": request_kind,
        "provider": ai_config.provider,
        "model": ai_config.model,
        "analysisInputTokenBudget": ai::analysis_batch_token_budget(ai_config),
        "estimatedRequestTokens": estimated_request_tokens,
        "estimatedMessageTokens": estimated_message_tokens,
        "estimatedContextTokens": estimated_context_tokens,
        "overlapMessageCount": overlap_message_count,
        "inputMessageCount": input_message_count,
        "batchIndex": batch_index,
        "totalBatches": total_batches,
    });
    let input_message_ids_json = serde_json::to_string(&input_message_ids).ok()?;
    let diagnostic_json = serde_json::to_string(&diagnostic).ok()?;
    let conn = state.db.lock().ok()?;
    conn.execute(
        "insert into ai_analysis_runs(
           id, day, profile_id, input_message_ids, status, model, diagnostic_json, created_at
         )
         values(?1, ?2, ?3, ?4, 'running', ?5, ?6, datetime('now'))",
        params![
            run_id,
            day,
            profile_id,
            input_message_ids_json,
            ai_config.model,
            diagnostic_json
        ],
    )
    .ok()?;
    Some(AiRunTracker {
        id: run_id,
        started_at: Instant::now(),
        diagnostic,
    })
}

fn finish_ai_analysis_run(
    state: &State<'_, AppState>,
    run: Option<AiRunTracker>,
    status: &str,
    error: Option<&str>,
    diagnostic: Option<serde_json::Value>,
) {
    let Some(run) = run else {
        return;
    };
    let duration_ms = run.started_at.elapsed().as_millis();
    let token_usage_json = diagnostic
        .as_ref()
        .and_then(|value| value.get("usage").cloned())
        .and_then(|value| serde_json::to_string(&value).ok());
    let diagnostic_json = serde_json::to_string(&diagnostic_for_run_status(
        status,
        run.diagnostic,
        diagnostic,
        duration_ms,
    ))
    .ok();
    let sanitized_error = error.map(crate::security::sanitize_log);
    if let Ok(conn) = state.db.lock() {
        let _ = conn.execute(
            "update ai_analysis_runs
             set status = ?1,
                 error = ?2,
                 diagnostic_json = coalesce(?3, diagnostic_json),
                 token_usage_json = coalesce(?4, token_usage_json),
                 finished_at = datetime('now')
             where id = ?5",
            params![
                status,
                sanitized_error,
                diagnostic_json,
                token_usage_json,
                run.id
            ],
        );
    }
}

fn diagnostic_for_run_status(
    status: &str,
    mut run_diagnostic: serde_json::Value,
    call_diagnostic: Option<serde_json::Value>,
    duration_ms: u128,
) -> serde_json::Value {
    if let Some(object) = run_diagnostic.as_object_mut() {
        object.insert("durationMs".to_owned(), serde_json::json!(duration_ms));
        if let Some(mut call_diagnostic) = call_diagnostic {
            if status == "done" {
                if let Some(call_object) = call_diagnostic.as_object_mut() {
                    call_object.remove("contentSnippet");
                    call_object.remove("responseBodySnippet");
                }
            }
            object.insert("call".to_owned(), call_diagnostic);
        }
    }
    if status == "done" {
        if let Some(object) = run_diagnostic.as_object_mut() {
            object.remove("contentSnippet");
            object.remove("responseBodySnippet");
        }
    }
    run_diagnostic
}

fn emit_analysis_progress(
    app: &tauri::AppHandle,
    profile: &ImProfile,
    batch_index: usize,
    total_batches: usize,
) {
    let _ = app.emit(
        "sync-progress",
        SyncProgress::new(
            profile.id.clone(),
            profile.label.clone(),
            "analysis",
            format!(
                "正在识别【{} · {}】待回复和待办事项第 {}/{} 批...",
                platform_label(&profile.platform),
                profile_remark(profile),
                batch_index,
                total_batches
            ),
            batch_index as i64,
            total_batches as i64,
        ),
    );
}

pub(crate) fn emit_sync_progress(
    app: &tauri::AppHandle,
    profile: &ImProfile,
    phase: &str,
    message: String,
    current: i64,
    total: i64,
) {
    let _ = app.emit(
        "sync-progress",
        SyncProgress::new(
            profile.id.clone(),
            profile.label.clone(),
            phase,
            message,
            current,
            total,
        ),
    );
}
