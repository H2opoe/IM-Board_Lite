use std::collections::HashMap;

use crate::ai::MIN_KEYWORD_CLOUD_COUNT;

use super::candidates::{
    build_local_keyword_segmenter, is_ascii_keyword, is_generic_single_term, is_local_stopword,
    keyword_char_count, local_keyword_candidates, looks_like_noise_keyword,
};
use super::repository::load_local_keyword_messages;
use super::text_cleaning::keyword_texts_from_message_content;
use super::types::{LocalKeywordRank, LocalKeywordScore, LocalKeywordSource};

const MAX_LOCAL_KEYWORDS: usize = 16;
const MIN_REDUNDANT_KEYWORD_OVERLAP: usize = 4;
const MIN_CONTAINED_KEYWORD_CHARS: usize = 2;

pub(crate) fn local_keywords(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let messages = load_local_keyword_messages(conn, day, profile_id)?;
    let (jieba, context_terms) = build_local_keyword_segmenter();
    let latest_message_timestamp = messages
        .iter()
        .map(|message| message.timestamp)
        .max()
        .unwrap_or_default();

    let mut scores = HashMap::<String, LocalKeywordScore>::new();
    for message in &messages {
        let mut seen = HashMap::<String, LocalKeywordSource>::new();
        for content in keyword_texts_from_message_content(&message.content) {
            for candidate in local_keyword_candidates(&jieba, &content, &context_terms) {
                seen.entry(candidate.text)
                    .and_modify(|source| {
                        if candidate.source.rank() > source.rank() {
                            *source = candidate.source;
                        }
                    })
                    .or_insert(candidate.source);
            }
        }

        for (candidate, source) in seen {
            let is_context = context_terms.contains(&candidate);
            let entry = scores.entry(candidate.clone()).or_default();
            entry.occurrences += 1;
            entry.chat_ids.insert(message.chat_id.clone());
            if is_context {
                entry.context_hits += 1;
            }
            if source.rank() > entry.best_source.rank() {
                entry.best_source = if is_context {
                    LocalKeywordSource::Context
                } else {
                    source
                };
            }
            entry.latest_timestamp = entry.latest_timestamp.max(message.timestamp);
            entry.score += local_keyword_score(&candidate, source, is_context);
        }
    }

    let mut items = scores
        .into_iter()
        .filter(|(text, value)| is_selectable_local_keyword(text, value))
        .map(|(text, value)| {
            let final_score = final_local_keyword_score(&text, &value, latest_message_timestamp);
            LocalKeywordRank {
                text,
                value,
                final_score,
            }
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .final_score
            .partial_cmp(&left.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| keyword_char_count(&right.text).cmp(&keyword_char_count(&left.text)))
            .then_with(|| left.text.cmp(&right.text))
    });

    let selected = select_local_keyword_ranks(items);

    Ok(selected
        .into_iter()
        .map(|item| {
            let weight = item.final_score.round().max(item.value.occurrences as f64) as i64;
            serde_json::json!({
                "text": item.text,
                "display": item.text,
                "score": item.final_score,
                "localScore": item.final_score,
                "aiScore": 0.0,
                "weight": weight.max(1),
                "count": item.value.occurrences,
                "messageCount": item.value.occurrences,
                "chatCount": item.value.chat_ids.len(),
                "category": local_keyword_category(&item.text),
                "confidence": 0.86,
                "aliases": [],
                "source": "local",
                "localSource": item.value.best_source.as_str()
            })
        })
        .collect())
}

pub(crate) fn select_local_keyword_ranks(items: Vec<LocalKeywordRank>) -> Vec<LocalKeywordRank> {
    let mut selected = Vec::<LocalKeywordRank>::new();
    for item in items {
        let overlapping = selected
            .iter()
            .enumerate()
            .filter_map(|(index, selected_item)| {
                keywords_have_redundant_overlap(&item.text, &selected_item.text).then_some(index)
            })
            .collect::<Vec<_>>();

        if overlapping.is_empty() {
            if selected.len() < MAX_LOCAL_KEYWORDS {
                selected.push(item);
            }
            continue;
        }

        let item_is_context = item.value.context_hits > 0;
        if !item_is_context
            && overlapping
                .iter()
                .any(|index| selected[*index].value.context_hits > 0)
        {
            continue;
        }
        if item_is_context {
            for index in overlapping
                .iter()
                .copied()
                .filter(|index| selected[*index].value.context_hits == 0)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                selected.remove(index);
            }
            if selected.iter().all(|selected_item| {
                !keywords_have_redundant_overlap(&item.text, &selected_item.text)
            }) && selected.len() < MAX_LOCAL_KEYWORDS
            {
                selected.push(item);
            }
            continue;
        }

        if overlapping.iter().any(|index| {
            keyword_char_count(&selected[*index].text) >= keyword_char_count(&item.text)
        }) {
            continue;
        }

        for index in overlapping.into_iter().rev() {
            selected.remove(index);
        }
        if selected.len() < MAX_LOCAL_KEYWORDS {
            selected.push(item);
        }
    }
    selected
}
fn local_keyword_score(token: &str, source: LocalKeywordSource, is_context: bool) -> f64 {
    let chars = keyword_char_count(token);
    let source = if is_context {
        LocalKeywordSource::Context
    } else {
        source
    };
    source.multiplier() * specificity_weight(token, chars)
}

