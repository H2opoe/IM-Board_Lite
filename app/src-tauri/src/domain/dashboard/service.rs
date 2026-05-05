use chrono::Local;
use rusqlite::params;

use crate::daily_cache;
use crate::storage::models::DashboardData;
use crate::storage::AppState;

use super::projection;

pub fn get_dashboard(
    state: &AppState,
    profile_id: Option<String>,
) -> anyhow::Result<DashboardData> {
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let day = daily_cache::current_dashboard_day(&conn)?;
    let profile_filter = profile_id.unwrap_or_else(|| "aggregate".to_owned());

    projection::build_dashboard(state, &conn, &day, &profile_filter)
}

pub fn mark_action_item(state: &AppState, action_id: &str, status: &str) -> anyhow::Result<()> {
    let completed_at = if status == "done" {
        Some(Local::now().to_rfc3339())
    } else {
        None
    };
    let conn = state
        .db
        .lock()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    conn.execute(
        "update action_items
         set status = ?1, completed_at = ?2, last_updated_at = datetime('now')
         where id = ?3",
        params![status, completed_at, action_id],
    )?;
    Ok(())
}
