use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tauri::State;
use zip::write::SimpleFileOptions;

use crate::daily_cache;
use crate::security::sanitize_log;
use crate::storage::AppState;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    cache_clear_time: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticExport {
    file_path: String,
}

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    let minutes = daily_cache::cache_clear_minutes(&conn).map_err(|err| err.to_string())?;
    Ok(AppSettings {
        cache_clear_time: minutes_to_time(minutes),
    })
}

#[tauri::command]
pub fn save_app_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let minutes = parse_time_to_minutes(&settings.cache_clear_time)?;
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    daily_cache::set_cache_clear_minutes(&conn, minutes).map_err(|err| err.to_string())?;
    Ok(AppSettings {
        cache_clear_time: minutes_to_time(minutes),
    })
}

#[tauri::command]
pub fn export_diagnostic_package(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<DiagnosticExport, String> {
    export_diagnostic_package_impl(&state, file_path).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_theme_dock_icon(_app: tauri::AppHandle, theme: String) -> Result<(), String> {
    match theme.as_str() {
        "light" | "dark" => Ok(()),
        _ => Err("主题参数无效。".to_string()),
    }
}

fn export_diagnostic_package_impl(
    state: &State<'_, AppState>,
    file_path: String,
) -> anyhow::Result<DiagnosticExport> {
    let exported_at = chrono::Local::now();
    let file_path = normalize_diagnostic_export_path(file_path)?;
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let conn = state
        .db
        .lock()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let diagnostic = build_diagnostic_json(&conn, state, exported_at.to_rfc3339())?;

    let file = fs::File::create(&file_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    add_zip_text(
        &mut zip,
        options,
        "README.txt",
        "IM-Board诊断包\n\n此诊断包用于定位同步、AI分析、账号绑定和运行时问题。\n包内默认不包含聊天内容、API Key等任何敏感信息。\n如果问题涉及特定聊天，请另行提供对应截图或手动脱敏后的上下文。\n",
    )?;
    add_zip_text(
        &mut zip,
        options,
        "diagnostic.json",
        &serde_json::to_string_pretty(&diagnostic)?,
    )?;
    add_recent_crash_reports(&mut zip, options)?;
    zip.finish()?;

    Ok(DiagnosticExport {
        file_path: file_path.display().to_string(),
    })
}

fn normalize_diagnostic_export_path(file_path: String) -> anyhow::Result<PathBuf> {
    let trimmed = file_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("诊断包保存路径无效。");
    }

    let path = PathBuf::from(trimmed);
    if path.exists() && path.is_dir() {
        anyhow::bail!("请选择具体的诊断包文件名，不能直接保存到文件夹。");
    }
    Ok(path)
}

fn add_zip_text<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: SimpleFileOptions,
    name: &str,
    content: &str,
) -> anyhow::Result<()> {
    zip.start_file(name, options)?;
    zip.write_all(content.as_bytes())?;
    Ok(())
}

fn build_diagnostic_json(
    conn: &Connection,
    state: &AppState,
    exported_at: String,
) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "exportedAt": exported_at,
        "app": collect_app_info(state),
        "meta": collect_app_meta(conn)?,
        "profiles": collect_profiles(conn)?,
        "aiConfig": collect_ai_config(conn)?,
        "syncState": collect_sync_state(conn)?,
        "dailySummary": collect_daily_summary(conn)?,
        "recentAiRuns": collect_recent_ai_runs(conn)?,
    }))
}

fn collect_app_info(state: &AppState) -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "targetOs": std::env::consts::OS,
        "targetArch": std::env::consts::ARCH,
        "appDataDir": state.app_dir.display().to_string(),
        "cacheDir": state.cache_dir.display().to_string(),
    })
}

