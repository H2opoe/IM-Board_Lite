use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone};

#[derive(Debug, Clone)]
pub(crate) struct MessageImportWindow {
    pub(crate) day: String,
    pub(crate) start_timestamp: i64,
    pub(crate) end_timestamp: i64,
}

impl MessageImportWindow {
    pub(crate) fn natural_day(day: &str) -> Option<Self> {
        let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
        let start = local_day_boundary(date)?;
        // A whole civil date may be skipped by a timezone change.
        if start.date_naive() != date {
            return None;
        }
        let end = local_day_boundary(date.succ_opt()?)? - Duration::seconds(1);
        Some(Self {
            day: day.to_owned(),
            start_timestamp: start.timestamp(),
            end_timestamp: end.timestamp(),
        })
    }

    pub(crate) fn contains(&self, timestamp: i64) -> bool {
        timestamp >= self.start_timestamp && timestamp <= self.end_timestamp
    }
}

// Search absolute instants for the local calendar boundary. Converting a UTC
// instant to local time is unambiguous, including repeated or missing midnight.
// Chrono's fixed offsets are strictly less than 24 hours in either direction.
pub(crate) fn local_day_boundary(date: NaiveDate) -> Option<DateTime<Local>> {
    let anchor = date.and_hms_opt(0, 0, 0)?.and_utc().timestamp();
    let mut low = anchor.checked_sub(24 * 60 * 60)?;
    let mut high = anchor.checked_add(24 * 60 * 60)?;
    while low < high {
        let mid = low + (high - low) / 2;
        if Local.timestamp_opt(mid, 0).single()?.date_naive() < date {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    Local.timestamp_opt(low, 0).single()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn natural_days_follow_calendar_in_isolated_timezones() {
        const MARKER: &str = "IM_BOARD_IMPORT_WINDOW_ZONE_FIXTURE";
        if let Ok(zone) = std::env::var(MARKER) {
            let cases: &[(&str, i64)] = match zone.as_str() {
                "America/New_York" => &[("2026-03-08", 23), ("2026-11-01", 25)],
                "Asia/Shanghai" => &[("2026-03-08", 24), ("2026-11-01", 24), ("2024-02-29", 24)],
                "America/Havana" => &[("2026-03-07", 24), ("2026-03-08", 23), ("2026-11-01", 25)],
                "Pacific/Apia" => &[],
                _ => panic!("unknown fixture timezone"),
            };
            for (day, hours) in cases {
                let window = MessageImportWindow::natural_day(day).unwrap();
                assert_eq!(
                    window.end_timestamp - window.start_timestamp + 1,
                    hours * 3600,
                    "{zone} {day}"
                );
                let next = NaiveDate::parse_from_str(day, "%Y-%m-%d")
                    .unwrap()
                    .succ_opt()
                    .unwrap()
                    .to_string();
                let next_start = MessageImportWindow::natural_day(&next)
                    .unwrap()
                    .start_timestamp;
                assert_eq!(window.end_timestamp + 1, next_start);
                assert!(window.contains(window.start_timestamp));
                assert!(window.contains(window.end_timestamp));
                assert!(!window.contains(window.start_timestamp - 1));
                assert!(!window.contains(next_start));
            }
            if zone == "Pacific/Apia" {
                let before = MessageImportWindow::natural_day("2011-12-29").unwrap();
                assert!(MessageImportWindow::natural_day("2011-12-30").is_none());
                let after = MessageImportWindow::natural_day("2011-12-31").unwrap();
                assert_eq!(before.end_timestamp + 1, after.start_timestamp);
                assert_eq!(before.end_timestamp - before.start_timestamp + 1, 24 * 3600);
            }
            return;
        }
        // TZ is set before the child test process starts; never mutate the
        // timezone of this parallel test runner or the user's operating system.
        for zone in [
            "America/New_York",
            "Asia/Shanghai",
            "America/Havana",
            "Pacific/Apia",
        ] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "messages::import_window::tests::natural_days_follow_calendar_in_isolated_timezones", "--nocapture"])
                .env("TZ", zone).env(MARKER, zone).output().unwrap();
            assert!(
                output.status.success(),
                "{zone}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn invalid_calendar_dates_are_rejected() {
        for day in ["", "not-a-date", "2026-02-30"] {
            assert!(MessageImportWindow::natural_day(day).is_none());
        }
    }
}
