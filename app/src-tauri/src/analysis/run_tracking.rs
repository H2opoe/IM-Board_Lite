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
