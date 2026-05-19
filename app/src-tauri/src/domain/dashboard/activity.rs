use std::collections::HashMap;

use chrono::{Local, TimeZone};

use crate::storage::models::SourceStat;

use super::sources::{load_effective_message_rows, source_stat};

pub fn message_types(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    group_count(conn, day, profile_id, "msg_type", "type")
}

pub fn hourly_activity(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut hours = (0..24)
        .map(|hour| serde_json::json!({ "hour": format!("{hour:02}"), "count": 0, "sources": [] }))
        .collect::<Vec<_>>();
    let mut grouped = HashMap::<(i64, String, String, String), i64>::new();
    for message in load_effective_message_rows(conn, day, profile_id)? {
        let Some(datetime) = Local.timestamp_opt(message.timestamp, 0).single() else {
            continue;
        };
        let hour = datetime
            .format("%H")
            .to_string()
            .parse::<i64>()
            .unwrap_or(0);
        *grouped
            .entry((hour, message.profile_id, message.platform, message.remark))
            .or_insert(0) += 1;
    }
    let mut grouped_sources = HashMap::<i64, Vec<SourceStat>>::new();
    for ((hour, row_profile_id, platform, remark), count) in grouped {
        grouped_sources.entry(hour).or_default().push(source_stat(
            row_profile_id,
            platform,
            remark,
            count,
            Vec::new(),
        ));
    }
    for (hour, mut sources) in grouped_sources {
        if let Some(slot) = hours.get_mut(hour as usize) {
            sources.sort_by(|left, right| {
                right
                    .count
                    .cmp(&left.count)
                    .then_with(|| left.label.cmp(&right.label))
            });
            let count: i64 = sources.iter().map(|source| source.count).sum();
            *slot = serde_json::json!({ "hour": format!("{hour:02}"), "count": count, "sources": sources });
        }
    }
    Ok(hours)
}

fn group_count(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    column: &str,
    label_key: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut counts = HashMap::<(String, String, String, String), i64>::new();
    for message in load_effective_message_rows(conn, day, profile_id)? {
        let label = match column {
            "msg_type" => message.msg_type,
            _ => message.msg_type,
        };
        *counts
            .entry((label, message.profile_id, message.platform, message.remark))
            .or_insert(0) += 1;
    }
    let mut grouped = HashMap::<String, Vec<SourceStat>>::new();
    for ((label, row_profile_id, platform, remark), count) in counts {
        grouped.entry(label).or_default().push(source_stat(
            row_profile_id,
            platform,
            remark,
            count,
            Vec::new(),
        ));
    }
    let mut values = grouped
        .into_iter()
        .map(|(label, mut sources)| {
            sources.sort_by(|left, right| {
                right
                    .count
                    .cmp(&left.count)
                    .then_with(|| left.label.cmp(&right.label))
            });
            let count: i64 = sources.iter().map(|source| source.count).sum();
            serde_json::json!({ label_key: label, "count": count, "sources": sources })
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .get("count")
            .and_then(|value| value.as_i64())
            .unwrap_or_default()
            .cmp(
                &left
                    .get("count")
                    .and_then(|value| value.as_i64())
                    .unwrap_or_default(),
            )
    });
    values.truncate(10);
    Ok(values)
}
