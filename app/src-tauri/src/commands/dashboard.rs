use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::Local;
use rusqlite::params;
use tauri::State;

use crate::ai;
use crate::commands::ai as ai_commands;
use crate::daily_cache;
use crate::storage::models::{ActionItem, DashboardData, DashboardMetric, SourceStat};
use crate::storage::AppState;

#[tauri::command]
pub fn get_dashboard(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<DashboardData, String> {
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    let day = daily_cache::current_dashboard_day(&conn).map_err(|err| err.to_string())?;
    let profile_filter = profile_id.unwrap_or_else(|| "aggregate".to_owned());

    let messages = count_daily(&conn, &day, &profile_filter).map_err(|err| err.to_string())?;
    let chats = count_chats(&conn, &day, &profile_filter).map_err(|err| err.to_string())?;
    let replies = list_actions(&conn, "reply", &profile_filter).map_err(|err| err.to_string())?;
    let tasks = list_actions(&conn, "task", &profile_filter).map_err(|err| err.to_string())?;
    let open_replies = replies.iter().filter(|item| item.status == "open").count() as i64;
    let open_tasks = tasks.iter().filter(|item| item.status == "open").count() as i64;
    let ai_configured = ai::get_config(&conn)
        .map(|config| ai_commands::is_configured_for_current_runtime(&config, &state))
        .unwrap_or(false);

    Ok(DashboardData {
        day: day.clone(),
        metrics: vec![
            DashboardMetric {
                key: "messages".to_owned(),
                label: "今天消息数".to_owned(),
                value: messages,
                sources: daily_source_counts(&conn, &day, &profile_filter).unwrap_or_default(),
            },
            DashboardMetric {
                key: "replies".to_owned(),
                label: "待我回复".to_owned(),
                value: open_replies,
                sources: action_source_counts(&conn, "reply", &profile_filter).unwrap_or_default(),
            },
            DashboardMetric {
                key: "tasks".to_owned(),
                label: "待办事项".to_owned(),
                value: open_tasks,
                sources: action_source_counts(&conn, "task", &profile_filter).unwrap_or_default(),
            },
            DashboardMetric {
                key: "chats".to_owned(),
                label: "对话/群聊数".to_owned(),
                value: chats,
                sources: chat_source_counts(&conn, &day, &profile_filter).unwrap_or_default(),
            },
        ],
        replies,
        tasks,
        topics: enrich_topics(
            &conn,
            &day,
            &profile_filter,
            dashboard_stats(&conn, &day, &profile_filter, "topics").unwrap_or_default(),
        )
        .unwrap_or_default(),
        chat_rank: chat_rank(&conn, &day, &profile_filter).unwrap_or_default(),
        speaker_top: speaker_top(&conn, &day, &profile_filter).unwrap_or_default(),
        hourly_activity: hourly_activity(&conn, &day, &profile_filter).unwrap_or_default(),
        message_types: message_types(&conn, &day, &profile_filter).unwrap_or_default(),
        keywords: enrich_keywords(
            &conn,
            &day,
            &profile_filter,
            dashboard_stats(&conn, &day, &profile_filter, "keywords").unwrap_or_default(),
        )
        .unwrap_or_default(),
        ai_status: if ai_configured {
            "ready".to_owned()
        } else {
            "not_configured".to_owned()
        },
        sync_status: "idle".to_owned(),
    })
}

#[tauri::command]
pub fn mark_action_item(
    state: State<'_, AppState>,
    action_id: String,
    status: String,
) -> Result<(), String> {
    let completed_at = if status == "done" {
        Some(Local::now().to_rfc3339())
    } else {
        None
    };
    let conn = state.db.lock().map_err(|err| err.to_string())?;
    conn.execute(
        "update action_items
         set status = ?1, completed_at = ?2, last_updated_at = datetime('now')
         where id = ?3",
        params![status, completed_at, action_id],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn is_aggregate(profile_id: &str) -> bool {
    profile_id == "aggregate"
}

fn count_daily(conn: &rusqlite::Connection, day: &str, profile_id: &str) -> anyhow::Result<i64> {
    if is_aggregate(profile_id) {
        Ok(conn.query_row(
            "select count(*) from daily_messages where day = ?1",
            params![day],
            |row| row.get(0),
        )?)
    } else {
        Ok(conn.query_row(
            "select count(*) from daily_messages where day = ?1 and profile_id = ?2",
            params![day, profile_id],
            |row| row.get(0),
        )?)
    }
}

fn count_chats(conn: &rusqlite::Connection, day: &str, profile_id: &str) -> anyhow::Result<i64> {
    if is_aggregate(profile_id) {
        Ok(conn.query_row(
            "select count(distinct chat_id) from daily_messages where day = ?1",
            params![day],
            |row| row.get(0),
        )?)
    } else {
        Ok(conn.query_row(
            "select count(distinct chat_id) from daily_messages where day = ?1 and profile_id = ?2",
            params![day, profile_id],
            |row| row.get(0),
        )?)
    }
}

fn daily_source_counts(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SourceStat>> {
    source_counts(
        conn,
        &format!(
            "select daily_messages.profile_id, daily_messages.platform,
                    coalesce(json_extract(profiles.config_json, '$.remark'), ''), count(*)
             from daily_messages
             left join profiles on profiles.id = daily_messages.profile_id
             where daily_messages.day = ?1{}
             group by daily_messages.profile_id, daily_messages.platform
             order by count(*) desc",
            if is_aggregate(profile_id) {
                ""
            } else {
                " and daily_messages.profile_id = ?2"
            }
        ),
        day,
        profile_id,
    )
}

fn action_source_counts(
    conn: &rusqlite::Connection,
    item_type: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SourceStat>> {
    let sql = format!(
        "select action_items.profile_id, action_items.platform,
                coalesce(json_extract(profiles.config_json, '$.remark'), ''), count(*)
         from action_items
         left join profiles on profiles.id = action_items.profile_id
         where action_items.status = 'open' and action_items.type = ?1{}
         group by action_items.profile_id, action_items.platform
         order by count(*) desc",
        if is_aggregate(profile_id) {
            ""
        } else {
            " and action_items.profile_id = ?2"
        }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = if is_aggregate(profile_id) {
        stmt.query_map(params![item_type], map_source_count)?
    } else {
        stmt.query_map(params![item_type, profile_id], map_source_count)?
    };
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn chat_source_counts(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SourceStat>> {
    source_counts(
        conn,
        &format!(
            "select daily_messages.profile_id, daily_messages.platform,
                    coalesce(json_extract(profiles.config_json, '$.remark'), ''),
                    count(distinct daily_messages.chat_id)
             from daily_messages
             left join profiles on profiles.id = daily_messages.profile_id
             where daily_messages.day = ?1{}
             group by daily_messages.profile_id, daily_messages.platform
             order by count(distinct daily_messages.chat_id) desc",
            if is_aggregate(profile_id) {
                ""
            } else {
                " and daily_messages.profile_id = ?2"
            }
        ),
        day,
        profile_id,
    )
}

fn source_counts(
    conn: &rusqlite::Connection,
    sql: &str,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SourceStat>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = if is_aggregate(profile_id) {
        stmt.query_map(params![day], map_source_count)?
    } else {
        stmt.query_map(params![day, profile_id], map_source_count)?
    };
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn map_source_count(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceStat> {
    let profile_id: String = row.get(0)?;
    let platform: String = row.get(1)?;
    let remark: String = row.get(2)?;
    let count: i64 = row.get(3)?;
    Ok(source_stat(profile_id, platform, remark, count, Vec::new()))
}

fn source_stat(
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

fn platform_label(platform: &str) -> &str {
    match platform {
        "wechat" => "微信",
        "wecom" => "企业微信",
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        _ => platform,
    }
}

fn source_label(platform: &str, remark: &str) -> String {
    let label = platform_label(platform);
    let remark = remark.trim();
    if remark.is_empty() {
        label.to_owned()
    } else {
        format!("{label}（{remark}）")
    }
}

fn list_actions(
    conn: &rusqlite::Connection,
    item_type: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<ActionItem>> {
    let sql = if is_aggregate(profile_id) {
        "select action_items.id, type, action_items.status, priority, title, description, suggested_reply,
                profile_id, action_items.platform, chat_id, chat_name, evidence_summary,
                context_incomplete, carry_over, last_updated_at, completed_at,
                coalesce(json_extract(profiles.config_json, '$.remark'), ''),
                coalesce((
                    select datetime(max(daily_messages.timestamp), 'unixepoch', 'localtime')
                    from daily_messages
                    where daily_messages.id in (
                        select value from json_each(action_items.source_message_ids)
                    )
                ), last_updated_at) as source_message_at
         from action_items
         left join profiles on profiles.id = action_items.profile_id
         where type = ?1
         order by case action_items.status when 'open' then 0 when 'done' then 1 else 2 end,
                  case when action_items.status = 'open' then source_message_at end asc,
                  case when action_items.status = 'done' then coalesce(completed_at, last_updated_at) end desc,
                  case when action_items.status not in ('open', 'done') then source_message_at end desc"
    } else {
        "select action_items.id, type, action_items.status, priority, title, description, suggested_reply,
                profile_id, action_items.platform, chat_id, chat_name, evidence_summary,
                context_incomplete, carry_over, last_updated_at, completed_at,
                coalesce(json_extract(profiles.config_json, '$.remark'), ''),
                coalesce((
                    select datetime(max(daily_messages.timestamp), 'unixepoch', 'localtime')
                    from daily_messages
                    where daily_messages.id in (
                        select value from json_each(action_items.source_message_ids)
                    )
                ), last_updated_at) as source_message_at
         from action_items
         left join profiles on profiles.id = action_items.profile_id
         where type = ?1 and profile_id = ?2
         order by case action_items.status when 'open' then 0 when 'done' then 1 else 2 end,
                  case when action_items.status = 'open' then source_message_at end asc,
                  case when action_items.status = 'done' then coalesce(completed_at, last_updated_at) end desc,
                  case when action_items.status not in ('open', 'done') then source_message_at end desc"
    };

    let mut stmt = conn.prepare(sql)?;
    if is_aggregate(profile_id) {
        let rows = stmt.query_map(params![item_type], map_action)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    } else {
        let rows = stmt.query_map(params![item_type, profile_id], map_action)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn map_action(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActionItem> {
    let platform: String = row.get(8)?;
    let platform_remark: String = row.get(16)?;
    let platform_label = platform_label(&platform).to_owned();
    let source_label = source_label(&platform, &platform_remark);
    Ok(ActionItem {
        id: row.get(0)?,
        item_type: row.get(1)?,
        status: row.get(2)?,
        priority: row.get(3)?,
        title: row.get(4)?,
        description: row.get(5)?,
        suggested_reply: row.get(6)?,
        profile_id: row.get(7)?,
        platform,
        platform_label,
        platform_remark,
        source_label,
        chat_id: row.get(9)?,
        chat_name: row.get(10)?,
        evidence_summary: row.get(11)?,
        context_incomplete: row.get::<_, i64>(12)? == 1,
        carry_over: row.get::<_, i64>(13)? == 1,
        source_message_at: row.get(17)?,
        last_updated_at: row.get(14)?,
        completed_at: row.get(15)?,
    })
}

fn load_json_stats(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    metric: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let value: String = if is_aggregate(profile_id) {
        conn.query_row("select value_json from daily_stats where day = ?1 and profile_id = 'aggregate' and metric = ?2", params![day, metric], |row| row.get(0))?
    } else {
        conn.query_row(
            "select value_json from daily_stats where day = ?1 and profile_id = ?2 and metric = ?3",
            params![day, profile_id, metric],
            |row| row.get(0),
        )?
    };
    Ok(serde_json::from_str(&value)?)
}

fn dashboard_stats(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    metric: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    if let Ok(values) = load_json_stats(conn, day, profile_id, metric) {
        if !values.is_empty() {
            return Ok(values);
        }
    }

    if is_aggregate(profile_id) {
        let mut stmt = conn.prepare(
            "select value_json from daily_stats
             where day = ?1 and profile_id <> 'aggregate' and metric = ?2",
        )?;
        let rows = stmt.query_map(params![day, metric], |row| row.get::<_, String>(0))?;
        let mut values = Vec::new();
        for row in rows {
            if let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(&row?) {
                values.extend(items);
            }
        }
        let merged = if metric == "keywords" {
            merge_keywords(values)
        } else {
            merge_topics(values)
        };
        if !merged.is_empty() {
            return Ok(merged);
        }
    }

    Ok(Vec::new())
}

fn merge_topics(values: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    values
        .into_iter()
        .filter(|value| {
            let title = value
                .get("title")
                .and_then(|inner| inner.as_str())
                .unwrap_or_default();
            let summary = value
                .get("summary")
                .and_then(|inner| inner.as_str())
                .unwrap_or_default();
            !ai::is_disallowed_dashboard_topic(title, summary)
        })
        .take(12)
        .collect()
}

fn merge_keywords(values: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut counts = std::collections::HashMap::<String, (i64, i64)>::new();
    for value in values {
        if let Some(text) = value.get("text").and_then(|inner| inner.as_str()) {
            if ai::is_disallowed_dashboard_keyword(text) {
                continue;
            }
            let weight = value
                .get("weight")
                .and_then(|inner| inner.as_i64())
                .unwrap_or(1);
            let count = value
                .get("count")
                .and_then(|inner| inner.as_i64())
                .unwrap_or(weight);
            let entry = counts.entry(text.to_owned()).or_insert((0, 0));
            entry.0 += weight;
            entry.1 += count;
        }
    }
    let mut items = counts.into_iter().collect::<Vec<_>>();
    items.sort_by(
        |(left_text, (left_weight, _)), (right_text, (right_weight, _))| {
            right_weight
                .cmp(left_weight)
                .then_with(|| left_text.cmp(right_text))
        },
    );
    items
        .into_iter()
        .filter(|(_, (_, count))| *count >= ai::MIN_KEYWORD_CLOUD_COUNT)
        .take(16)
        .map(|(text, (weight, count))| {
            serde_json::json!({ "text": text, "weight": weight, "count": count })
        })
        .collect()
}

fn enrich_topics(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    topics: Vec<serde_json::Value>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    topics
        .into_iter()
        .filter(|topic| {
            let title = topic
                .get("title")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let summary = topic
                .get("summary")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            !ai::is_disallowed_dashboard_topic(title, summary)
        })
        .map(|mut topic| {
            let title = topic
                .get("title")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let summary = topic
                .get("summary")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let mut terms = vec![title, summary];
            let hinted_chats = topic_source_chat_names(&topic);
            terms.extend(hinted_chats.iter().map(String::as_str));
            let sources = source_chats_for_terms(conn, day, profile_id, &terms, true)?;
            if let Some(object) = topic.as_object_mut() {
                object.insert("sources".to_owned(), serde_json::to_value(sources)?);
            }
            Ok(topic)
        })
        .collect()
}

fn topic_source_chat_names(topic: &serde_json::Value) -> Vec<String> {
    topic
        .get("sourceChats")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("chatName")
                        .or_else(|| item.get("chat_name"))
                        .and_then(|value| value.as_str())
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn enrich_keywords(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    keywords: Vec<serde_json::Value>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    keywords
        .into_iter()
        .filter(|keyword| {
            keyword
                .get("text")
                .and_then(|value| value.as_str())
                .is_some_and(|text| !ai::is_disallowed_dashboard_keyword(text))
        })
        .map(|mut keyword| {
            let text = keyword
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let sources = source_chats_for_terms(conn, day, profile_id, &[text], false)?;
            let count = sources.iter().map(|source| source.count).sum::<i64>();
            // 兼容旧缓存：即使 daily_stats 里已有低频词，展示前也用消息命中次数兜底过滤。
            if count < ai::MIN_KEYWORD_CLOUD_COUNT {
                return Ok(None);
            }
            if let Some(object) = keyword.as_object_mut() {
                object.insert("sources".to_owned(), serde_json::to_value(sources)?);
                object.insert("count".to_owned(), serde_json::json!(count));
            }
            Ok(Some(keyword))
        })
        .filter_map(Result::transpose)
        .collect()
}

fn source_chats_for_terms(
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

fn chat_rank(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut sql = "select chat_name, daily_messages.platform,
                          coalesce(json_extract(profiles.config_json, '$.remark'), ''), count(*)
                   from daily_messages
                   left join profiles on profiles.id = daily_messages.profile_id
                   where day = ?1 and is_group = 0"
        .to_owned();
    if !is_aggregate(profile_id) {
        sql.push_str(" and daily_messages.profile_id = ?2");
    }
    sql.push_str(
        " group by daily_messages.profile_id, chat_id, chat_name, daily_messages.platform
          having sum(case when lower(trim(sender_id)) in ('me', 'self') or trim(sender_name) = '我' or lower(trim(sender_name)) in ('me', 'self') then 1 else 0 end) > 0
             and sum(case when lower(trim(sender_id)) not in ('me', 'self') and trim(sender_name) <> '我' and lower(trim(sender_name)) not in ('me', 'self') then 1 else 0 end) > 0
          order by count(*) desc limit 10",
    );
    let mut stmt = conn.prepare(&sql)?;
    if is_aggregate(profile_id) {
        let rows = stmt.query_map(params![day], map_chat_rank_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    } else {
        let rows = stmt.query_map(params![day, profile_id], map_chat_rank_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn map_chat_rank_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<serde_json::Value> {
    let chat: String = row.get(0)?;
    let platform: String = row.get(1)?;
    let remark: String = row.get(2)?;
    let count: i64 = row.get(3)?;
    Ok(serde_json::json!({
        "chat": chat,
        "count": count,
        "sourceLabel": source_label(&platform, &remark)
    }))
}

fn speaker_top(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    sender_rank(conn, day, profile_id, "speaker")
}

#[derive(Debug)]
struct SenderRankRow {
    platform: String,
    remark: String,
    profile_id: String,
    sender_id: String,
    sender_name: String,
    chat_id: String,
    chat_name: String,
    is_group: bool,
    content: String,
}

fn sender_rank(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
    label_key: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let reciprocal_direct_chats = reciprocal_direct_chats(conn, day, profile_id)?;
    let mut sql = "select daily_messages.platform,
                          coalesce(json_extract(profiles.config_json, '$.remark'), ''),
                          daily_messages.profile_id, sender_id, sender_name, chat_id, chat_name, is_group, content
                   from daily_messages
                   left join profiles on profiles.id = daily_messages.profile_id
                   where day = ?1"
        .to_owned();
    if !is_aggregate(profile_id) {
        sql.push_str(" and daily_messages.profile_id = ?2");
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = if is_aggregate(profile_id) {
        stmt.query_map(params![day], map_sender_rank_row)?
    } else {
        stmt.query_map(params![day, profile_id], map_sender_rank_row)?
    };

    let mut counts = HashMap::<(String, String), i64>::new();
    for row in rows {
        let row = row?;
        if !row.is_group
            && !reciprocal_direct_chats.contains(&(row.profile_id.clone(), row.chat_id.clone()))
        {
            continue;
        }
        if is_self_sender(&row.sender_id) || is_self_sender(&row.sender_name) {
            continue;
        }
        if let Some(sender) = display_sender(&row) {
            *counts
                .entry((sender, source_label(&row.platform, &row.remark)))
                .or_insert(0) += 1;
        }
    }

    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(
        |((left_name, left_source), left_count), ((right_name, right_source), right_count)| {
            right_count
                .cmp(left_count)
                .then_with(|| left_name.cmp(right_name))
                .then_with(|| left_source.cmp(right_source))
        },
    );
    ranked.truncate(10);

    Ok(ranked
        .into_iter()
        .map(|((label, source_label), count)| serde_json::json!({ label_key: label, "count": count, "sourceLabel": source_label }))
        .collect())
}

fn map_sender_rank_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SenderRankRow> {
    Ok(SenderRankRow {
        platform: row.get(0)?,
        remark: row.get(1)?,
        profile_id: row.get(2)?,
        sender_id: row.get(3)?,
        sender_name: row.get(4)?,
        chat_id: row.get(5)?,
        chat_name: row.get(6)?,
        is_group: row.get::<_, i64>(7)? == 1,
        content: row.get(8)?,
    })
}

fn reciprocal_direct_chats(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<HashSet<(String, String)>> {
    let mut sql = "select profile_id, chat_id
                   from daily_messages
                   where day = ?1 and is_group = 0"
        .to_owned();
    if !is_aggregate(profile_id) {
        sql.push_str(" and profile_id = ?2");
    }
    sql.push_str(
        " group by profile_id, chat_id
          having sum(case when lower(trim(sender_id)) in ('me', 'self') or trim(sender_name) = '我' or lower(trim(sender_name)) in ('me', 'self') then 1 else 0 end) > 0
             and sum(case when lower(trim(sender_id)) not in ('me', 'self') and trim(sender_name) <> '我' and lower(trim(sender_name)) not in ('me', 'self') then 1 else 0 end) > 0",
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = if is_aggregate(profile_id) {
        stmt.query_map(params![day], map_profile_chat_row)?
    } else {
        stmt.query_map(params![day, profile_id], map_profile_chat_row)?
    };
    rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
}

fn map_profile_chat_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String)> {
    Ok((row.get(0)?, row.get(1)?))
}

fn display_sender(row: &SenderRankRow) -> Option<String> {
    if row.is_group {
        if let Some(sender) = content_sender_prefix(&row.content) {
            return Some(sender);
        }
    }

    let sender_name = row.sender_name.trim();
    if sender_name.is_empty() {
        return None;
    }
    if row.is_group && (sender_name == row.chat_name.trim() || sender_name == row.chat_id.trim()) {
        return None;
    }
    Some(sender_name.to_owned())
}

fn content_sender_prefix(content: &str) -> Option<String> {
    let body = if let Some((_, body)) = content.trim().strip_prefix('[')?.split_once("] ") {
        body
    } else {
        content.trim()
    };
    let (sender, message) = body.split_once(": ")?;
    let sender = sender.trim();
    if sender.is_empty() || message.trim().is_empty() || is_self_sender(sender) {
        None
    } else {
        Some(sender.to_owned())
    }
}

fn is_self_sender(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "me" | "self" | "我"
    )
}

fn message_types(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    group_count(conn, day, profile_id, "msg_type", "type")
}

fn hourly_activity(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keywords_requires_minimum_keyword_cloud_count() {
        let merged = merge_keywords(vec![
            serde_json::json!({ "text": "低频词", "weight": 99, "count": 2 }),
            serde_json::json!({ "text": "高频词", "weight": 3, "count": 3 }),
            serde_json::json!({ "text": "聚合词", "weight": 1, "count": 1 }),
            serde_json::json!({ "text": "聚合词", "weight": 1, "count": 2 }),
        ]);
        let texts = merged
            .iter()
            .filter_map(|value| value.get("text").and_then(|value| value.as_str()))
            .collect::<Vec<_>>();
        assert!(!texts.contains(&"低频词"), "got {texts:?}");
        assert!(texts.contains(&"高频词"), "got {texts:?}");
        assert!(texts.contains(&"聚合词"), "got {texts:?}");
    }
}
