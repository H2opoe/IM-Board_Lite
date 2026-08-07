use std::collections::{BTreeSet, HashMap, HashSet};

use super::*;

pub(crate) fn merge_incremental_summary_topics(
    summary: &mut AiSummary,
    existing_topics: &[SummaryTopicContext],
    candidates: &[SummaryCandidate],
) {
    let mut valid_message_ids = existing_topics
        .iter()
        .flat_map(|topic| topic.source_message_ids.iter().cloned())
        .collect::<HashSet<_>>();
    valid_message_ids.extend(
        candidates
            .iter()
            .flat_map(|candidate| candidate.source_message_ids.iter().cloned()),
    );
    let existing_by_id = existing_topics
        .iter()
        .map(|topic| (topic.id.clone(), topic))
        .collect::<HashMap<_, _>>();
    for topic in &mut summary.topics {
        let Some(object) = topic.as_object_mut() else {
            continue;
        };
        let id = object
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                let title = object
                    .get("title")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let summary = object
                    .get("summary")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                summary_topic_id(title, summary)
            });
        let mut source_message_ids = existing_by_id
            .get(&id)
            .map(|existing| {
                existing
                    .source_message_ids
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        source_message_ids.extend(
            object
                .get("sourceMessageIds")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(str::trim)
                        .filter(|id| !id.is_empty() && valid_message_ids.contains(*id))
                        .map(str::to_owned)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default(),
        );
        object.insert("id".to_owned(), serde_json::json!(id));
        object.insert(
            "sourceMessageIds".to_owned(),
            serde_json::json!(source_message_ids.iter().cloned().collect::<Vec<_>>()),
        );
        let count = if source_message_ids.is_empty() {
            existing_by_id
                .get(&id)
                .map(|topic| topic.count)
                .unwrap_or_default()
        } else if let Some(existing) = existing_by_id.get(&id) {
            if existing.source_message_ids.is_empty() {
                existing
                    .count
                    .saturating_add(source_message_ids.len().try_into().unwrap_or_default())
            } else {
                source_message_ids.len().try_into().unwrap_or_default()
            }
        } else {
            source_message_ids.len().try_into().unwrap_or_default()
        };
        object.insert("count".to_owned(), serde_json::json!(count));
    }
    let returned_ids = summary
        .topics
        .iter()
        .filter_map(|topic| topic.get("id").and_then(|value| value.as_str()))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    for existing in existing_topics {
        if returned_ids.contains(&existing.id) {
            continue;
        }
        summary
            .topics
            .push(summary_topic_context_to_value(existing));
    }
    merge_duplicate_summary_topics(&mut summary.topics);
    summary.topics.sort_by(|left, right| {
        let left_count = left
            .get("count")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        let right_count = right
            .get("count")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        right_count.cmp(&left_count)
    });
    summary.topics.truncate(MAX_SUMMARY_CANDIDATES);
}

pub(crate) fn merge_duplicate_summary_topics(topics: &mut Vec<serde_json::Value>) {
    let mut merged = Vec::<serde_json::Value>::new();
    let mut index_by_key = HashMap::<String, usize>::new();
    for topic in topics.drain(..) {
        let Some(key) = summary_topic_merge_key(&topic) else {
            merged.push(topic);
            continue;
        };
        if let Some(index) = index_by_key.get(&key).copied() {
            merge_summary_topic_value(&mut merged[index], topic);
        } else {
            index_by_key.insert(key, merged.len());
            merged.push(topic);
        }
    }
    *topics = merged;
}

fn summary_topic_merge_key(topic: &serde_json::Value) -> Option<String> {
    let title = normalize_summary_topic_text(topic.get("title")?.as_str()?);
    if title.is_empty() {
        return None;
    }
    let summary = topic
        .get("summary")
        .and_then(|value| value.as_str())
        .map(normalize_summary_topic_text)
        .unwrap_or_default();
    Some(format!("{title}\n{summary}"))
}

fn normalize_summary_topic_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("")
        .to_lowercase()
}

fn merge_summary_topic_value(target: &mut serde_json::Value, incoming: serde_json::Value) {
    let mut source_message_ids = topic_source_message_ids(target);
    source_message_ids.extend(topic_source_message_ids(&incoming));
    let incoming_count = incoming
        .get("count")
        .and_then(|value| value.as_i64())
        .unwrap_or_default();
    let target_count = target
        .get("count")
        .and_then(|value| value.as_i64())
        .unwrap_or_default();
    let merged_count = if source_message_ids.is_empty() {
        target_count.saturating_add(incoming_count)
    } else {
        source_message_ids.len().try_into().unwrap_or_default()
    };
    let source_chats = merged_topic_source_chats(target, &incoming);
    if let Some(object) = target.as_object_mut() {
        object.insert(
            "sourceMessageIds".to_owned(),
            serde_json::json!(source_message_ids.into_iter().collect::<Vec<_>>()),
        );
        object.insert("count".to_owned(), serde_json::json!(merged_count));
        object.insert("sourceChats".to_owned(), source_chats);
    }
}

fn topic_source_message_ids(topic: &serde_json::Value) -> BTreeSet<String> {
    topic
        .get("sourceMessageIds")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn merged_topic_source_chats(
    left: &serde_json::Value,
    right: &serde_json::Value,
) -> serde_json::Value {
    let mut seen = HashSet::<String>::new();
    let mut source_chats = Vec::<serde_json::Value>::new();
    for topic in [left, right] {
        let Some(items) = topic.get("sourceChats").and_then(|value| value.as_array()) else {
            continue;
        };
        for item in items {
            let chat_name = item
                .get("chatName")
                .or_else(|| item.get("chat_name"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let Some(chat_name) = chat_name else {
                continue;
            };
            let is_group = item
                .get("isGroup")
                .or_else(|| item.get("is_group"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let key = format!("{chat_name}\n{is_group}");
            if seen.insert(key) {
                source_chats.push(serde_json::json!({
                    "chatName": chat_name,
                    "isGroup": is_group,
                }));
            }
        }
    }
    serde_json::json!(source_chats)
}
