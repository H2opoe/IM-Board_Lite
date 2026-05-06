use crate::ai;
use crate::commands::ai as ai_commands;
use crate::storage::models::DashboardData;
use crate::storage::AppState;

use super::actions::list_actions;
use super::activity::{hourly_activity, message_types};
use super::metrics::load_metrics;
use super::rankings::{chat_rank, speaker_top};
use super::stats::{dashboard_stats, enrich_keywords, enrich_topics};

pub fn build_dashboard(
    state: &AppState,
    conn: &rusqlite::Connection,
    day: &str,
    profile_filter: &str,
) -> anyhow::Result<DashboardData> {
    let replies = list_actions(conn, "reply", profile_filter)?;
    let tasks = list_actions(conn, "task", profile_filter)?;
    let open_replies = replies.iter().filter(|item| item.status == "open").count() as i64;
    let open_tasks = tasks.iter().filter(|item| item.status == "open").count() as i64;
    let ai_configured = ai::get_config(conn)
        .map(|config| ai_commands::is_configured_for_current_runtime(&config, state))
        .unwrap_or(false);
    let keywords = enrich_keywords(
        conn,
        day,
        profile_filter,
        dashboard_stats(conn, day, profile_filter, "keywords").unwrap_or_default(),
    )
    .unwrap_or_default();
    let keyword_status = keyword_field(&keywords, "status").unwrap_or_else(|| {
        if ai_configured {
            "local_pending_ai".to_owned()
        } else {
            "local_final".to_owned()
        }
    });
    let keyword_source = match keyword_field(&keywords, "source").as_deref() {
        Some("ai_refined") => "ai".to_owned(),
        _ => "local".to_owned(),
    };
    let keyword_version = keyword_field(&keywords, "version").unwrap_or_default();
    let keyword_updated_at = keyword_field(&keywords, "updatedAt").unwrap_or_default();

    Ok(DashboardData {
        day: day.to_owned(),
        metrics: load_metrics(conn, day, profile_filter, open_replies, open_tasks)?,
        replies,
        tasks,
        topics: enrich_topics(
            conn,
            day,
            profile_filter,
            dashboard_stats(conn, day, profile_filter, "topics").unwrap_or_default(),
        )
        .unwrap_or_default(),
        chat_rank: chat_rank(conn, day, profile_filter).unwrap_or_default(),
        speaker_top: speaker_top(conn, day, profile_filter).unwrap_or_default(),
        hourly_activity: hourly_activity(conn, day, profile_filter).unwrap_or_default(),
        message_types: message_types(conn, day, profile_filter).unwrap_or_default(),
        keywords,
        keyword_status,
        keyword_source,
        keyword_version,
        keyword_updated_at,
        ai_status: if ai_configured {
            "ready".to_owned()
        } else {
            "not_configured".to_owned()
        },
        sync_status: "idle".to_owned(),
    })
}

fn keyword_field(keywords: &[serde_json::Value], field: &str) -> Option<String> {
    keywords
        .first()
        .and_then(|keyword| keyword.get(field))
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}
