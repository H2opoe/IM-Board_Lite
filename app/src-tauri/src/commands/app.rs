use std::fs;
use std::io::Write;
use std::path::PathBuf;

use rusqlite::Connection;
use tauri::{Manager, State};
use zip::write::SimpleFileOptions;

use crate::daily_cache;
use crate::security::diagnostic_metadata;
use crate::storage::AppState;
use crate::version::app_version;

const MAIN_WINDOW_LABEL: &str = "main";
const MAIN_TRAY_ID: &str = "main-tray";

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
pub fn set_theme_dock_icon(app: tauri::AppHandle, theme: String) -> Result<(), String> {
    match theme.as_str() {
        "light" | "dark" => set_theme_dock_icon_impl(app, &theme),
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
    write_diagnostic_archive(file, &diagnostic)?;

    Ok(DiagnosticExport {
        file_path: file_path.display().to_string(),
    })
}

fn write_diagnostic_archive<W: Write + std::io::Seek>(
    writer: W,
    diagnostic: &serde_json::Value,
) -> anyhow::Result<()> {
    let mut zip = zip::ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    add_zip_text(
        &mut zip,
        options,
        "README.txt",
        "IM-Board 诊断包\n仅包含结构化诊断元数据；不包含响应正文、原始日志或系统崩溃报告。\n",
    )?;
    add_zip_text(
        &mut zip,
        options,
        "diagnostic.json",
        &serde_json::to_string_pretty(diagnostic)?,
    )?;
    zip.finish()?;
    Ok(())
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
    let action_days = crate::daily_cache::action_day::resolve_action_days(conn)?;
    Ok(serde_json::json!({
        "exportedAt": exported_at,
        "app": collect_app_info(state),
        "meta": collect_app_meta(conn)?,
        "profiles": collect_profiles(conn)?,
        "aiConfig": collect_ai_config(conn)?,
        "syncState": collect_sync_state(conn)?,
        "dailySummary": collect_daily_summary(conn, &action_days)?,
        "actionDaySummary": crate::daily_cache::action_day::diagnostic_day_summary(&action_days),
        "recentAiRuns": collect_recent_ai_runs(conn)?,
        "recentErrorEvents": collect_recent_error_events(conn)?,
    }))
}

fn collect_app_info(state: &AppState) -> serde_json::Value {
    serde_json::json!({
        "version": app_version(),
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
    let mut stmt = conn.prepare("select id, platform, label, enabled, status, sort_order, config_json, created_at, updated_at from profiles order by sort_order, updated_at desc")?;
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
            "config": diagnostic_metadata(config),
            "createdAt": row.get::<_, String>(7)?,
            "updatedAt": row.get::<_, String>(8)?,
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn collect_ai_config(conn: &Connection) -> anyhow::Result<Option<serde_json::Value>> {
    let mut stmt = conn.prepare("select provider, base_url, model, user_prompt, analysis_prompt, summary_prompt, analysis_prompt_custom, summary_prompt_custom, analysis_batch_size, enabled, test_status, updated_at from ai_config where id = 1")?;
    let mut rows = stmt.query([])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(serde_json::json!({
        "provider": row.get::<_, String>(0)?,
        "baseUrlConfigured": !row.get::<_, String>(1)?.is_empty(),
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
    let mut stmt = conn.prepare("select profile_id, day, last_sync_at, last_analysis_at, cursor_json, updated_at, last_attempt_at, last_success_at, last_error from sync_state order by updated_at desc limit 40")?;
    let rows = stmt.query_map([], |row| {
        let cursor_json: String = row.get(4)?;
        let cursor = serde_json::from_str::<serde_json::Value>(&cursor_json)
            .unwrap_or_else(|_| serde_json::json!({ "parseError": "cursor_json 不是有效 JSON" }));
        Ok(serde_json::json!({
            "profileId": row.get::<_, String>(0)?,
            "day": row.get::<_, String>(1)?,
            "lastSyncAt": row.get::<_, Option<String>>(2)?,
            "lastAnalysisAt": row.get::<_, Option<String>>(3)?,
            "cursor": diagnostic_metadata(cursor),
            "updatedAt": row.get::<_, String>(5)?,
            "lastAttemptAt": row.get::<_, Option<String>>(6)?,
            "lastSuccessAt": row.get::<_, Option<String>>(7)?,
            "lastError": row.get::<_, Option<String>>(8)?.map(|_| "错误正文已省略。".to_owned()),
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn collect_recent_ai_runs(conn: &Connection) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare("select id, day, profile_id, status, model, token_usage_json, error, created_at, finished_at, input_message_ids, diagnostic_json from ai_analysis_runs order by created_at desc limit 40")?;
    let rows = stmt.query_map([], |row| {
        let token_usage_json: Option<String> = row.get(5)?;
        let token_usage = token_usage_json
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
        let error = row
            .get::<_, Option<String>>(6)?
            .map(|_| "错误正文已省略；请参考结构化诊断。".to_owned());
        let input_message_ids = row
            .get::<_, String>(9)
            .ok()
            .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
            .unwrap_or_default();
        let diagnostic = row
            .get::<_, Option<String>>(10)?
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .map(diagnostic_metadata);
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "day": row.get::<_, String>(1)?,
            "profileId": row.get::<_, String>(2)?,
            "status": row.get::<_, String>(3)?,
            "model": row.get::<_, Option<String>>(4)?,
            "tokenUsage": token_usage.map(diagnostic_metadata),
            "error": error,
            "inputMessageCount": input_message_ids.len(),
            "diagnostic": diagnostic,
            "createdAt": row.get::<_, String>(7)?,
            "finishedAt": row.get::<_, Option<String>>(8)?,
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn collect_recent_error_events(conn: &Connection) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare("select id, source, category, severity, profile_id, platform, operation, user_message, raw_detail_json, context_json, created_at from diagnostic_error_events order by created_at desc limit 100")?;
    let rows = stmt.query_map([], |row| {
        let raw_detail = row
            .get::<_, String>(8)
            .ok()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
            .map(diagnostic_metadata)
            .unwrap_or_else(|| serde_json::json!({}));
        let context = row
            .get::<_, String>(9)
            .ok()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
            .map(diagnostic_metadata)
            .unwrap_or_else(|| serde_json::json!({}));
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "source": row.get::<_, String>(1)?,
            "category": row.get::<_, String>(2)?,
            "severity": row.get::<_, String>(3)?,
            "profileId": row.get::<_, Option<String>>(4)?,
            "platform": row.get::<_, Option<String>>(5)?,
            "operation": row.get::<_, String>(6)?,
            "userMessage": "请参考错误分类；不导出自由文本详情。",
            "rawDetail": raw_detail,
            "context": context,
            "createdAt": row.get::<_, String>(10)?,
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn collect_daily_summary(
    conn: &Connection,
    action_days: &[crate::daily_cache::action_day::ResolvedActionDay],
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut known_counts = std::collections::BTreeMap::<(&str, &str), i64>::new();
    for action in action_days {
        if let crate::daily_cache::action_day::ActionDay::Known(day) = &action.day {
            *known_counts.entry((&action.profile_id, day)).or_default() += 1;
        }
    }
    let mut stmt = conn.prepare(
        "select
           messages.day,
           messages.profile_id,
           messages.platform,
           count(*) as message_count,
           sum(case when messages.analyzed_at is null then 1 else 0 end) as pending_analysis_count,
           count(distinct messages.chat_id) as chat_count,
           coalesce(topics.topic_count, 0) as topic_count
         from daily_messages messages
         left join (
           select day, profile_id, count(*) as topic_count
           from daily_topics
           group by day, profile_id
         ) topics on topics.day = messages.day and topics.profile_id = messages.profile_id
         group by messages.day, messages.profile_id, messages.platform
         order by messages.day desc, messages.profile_id
         limit 80",
    )?;
    let rows = stmt.query_map([], |row| {
        let day = row.get::<_, String>(0)?;
        let profile_id = row.get::<_, String>(1)?;
        let action_count = known_counts
            .get(&(profile_id.as_str(), day.as_str()))
            .copied()
            .unwrap_or(0);
        Ok(serde_json::json!({
            "day": day,
            "profileId": profile_id,
            "platform": row.get::<_, String>(2)?,
            "messageCount": row.get::<_, i64>(3)?,
            "pendingAnalysisCount": row.get::<_, i64>(4)?,
            "chatCount": row.get::<_, i64>(5)?,
            "topicCount": row.get::<_, i64>(6)?,
            "actionCount": action_count,
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn set_theme_dock_icon_impl(app: tauri::AppHandle, theme: &str) -> Result<(), String> {
    let icon = tauri::image::Image::from_path(desktop_icon_path(&app, theme_icon_file_name(theme)))
        .map_err(|error| format!("读取应用图标失败：{error}"))?;
    // Windows 的 exe 与快捷方式图标来自安装包静态资源；运行中同步窗口任务栏图标与后台托盘图标。
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window
            .set_icon(icon.clone())
            .map_err(|error| format!("切换窗口图标失败：{error}"))?;
    }
    if let Some(tray) = app.tray_by_id(MAIN_TRAY_ID) {
        tray.set_icon(Some(icon))
            .map_err(|error| format!("切换后台图标失败：{error}"))?;
    }
    Ok(())
}

fn theme_icon_file_name(theme: &str) -> &'static str {
    if theme == "dark" {
        "icon-dark.png"
    } else {
        "icon-light.png"
    }
}

fn desktop_icon_path(app: &tauri::AppHandle, file_name: &str) -> std::path::PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled_icon = resource_dir.join("icons").join(file_name);
        if bundled_icon.exists() {
            return bundled_icon;
        }
    }
    #[cfg(debug_assertions)]
    {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("icons")
            .join(file_name)
    }
    #[cfg(not(debug_assertions))]
    {
        app.path()
            .resource_dir()
            .map(|root| root.join("icons").join(file_name))
            .unwrap_or_default()
    }
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

#[cfg(test)]
mod diagnostic_export_tests {
    use super::*;
    use std::io::{Cursor, Read};

    #[test]
    fn failed_legacy_ai_and_error_events_cannot_leak_into_final_zip() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .unwrap();
        conn.execute("insert into ai_analysis_runs(id,day,profile_id,status,error,diagnostic_json,token_usage_json,created_at) values('r','2026-09-29','p','failed','PRIVATE_CHAT',?1,?2,datetime('now'))", rusqlite::params![r#"{"call":{"contentSnippet":"PRIVATE_CHAT","responseBodySnippet":"SECRET_KEY","httpStatus":500},"unknown":"PRIVATE_CHAT"}"#,r#"{"total_tokens":12,"extra":"PRIVATE_CHAT"}"#]).unwrap();
        conn.execute("insert into diagnostic_error_events(id,source,category,severity,operation,user_message,raw_detail_json,context_json,created_at) values('e','ai','response_parse','error','analysis','PRIVATE_CHAT',?1,?2,datetime('now'))", rusqlite::params![r#"{"error":"PRIVATE_CHAT","diagnostic":{"contentSnippet":"SECRET_KEY","httpStatus":500}}"#,r#"{"anything":"PRIVATE_CHAT"}"#]).unwrap();
        let diagnostic = serde_json::json!({"recentAiRuns": collect_recent_ai_runs(&conn).unwrap(), "recentErrorEvents":collect_recent_error_events(&conn).unwrap()});
        let mut bytes = Vec::new();
        write_diagnostic_archive(Cursor::new(&mut bytes), &diagnostic).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(zip.len(), 2);
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).unwrap();
            let mut content = String::new();
            entry.read_to_string(&mut content).unwrap();
            assert!(!content.contains("PRIVATE_CHAT"));
            assert!(!content.contains("SECRET_KEY"));
            if entry.name().expect("valid ZIP entry name") == "diagnostic.json" {
                assert!(content.contains("500"));
            }
        }
    }
}

#[cfg(test)]
#[path = "app_action_day_tests.rs"]
mod action_day_tests;