fn collect_app_meta(conn: &Connection) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare("select key, value, updated_at from app_meta order by key")?;
    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "key": row.get::<_, String>(0)?,
            "value": row.get::<_, String>(1)?,
            "updatedAt": row.get::<_, String>(2)?,
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn collect_profiles(conn: &Connection) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "select id, platform, label, enabled, status, sort_order, config_json, created_at, updated_at
         from profiles
         order by sort_order, updated_at desc",
    )?;
    let rows = stmt.query_map([], |row| {
        let config_json: String = row.get(6)?;
        let config = serde_json::from_str::<serde_json::Value>(&config_json)
            .unwrap_or_else(|_| serde_json::json!({ "parseError": "config_json 不是有效 JSON" }));
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "platform": row.get::<_, String>(1)?,
            "label": row.get::<_, String>(2)?,
            "enabled": row.get::<_, i64>(3)? == 1,
            "status": row.get::<_, String>(4)?,
            "sortOrder": row.get::<_, i64>(5)?,
            "config": redact_json_value(config),
            "createdAt": row.get::<_, String>(7)?,
            "updatedAt": row.get::<_, String>(8)?,
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn collect_ai_config(conn: &Connection) -> anyhow::Result<Option<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "select provider, base_url, model, user_prompt, analysis_prompt, summary_prompt,
                analysis_prompt_custom, summary_prompt_custom, analysis_batch_size, enabled,
                test_status, updated_at
         from ai_config
         where id = 1",
    )?;
    let mut rows = stmt.query([])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(serde_json::json!({
        "provider": row.get::<_, String>(0)?,
        "baseUrl": row.get::<_, String>(1)?,
        "model": row.get::<_, String>(2)?,
        "userPromptLength": row.get::<_, String>(3)?.chars().count(),
        "analysisPromptLength": row.get::<_, String>(4)?.chars().count(),
        "summaryPromptLength": row.get::<_, String>(5)?.chars().count(),
        "analysisPromptCustom": row.get::<_, Option<i64>>(6)?.unwrap_or(0) == 1,
        "summaryPromptCustom": row.get::<_, Option<i64>>(7)?.unwrap_or(0) == 1,
        "analysisBatchSize": row.get::<_, i64>(8)?,
        "enabled": row.get::<_, i64>(9)? == 1,
        "testStatus": row.get::<_, String>(10)?,
        "updatedAt": row.get::<_, String>(11)?,
    })))
}

