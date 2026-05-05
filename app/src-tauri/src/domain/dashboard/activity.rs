use std::collections::HashMap;

use rusqlite::params;

use crate::storage::models::SourceStat;

use super::sources::{is_aggregate, source_stat};

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
    let mut sql = "select cast(strftime('%H', datetime(timestamp, 'unixepoch', 'localtime')) as integer) as hour,
                          daily_messages.profile_id, daily_messages.platform,
                          coalesce(json_extract(profiles.config_json, '$.remark'), ''),
                          count(*)
                   from daily_messages where day = ?1".to_owned();
    sql = sql.replace(
        "from daily_messages where",
        "from daily_messages left join profiles on profiles.id = daily_messages.profile_id where",
    );
    if !is_aggregate(profile_id) {
        sql.push_str(" and daily_messages.profile_id = ?2");
    }
    sql.push_str(" group by hour, daily_messages.profile_id, daily_messages.platform");
    let mut stmt = conn.prepare(&sql)?;
    let mut grouped = HashMap::<i64, Vec<SourceStat>>::new();
    if is_aggregate(profile_id) {
        let rows = stmt.query_map(params![day], map_hour_source_row)?;
        for row in rows {
            let (hour, source) = row?;
            grouped.entry(hour).or_default().push(source);
        }
    } else {
        let rows = stmt.query_map(params![day, profile_id], map_hour_source_row)?;
        for row in rows {
            let (hour, source) = row?;
            grouped.entry(hour).or_default().push(source);
        }
    }
    for (hour, mut sources) in grouped {
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

fn map_hour_source_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, SourceStat)> {
    let hour: i64 = row.get(0)?;
    let profile_id: String = row.get(1)?;
    let platform: String = row.get(2)?;
    let remark: String = row.get(3)?;
    let count: i64 = row.get(4)?;
    Ok((
        hour,
        source_stat(profile_id, platform, remark, count, Vec::new()),
    ))
}

fn group_count(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    column: &str,
    label_key: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut sql = format!(
        "select {column}, daily_messages.profile_id, daily_messages.platform,
                coalesce(json_extract(profiles.config_json, '$.remark'), ''), count(*)
         from daily_messages
         left join profiles on profiles.id = daily_messages.profile_id
         where day = ?1"
    );
    if !is_aggregate(profile_id) {
        sql.push_str(" and daily_messages.profile_id = ?2");
    }
    sql.push_str(&format!(" group by {column}, daily_messages.profile_id, daily_messages.platform order by count(*) desc"));
    let mut stmt = conn.prepare(&sql)?;
    let rows = if is_aggregate(profile_id) {
        stmt.query_map(params![day], map_group_source_row)?
    } else {
        stmt.query_map(params![day, profile_id], map_group_source_row)?
    };
    let mut grouped = HashMap::<String, Vec<SourceStat>>::new();
    for row in rows {
        let (label, source) = row?;
        grouped.entry(label).or_default().push(source);
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

fn map_group_source_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, SourceStat)> {
    let label: String = row.get(0)?;
    let profile_id: String = row.get(1)?;
    let platform: String = row.get(2)?;
    let remark: String = row.get(3)?;
    let count: i64 = row.get(4)?;
    Ok((
        label,
        source_stat(profile_id, platform, remark, count, Vec::new()),
    ))
}
