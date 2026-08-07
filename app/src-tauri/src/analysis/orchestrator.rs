use std::collections::HashMap;
use std::future::Future;
use std::time::{Duration as StdDuration, Instant};

use chrono::Duration;
use rusqlite::params;
use tauri::State;
use uuid::Uuid;

use crate::ai;
use crate::analysis::progress::{
    emit_analysis_done_for_scope, emit_analysis_progress_for_scope, emit_summary_progress_for_scope,
};
pub(crate) use crate::analysis::progress::{
    emit_sync_progress, ensure_sync_not_cancelled, is_sync_cancelled_message,
};
use crate::bridge_runner::BridgeRequest;
use crate::commands::ai as ai_commands;
use crate::messages::normalizer::{normalize_message, platform_label, profile_remark, value_array};
use crate::storage::models::ImProfile;
use crate::storage::AppState;
use crate::sync::fetch::{refresh_local_keyword_stats, MessageImportWindow};
use crate::sync::orchestrator::run_sync_bridge;

struct AiCallFailure {
    message: String,
    diagnostic: Option<serde_json::Value>,
}

struct AiRunTracker {
    id: String,
    profile_id: String,
    request_kind: String,
    started_at: Instant,
    diagnostic: serde_json::Value,
}

const CONTEXT_BACKFILL_DAYS: i64 = 7;
const CONTEXT_BACKFILL_LIMIT_PER_DAY: usize = 80;
const CONTEXT_EVIDENCE_LIMIT: usize = 12;

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
        let mut config = ai::get_config(&conn).map_err(|err| err.to_string())?;
        ai::apply_hardware_batch_limit(
            &mut config,
            state.system_capabilities.recommended_analysis_batch_size,
        );
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
        let analysis_result =
            await_ai_call_with_runtime_recovery(app, state, &ai_config, |config| {
                let config = config.clone();
                let day = day.to_owned();
                let profile_id = analysis_profile_id.clone();
                let messages = messages.clone();
                let context = context.clone();
                Box::pin(async move {
                    ai::request_profile_analysis(&config, &day, &profile_id, &messages, &context)
                        .await
                })
            })
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
                            match await_ai_call_with_runtime_recovery(
                                app,
                                state,
                                &ai_config,
                                |config| {
                                    let config = config.clone();
                                    let day = day.to_owned();
                                    let profile_id = analysis_profile_id.clone();
                                    let messages = messages.clone();
                                    let context = context.clone();
                                    Box::pin(async move {
                                        ai::request_profile_analysis(
                                            &config,
                                            &day,
                                            &profile_id,
                                            &messages,
                                            &context,
                                        )
                                        .await
                                    })
                                },
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
                                        "{}已补充{}条历史上下文并重新分析。",
                                        analysis_scope_label, history_count
                                    ));
                                }
                                Err(err) => {
                                    let user_message = crate::diagnostics::classify_ai_user_message(
                                        &err.message,
                                        err.diagnostic.as_ref(),
                                    );
                                    finish_ai_analysis_run(
                                        state,
                                        refine_run_id,
                                        "failed",
                                        Some(&err.message),
                                        err.diagnostic,
                                    );
                                    warnings.push(format!(
                                        "{}历史上下文重新分析失败，已保留首次AI结果：{}",
                                        analysis_scope_label, user_message
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
                                    "{}已为{}个事项补读历史证据。",
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
                            "{}待回复和待办事项识别结果保存失败（第{}/{}批）：{}",
                            analysis_scope_label,
                            batch_index + 1,
                            total_batches,
                            err
                        ));
                    }
                }
            }
            Err(err) => {
                let user_message = crate::diagnostics::classify_ai_user_message(
                    &err.message,
                    err.diagnostic.as_ref(),
                );
                finish_ai_analysis_run(state, run_id, "failed", Some(&err.message), err.diagnostic);
                if let Ok(conn) = state.db.lock() {
                    let _ = ai::keep_analysis_pending_for_messages(&conn, day, &primary_messages);
                }
                ai_status = "failed".to_owned();
                warnings.push(format!(
                    "{}待回复和待办事项识别失败（第{}/{}批）：{}",
                    analysis_scope_label,
                    batch_index + 1,
                    total_batches,
                    user_message
                ));
            }
        }
    }

    run_summary_stage(
        app,
        state,
        day,
        target_profiles,
        &ai_config,
        ai_configured,
        analysis_message_count,
        &analysis_scope_label,
        warnings,
    )
    .await?;

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

include!("context_backfill.rs");

include!("run_tracking.rs");

include!("summary_stage.rs");
