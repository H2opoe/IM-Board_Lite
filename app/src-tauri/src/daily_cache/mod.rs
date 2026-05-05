use chrono::{Datelike, Duration, Local, TimeZone};
use rusqlite::{params, Connection, OptionalExtension};

pub const DEFAULT_CACHE_CLEAR_MINUTES: i64 = 0;
const CACHE_CLEAR_MINUTES_KEY: &str = "cache_clear_minutes";

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolloverResult {
    pub rolled_over: bool,
    pub previous_day: String,
    pub current_day: String,
}

#[derive(Debug, Clone)]
pub struct DashboardDay {
    pub day: String,
    pub day_start_timestamp: i64,
    pub day_start_text: String,
    pub sync_end_timestamp: i64,
    pub sync_end_text: String,
}

pub fn detect_day_rollover(conn: &Connection) -> anyhow::Result<RolloverResult> {
    let today = current_dashboard_day(conn)?;
    let stored: String = conn.query_row(
        "select value from app_meta where key = 'current_day'",
        [],
        |row| row.get(0),
    )?;

    if stored == today {
        return Ok(RolloverResult {
            rolled_over: false,
            previous_day: stored,
            current_day: today,
        });
    }

    clear_expired_daily_cache(conn, &today)?;
    reset_daily_sync_state(conn, &today)?;
    conn.execute(
        "insert into app_meta(key, value, updated_at) values('current_day', ?1, datetime('now'))
         on conflict(key) do update set value = excluded.value, updated_at = excluded.updated_at",
        params![today],
    )?;

    Ok(RolloverResult {
        rolled_over: true,
        previous_day: stored,
        current_day: today,
    })
}

pub fn clear_expired_daily_cache(conn: &Connection, today: &str) -> anyhow::Result<()> {
    conn.execute("delete from daily_messages where day <> ?1", params![today])?;
    conn.execute("delete from daily_stats where day <> ?1", params![today])?;
    conn.execute("delete from daily_topics where day <> ?1", params![today])?;
    conn.execute(
        "delete from ai_analysis_runs where day <> ?1 and status in ('pending', 'running')",
        params![today],
    )?;
    Ok(())
}

pub fn reset_daily_sync_state(conn: &Connection, today: &str) -> anyhow::Result<()> {
    conn.execute("delete from sync_state where day <> ?1", params![today])?;
    Ok(())
}

pub fn current_dashboard_day(conn: &Connection) -> anyhow::Result<String> {
    Ok(current_dashboard_day_info(conn)?.day)
}

pub fn current_dashboard_day_info(conn: &Connection) -> anyhow::Result<DashboardDay> {
    let minutes = cache_clear_minutes(conn)?;
    let now = Local::now();
    let day_date = if now < today_clear_at(minutes)? {
        now.date_naive() - Duration::days(1)
    } else {
        now.date_naive()
    };
    let day_start = Local
        .with_ymd_and_hms(day_date.year(), day_date.month(), day_date.day(), 0, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("无法计算看板日期开始时间。"))?;
    let day = day_date.format("%Y-%m-%d").to_string();

    Ok(DashboardDay {
        day: day.clone(),
        day_start_timestamp: day_start.timestamp(),
        day_start_text: format!("{day} 00:00:00"),
        sync_end_timestamp: now.timestamp(),
        sync_end_text: now.format("%Y-%m-%d %H:%M:%S").to_string(),
    })
}

pub fn cache_clear_minutes(conn: &Connection) -> anyhow::Result<i64> {
    let stored: Option<String> = conn
        .query_row(
            "select value from app_meta where key = ?1",
            params![CACHE_CLEAR_MINUTES_KEY],
            |row| row.get(0),
        )
        .optional()?;
    Ok(stored
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|minutes| (0..1440).contains(minutes))
        .unwrap_or(DEFAULT_CACHE_CLEAR_MINUTES))
}

pub fn set_cache_clear_minutes(conn: &Connection, minutes: i64) -> anyhow::Result<()> {
    let minutes = minutes.clamp(0, 1439);
    conn.execute(
        "insert into app_meta(key, value, updated_at) values(?1, ?2, datetime('now'))
         on conflict(key) do update set value = excluded.value, updated_at = excluded.updated_at",
        params![CACHE_CLEAR_MINUTES_KEY, minutes.to_string()],
    )?;
    Ok(())
}

fn today_clear_at(minutes: i64) -> anyhow::Result<chrono::DateTime<Local>> {
    let now = Local::now();
    let midnight = Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("无法计算缓存清理时间。"))?;
    Ok(midnight + Duration::minutes(minutes))
}