fn final_local_keyword_score(
    text: &str,
    value: &LocalKeywordScore,
    latest_message_timestamp: i64,
) -> f64 {
    let message_count = (1.0 + value.occurrences as f64).ln();
    let chat_count = (1.0 + value.chat_ids.len() as f64).ln();
    let recency = recency_weight(value.latest_timestamp, latest_message_timestamp);
    let noise_penalty = if is_generic_single_term(text) {
        0.0
    } else {
        1.0
    };
    message_count * chat_count * value.score.max(0.1) * recency * noise_penalty
}

fn specificity_weight(token: &str, chars: usize) -> f64 {
    if is_generic_single_term(token) {
        return 0.0;
    }
    if is_ascii_keyword(token) {
        return 0.9;
    }
    if chars == 2 {
        0.6
    } else if (3..=6).contains(&chars) {
        1.0
    } else if (7..=12).contains(&chars) {
        1.1
    } else {
        0.75
    }
}

fn recency_weight(message_timestamp: i64, latest_message_timestamp: i64) -> f64 {
    if latest_message_timestamp <= 0 || message_timestamp <= 0 {
        return 1.0;
    }
    let age_seconds = latest_message_timestamp.saturating_sub(message_timestamp);
    if age_seconds <= 3600 {
        1.2
    } else if age_seconds <= 6 * 3600 {
        1.0
    } else {
        0.75
    }
}

fn local_keyword_category(text: &str) -> &'static str {
    let lower = text.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "飞书" | "钉钉" | "企微" | "企业微信" | "微信" | "hermes" | "erp" | "oa"
    ) {
        "tool_or_platform"
    } else if text.contains("问题")
        || text.contains("异常")
        || text.contains("投诉")
        || text.contains("卡")
    {
        "issue_or_risk"
    } else if text.contains("公司") {
        "organization"
    } else if text.contains("系统") {
        "system_or_project"
    } else {
        "business_topic"
    }
}

pub(crate) fn is_selectable_local_keyword(text: &str, value: &LocalKeywordScore) -> bool {
    if keyword_char_count(text) < 2
        || looks_like_noise_keyword(text)
        || is_local_stopword(text)
        || is_generic_single_term(text)
        || value.score <= 0.0
    {
        return false;
    }
    // 词云只展示真正反复出现的关键词，避免一次性闲聊或噪声词进入看板。
    if value.occurrences < MIN_KEYWORD_CLOUD_COUNT {
        return false;
    }
    value.context_hits > 0 || value.occurrences > 1 || keyword_char_count(text) >= 4
}

fn keywords_have_redundant_overlap(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    if keywords_have_containment_overlap(left, right) {
        return true;
    }
    longest_common_keyword_overlap(left, right) >= MIN_REDUNDANT_KEYWORD_OVERLAP
}

fn keywords_have_containment_overlap(left: &str, right: &str) -> bool {
    let left_chars = keyword_char_count(left);
    let right_chars = keyword_char_count(right);
    let shorter_chars = left_chars.min(right_chars);
    if shorter_chars < MIN_CONTAINED_KEYWORD_CHARS {
        return false;
    }
    left.contains(right) || right.contains(left)
}

fn longest_common_keyword_overlap(left: &str, right: &str) -> usize {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    if left_chars.is_empty() || right_chars.is_empty() {
        return 0;
    }

    let mut previous = vec![0usize; right_chars.len() + 1];
    let mut best = 0usize;
    for left_char in &left_chars {
        let mut current = vec![0usize; right_chars.len() + 1];
        for (right_index, right_char) in right_chars.iter().enumerate() {
            if left_char == right_char {
                current[right_index + 1] = previous[right_index] + 1;
                best = best.max(current[right_index + 1]);
            }
        }
        previous = current;
    }
    best
}
