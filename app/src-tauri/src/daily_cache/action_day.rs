use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use rusqlite::Connection;

use super::day::dashboard_day_at;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActionDay {
    Known(String),
    LegacyCandidates(BTreeSet<String>),
    Unknown,
}

impl ActionDay {
    pub(super) fn expired_on(&self, today: &str) -> bool {
        match self {
            Self::Known(day) => day != today,
            Self::LegacyCandidates(days) => !days.is_empty() && !days.contains(today),
            Self::Unknown => false,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ResolvedActionDay {
    pub id: String,
    pub profile_id: String,
    pub day: ActionDay,
}

pub(crate) fn canonical_day(value: &str) -> Option<String> {
    if value.len() != 10 {
        return None;
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    (date.format("%Y-%m-%d").to_string() == value).then(|| value.to_owned())
}

// Source messages retain their existing max(day) policy, including carried
// actions that acquire a newer source. First detection is immutable otherwise.
pub(crate) fn resolve_action_days(conn: &Connection) -> anyhow::Result<Vec<ResolvedActionDay>> {
    let minutes = super::cache_clear_minutes(conn)?;
    let mut stmt = conn.prepare(
        "select action_items.id, profile_id, detected_day, first_detected_at,
                (select max(daily_messages.day) from daily_messages
                 where daily_messages.id in (
                   select value from json_each(action_items.source_message_ids)
                 ))
         from action_items",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    rows.map(|row| {
        let (id, profile_id, detected_day, first_detected_at, source_day) = row?;
        let known_day = source_day
            .as_deref()
            .and_then(canonical_day)
            .or_else(|| detected_day.as_deref().and_then(canonical_day));
        let day = if let Some(day) = known_day {
            ActionDay::Known(day)
        } else {
            legacy_candidates(&first_detected_at, minutes)?
        };
        Ok(ResolvedActionDay {
            id,
            profile_id,
            day,
        })
    })
    .collect()
}

fn add_calendar_candidates(days: &mut BTreeSet<String>, date: NaiveDate) {
    days.insert(date.format("%Y-%m-%d").to_string());
    // Legacy records do not retain their historical cache-clear setting.
    // Any supported setting can put early wall-clock times in the prior day.
    if let Some(previous) = date.pred_opt() {
        days.insert(previous.format("%Y-%m-%d").to_string());
    }
}

fn add_instant_candidates(
    days: &mut BTreeSet<String>,
    instant: DateTime<Local>,
    minutes: i64,
) -> anyhow::Result<()> {
    add_calendar_candidates(days, instant.date_naive());
    days.insert(dashboard_day_at(instant, minutes)?.day);
    // A civil date can be skipped; use the same prior-instant rule as the
    // dashboard resolver, rather than assuming date-1 was an actual local day.
    if let Some(start) = crate::messages::import_window::local_day_boundary(instant.date_naive()) {
        if let Some(previous) = Local.timestamp_opt(start.timestamp() - 1, 0).single() {
            days.insert(previous.date_naive().format("%Y-%m-%d").to_string());
        }
    }
    Ok(())
}

fn legacy_candidates(value: &str, minutes: i64) -> anyhow::Result<ActionDay> {
    let mut days = BTreeSet::new();
    if let Ok(instant) = DateTime::parse_from_rfc3339(value) {
        // Retain both the recorded civil date and the current-zone reading.
        // Neither proves the old writer's timezone or cache-clear policy.
        add_calendar_candidates(&mut days, instant.date_naive());
        add_instant_candidates(&mut days, instant.with_timezone(&Local), minutes)?;
    } else if let Some(value) = ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
    {
        match Local.from_local_datetime(&value) {
            LocalResult::Single(instant) => {
                add_calendar_candidates(&mut days, value.date());
                add_instant_candidates(&mut days, instant, minutes)?;
            }
            LocalResult::Ambiguous(first, second) => {
                add_calendar_candidates(&mut days, value.date());
                add_instant_candidates(&mut days, first, minutes)?;
                add_instant_candidates(&mut days, second, minutes)?;
            }
            LocalResult::None => return Ok(ActionDay::Unknown),
        }
    } else if let Some(date) =
        canonical_day(value).and_then(|date| NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok())
    {
        add_calendar_candidates(&mut days, date);
    } else {
        return Ok(ActionDay::Unknown);
    }
    Ok(ActionDay::LegacyCandidates(days))
}

// Uncertain totals occur once per profile, outside dailySummary, so consumers
// cannot mistake one unknown action for a separate action on each candidate day.
pub(crate) fn diagnostic_day_summary(days: &[ResolvedActionDay]) -> serde_json::Value {
    let mut profiles = BTreeMap::<&str, (usize, usize, usize)>::new();
    for action in days {
        let counts = profiles.entry(&action.profile_id).or_default();
        match &action.day {
            ActionDay::Known(_) => counts.0 += 1,
            ActionDay::LegacyCandidates(_) => counts.1 += 1,
            ActionDay::Unknown => counts.2 += 1,
        }
    }
    serde_json::json!({
        "schemaVersion": 1,
        "dailySummaryActionCountScope": "confirmed-day-only",
        "profiles": profiles.into_iter().map(|(id, (known, uncertain, unknown))| {
            serde_json::json!({
                "profileId": id,
                "confirmedActionCount": known,
                "uncertainActionCount": uncertain,
                "unassignedActionCount": unknown,
            })
        }).collect::<Vec<_>>()
    })
}

#[cfg(test)]
#[path = "action_day_tests.rs"]
mod tests;
