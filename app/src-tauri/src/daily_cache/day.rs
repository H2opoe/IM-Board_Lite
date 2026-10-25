use crate::messages::import_window::local_day_boundary;
use chrono::{DateTime, Local, NaiveDate, TimeZone};
use rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct DashboardDay {
    pub day: String,
    pub day_start_timestamp: i64,
    pub day_start_text: String,
    pub sync_end_timestamp: i64,
    pub sync_end_text: String,
}

pub(super) fn current_dashboard_day_info(conn: &Connection) -> anyhow::Result<DashboardDay> {
    dashboard_day_at(Local::now(), super::cache_clear_minutes(conn)?)
}

pub(super) fn dashboard_day_at(
    now: chrono::DateTime<Local>,
    minutes: i64,
) -> anyhow::Result<DashboardDay> {
    let today_start = local_day_boundary(now.date_naive())
        .ok_or_else(|| anyhow::anyhow!("无法计算看板日期开始时间。"))?;
    let day_date = if now < clear_at(now.date_naive(), minutes)? {
        // Use the prior actual instant, since a civil date can be skipped.
        Local
            .timestamp_opt(today_start.timestamp() - 1, 0)
            .single()
            .ok_or_else(|| anyhow::anyhow!("无法计算上一看板日期。"))?
            .date_naive()
    } else {
        now.date_naive()
    };
    let day_start = local_day_boundary(day_date)
        .ok_or_else(|| anyhow::anyhow!("无法计算看板日期开始时间。"))?;
    let day = day_date.format("%Y-%m-%d").to_string();

    Ok(DashboardDay {
        day: day.clone(),
        day_start_timestamp: day_start.timestamp(),
        day_start_text: day_start.format("%Y-%m-%d %H:%M:%S").to_string(),
        sync_end_timestamp: now.timestamp(),
        sync_end_text: now.format("%Y-%m-%d %H:%M:%S").to_string(),
    })
}

// First occurrence of the configured wall-clock time. If that time is
// skipped, roll over at the next valid instant. A repeated clock never rolls
// the dashboard backwards after the first occurrence has passed.
fn clear_at(date: NaiveDate, minutes: i64) -> anyhow::Result<DateTime<Local>> {
    anyhow::ensure!((0..1440).contains(&minutes), "无效的缓存清理时间。");
    let target = date
        .and_hms_opt((minutes / 60) as u32, (minutes % 60) as u32, 0)
        .ok_or_else(|| anyhow::anyhow!("无法计算缓存清理时间。"))?;
    let start =
        local_day_boundary(date).ok_or_else(|| anyhow::anyhow!("无法计算缓存清理时间。"))?;
    let next = date
        .succ_opt()
        .and_then(local_day_boundary)
        .ok_or_else(|| anyhow::anyhow!("无法计算下一日期边界。"))?;
    let mut probe = start.timestamp();
    let mut previous = probe;
    loop {
        let instant = Local
            .timestamp_opt(probe, 0)
            .single()
            .ok_or_else(|| anyhow::anyhow!("无法读取本地时间。"))?;
        if instant.naive_local() >= target {
            // Refine only the final minute, including historical sub-minute
            // offsets. UTC iteration avoids ambiguous local-time conversion.
            for second in previous..=probe {
                let candidate = Local
                    .timestamp_opt(second, 0)
                    .single()
                    .ok_or_else(|| anyhow::anyhow!("无法读取本地时间。"))?;
                if candidate.naive_local() >= target {
                    return Ok(candidate);
                }
            }
        }
        anyhow::ensure!(probe < next.timestamp(), "无法计算缓存清理时间。");
        previous = probe + 1;
        probe = (probe + 60).min(next.timestamp());
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn rollover_uses_local_clock_time_in_isolated_timezones() {
        const MARKER: &str = "IM_BOARD_DASHBOARD_DAY_ZONE_FIXTURE";
        if let Ok(zone) = std::env::var(MARKER) {
            let cases: &[(&str, i64, &str)] = match zone.as_str() {
                "America/New_York" => &[
                    ("2026-03-08T07:30:00Z", 180, "2026-03-08"),
                    ("2026-11-01T07:30:00Z", 180, "2026-10-31"),
                    ("2026-03-08T07:00:00Z", 150, "2026-03-08"),
                    ("2026-11-01T05:35:00Z", 90, "2026-11-01"),
                    ("2026-11-01T06:15:00Z", 90, "2026-11-01"),
                ],
                "America/Havana" => &[("2026-03-08T05:30:00Z", 0, "2026-03-08")],
                "Pacific/Apia" => &[("2011-12-30T10:30:00Z", 60, "2011-12-29")],
                "Asia/Shanghai" => &[
                    ("2026-09-30T16:59:00Z", 60, "2026-09-30"),
                    ("2026-09-30T17:00:00Z", 60, "2026-10-01"),
                    ("2026-09-30T15:58:00Z", 1439, "2026-09-29"),
                ],
                _ => panic!("unknown fixture timezone"),
            };
            for (utc, minutes, expected) in cases {
                let now = chrono::DateTime::parse_from_rfc3339(utc)
                    .unwrap()
                    .with_timezone(&Local);
                let day = dashboard_day_at(now, *minutes).unwrap();
                assert_eq!(&day.day, expected, "{zone} {utc} {minutes}");
                assert_eq!(day.sync_end_timestamp, now.timestamp());
                assert!(day.day_start_timestamp <= day.sync_end_timestamp);
                let actual_start = Local
                    .timestamp_opt(day.day_start_timestamp, 0)
                    .single()
                    .unwrap();
                assert_eq!(
                    day.day_start_text,
                    actual_start.format("%Y-%m-%d %H:%M:%S").to_string()
                );
            }
            return;
        }
        for zone in [
            "America/New_York",
            "America/Havana",
            "Pacific/Apia",
            "Asia/Shanghai",
        ] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "daily_cache::day::tests::rollover_uses_local_clock_time_in_isolated_timezones",
                    "--nocapture",
                ])
                .env("TZ", zone)
                .env(MARKER, zone)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{zone}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