fn collect_sync_state(conn: &Connection) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "select profile_id, day, last_sync_at, last_analysis_at, cursor_json, updated_at
         from sync_state
         order by updated_at desc
         limit 40",
    )?;
    let rows = stmt.query_map([], |row| {
        let cursor_json: String = row.get(4)?;
        let cursor = serde_json::from_str::<serde_json::Value>(&cursor_json)
            .unwrap_or_else(|_| serde_json::json!({ "parseError": "cursor_json 不是有效 JSON" }));
        Ok(serde_json::json!({
            "profileId": row.get::<_, String>(0)?,
            "day": row.get::<_, String>(1)?,
            "lastSyncAt": row.get::<_, Option<String>>(2)?,
            "lastAnalysisAt": row.get::<_, Option<String>>(3)?,
            "cursor": redact_json_value(cursor),
            "updatedAt": row.get::<_, String>(5)?,
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn collect_recent_ai_runs(conn: &Connection) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "select id, day, profile_id, status, model, token_usage_json, error, created_at, finished_at,
                input_message_ids, diagnostic_json
         from ai_analysis_runs
         order by created_at desc
         limit 40",
    )?;
    let rows = stmt.query_map([], |row| {
        let token_usage_json: Option<String> = row.get(5)?;
        let token_usage = token_usage_json
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
        let error = row
            .get::<_, Option<String>>(6)?
            .map(|value| sanitize_log(&value))
            .map(|value| truncate_for_diagnostic(&value, 1200));
        let input_message_ids = row
            .get::<_, String>(9)
            .ok()
            .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
            .unwrap_or_default();
        let diagnostic = row
            .get::<_, Option<String>>(10)?
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .map(redact_json_value);
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "day": row.get::<_, String>(1)?,
            "profileId": row.get::<_, String>(2)?,
            "status": row.get::<_, String>(3)?,
            "model": row.get::<_, Option<String>>(4)?,
            "tokenUsage": token_usage,
            "error": error,
            "inputMessageCount": input_message_ids.len(),
            "diagnostic": diagnostic,
            "createdAt": row.get::<_, String>(7)?,
            "finishedAt": row.get::<_, Option<String>>(8)?,
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn collect_daily_summary(conn: &Connection) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "select
           messages.day,
           messages.profile_id,
           messages.platform,
           count(*) as message_count,
           sum(case when messages.analyzed_at is null then 1 else 0 end) as pending_analysis_count,
           count(distinct messages.chat_id) as chat_count,
           coalesce(topics.topic_count, 0) as topic_count,
           coalesce(actions.action_count, 0) as action_count
         from daily_messages messages
         left join (
           select day, profile_id, count(*) as topic_count
           from daily_topics
           group by day, profile_id
         ) topics on topics.day = messages.day and topics.profile_id = messages.profile_id
         left join (
           select date(first_detected_at) as day, profile_id, count(*) as action_count
           from action_items
           group by date(first_detected_at), profile_id
         ) actions on actions.day = messages.day and actions.profile_id = messages.profile_id
         group by messages.day, messages.profile_id, messages.platform
         order by messages.day desc, messages.profile_id
         limit 80",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "day": row.get::<_, String>(0)?,
            "profileId": row.get::<_, String>(1)?,
            "platform": row.get::<_, String>(2)?,
            "messageCount": row.get::<_, i64>(3)?,
            "pendingAnalysisCount": row.get::<_, i64>(4)?,
            "chatCount": row.get::<_, i64>(5)?,
            "topicCount": row.get::<_, i64>(6)?,
            "actionCount": row.get::<_, i64>(7)?,
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn add_recent_crash_reports<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: SimpleFileOptions,
) -> anyhow::Result<()> {
    let Some(home_dir) = dirs::home_dir() else {
        return Ok(());
    };
    let report_dir = home_dir
        .join("Library")
        .join("Logs")
        .join("DiagnosticReports");
    if !report_dir.exists() {
        return Ok(());
    }

    let mut reports = fs::read_dir(report_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_im_board_crash_report(path))
        .filter_map(|path| {
            let modified = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| right.0.cmp(&left.0));

    for (_, path) in reports.into_iter().take(3) {
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let content = fs::read(&path)?;
        zip.start_file(format!("crash-reports/{file_name}"), options)?;
        let mut reader = Cursor::new(content);
        std::io::copy(&mut reader, zip)?;
    }
    Ok(())
}

fn is_im_board_crash_report(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let normalized = file_name.to_ascii_lowercase();
    normalized.starts_with("im-board")
        && (normalized.ends_with(".ips") || normalized.ends_with(".crash"))
}

fn redact_json_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(redact_json_value).collect())
        }
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    if is_sensitive_key(&key) {
                        (key, serde_json::json!("***"))
                    } else {
                        (key, redact_json_value(value))
                    }
                })
                .collect(),
        ),
        serde_json::Value::String(text) => {
            serde_json::Value::String(truncate_for_diagnostic(&sanitize_log(&text), 1200))
        }
        other => other,
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    // 诊断包可以定位路径、平台和状态，但不能外带授权令牌或密钥。
    [
        "key",
        "token",
        "secret",
        "authorization",
        "password",
        "cookie",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn truncate_for_diagnostic(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_owned();
    }
    let mut output = value.chars().take(limit).collect::<String>();
    output.push_str("...");
    output
}

fn parse_time_to_minutes(value: &str) -> Result<i64, String> {
    let (hour, minute) = value
        .split_once(':')
        .ok_or_else(|| "缓存清理时间格式无效。".to_string())?;
    let hour = hour
        .parse::<i64>()
        .map_err(|_| "缓存清理时间格式无效。".to_string())?;
    let minute = minute
        .parse::<i64>()
        .map_err(|_| "缓存清理时间格式无效。".to_string())?;
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        return Err("缓存清理时间超出范围。".to_string());
    }
    Ok(hour * 60 + minute)
}

fn minutes_to_time(minutes: i64) -> String {
    let minutes = minutes.clamp(0, 1439);
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}
