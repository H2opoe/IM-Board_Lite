use rusqlite::params;

use crate::ai;

use super::sources::{is_aggregate, source_chats_for_terms};

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

pub fn dashboard_stats(
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

pub fn enrich_topics(
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

pub fn enrich_keywords(
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
