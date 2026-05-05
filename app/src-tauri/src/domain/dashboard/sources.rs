use std::collections::{BTreeSet, HashMap};

use rusqlite::params;

use crate::ai;
use crate::storage::models::SourceStat;

pub fn is_aggregate(profile_id: &str) -> bool {
    profile_id == "aggregate"
}

pub fn source_stat(
    profile_id: String,
    platform: String,
    remark: String,
    count: i64,
    chats: Vec<String>,
) -> SourceStat {
    let platform_label = platform_label(&platform).to_owned();
    let label = source_label(&platform, &remark);
    SourceStat {
        profile_id,
        platform,
        platform_label,
        remark,
        label,
        count,
        chats,
    }
}

pub fn platform_label(platform: &str) -> &str {
    match platform {
        "wechat" => "微信",
        "wecom" => "企业微信",
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        _ => platform,
    }
}

pub fn source_label(platform: &str, remark: &str) -> String {
    let label = platform_label(platform);
    let remark = remark.trim();
    if remark.is_empty() {
        label.to_owned()
    } else {
        format!("{label}（{remark}）")
    }
}

pub fn source_chats_for_terms(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    terms: &[&str],
    include_chat_name: bool,
) -> anyhow::Result<Vec<SourceStat>> {
    let terms = terms
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let mut sql = "select daily_messages.profile_id, daily_messages.platform,
                          coalesce(json_extract(profiles.config_json, '$.remark'), ''),
                          chat_name, msg_type, content
                   from daily_messages
                   left join profiles on profiles.id = daily_messages.profile_id
                   where day = ?1"
        .to_owned();
    if !is_aggregate(profile_id) {
        sql.push_str(" and daily_messages.profile_id = ?2");
    }
    sql.push_str(" order by timestamp desc limit 2000");
    let mut stmt = conn.prepare(&sql)?;
    let rows = if is_aggregate(profile_id) {
        stmt.query_map(params![day], map_source_chat_row)?
    } else {
        stmt.query_map(params![day, profile_id], map_source_chat_row)?
    };

    let mut grouped = HashMap::<(String, String, String), (i64, BTreeSet<String>)>::new();
    for row in rows {
        let (row_profile_id, platform, remark, chat_name, msg_type, raw_content) = row?;
        let Some(content) = ai::clean_message_content_for_ai(&msg_type, &raw_content) else {
            continue;
        };
        let haystack = if include_chat_name {
            format!("{chat_name}\n{content}")
        } else {
            content.clone()
        };
        if terms.iter().any(|term| contains_term(&haystack, term)) {
            let entry = grouped
                .entry((row_profile_id, platform, remark))
                .or_insert_with(|| (0, BTreeSet::new()));
            entry.0 += 1;
            entry.1.insert(chat_name);
        }
    }

    let mut sources = grouped
        .into_iter()
        .map(|((row_profile_id, platform, remark), (count, chats))| {
            source_stat(
                row_profile_id,
                platform,
                remark,
                count,
                chats.into_iter().take(8).collect(),
            )
        })
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.label.cmp(&right.label))
    });
    Ok(sources)
}

fn map_source_chat_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, String, String, String, String, String)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn contains_term(haystack: &str, term: &str) -> bool {
    if haystack.contains(term) {
        return true;
    }
    haystack
        .to_ascii_lowercase()
        .contains(&term.to_ascii_lowercase())
}
