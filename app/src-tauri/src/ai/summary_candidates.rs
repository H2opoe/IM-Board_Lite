use std::collections::{BTreeSet, HashMap, HashSet};

use rusqlite::{params, OptionalExtension};

use crate::analysis::local_keywords::{
    build_local_keyword_segmenter, contains_any, keyword_char_count,
    keyword_texts_from_message_content, local_keyword_candidates, ANALYSIS_RISK_TERMS,
    ANALYSIS_TASK_TERMS, ANALYSIS_URGENCY_TERMS,
};
use crate::analysis::message_noise;
use crate::messages::wechat_accounts::is_wechat_system_account;

use super::*;

pub(crate) fn load_summary_candidates(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SummaryCandidate>> {
    Ok(summary_candidates_from_messages(
        load_summary_candidate_messages(conn, day, profile_id)?,
    ))
}

pub(crate) fn load_summary_candidates_for_profiles(
    conn: &rusqlite::Connection,
    day: &str,
    profile_ids: &[String],
) -> anyhow::Result<Vec<SummaryCandidate>> {
    let mut messages = Vec::new();
    for profile_id in profile_ids {
        messages.extend(load_summary_candidate_messages(conn, day, profile_id)?);
    }
    messages.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.profile_id.cmp(&right.profile_id))
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    if messages.len() > MAX_SUMMARY_MESSAGES {
        messages = messages
            .into_iter()
            .rev()
            .take(MAX_SUMMARY_MESSAGES)
            .collect::<Vec<_>>();
        messages.reverse();
    }
    Ok(summary_candidates_from_messages(messages))
}

fn load_summary_candidate_messages(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<AnalysisMessage>> {
    let mut stmt = conn.prepare(
        "select id, profile_id, platform, chat_id, chat_name, is_group, timestamp,
                sender_id, sender_name, time_text, msg_type, content, partial
         from daily_messages
         where day = ?1 and profile_id = ?2 and topic_summarized_at is null and trim(content) <> ''
         order by timestamp asc
         limit ?3",
    )?;
    let rows = stmt.query_map(
        params![day, profile_id, MAX_SUMMARY_MESSAGES as i64],
        |row| {
            let chat_id: String = row.get(3)?;
            if is_wechat_system_account(&chat_id) {
                return Ok(None);
            }
            let sender_id: String = row.get(7)?;
            if is_wechat_system_account(&sender_id) {
                return Ok(None);
            }
            let sender_name: String = row.get(8)?;
            let is_me = is_self_sender(&sender_id, &sender_name);
            let msg_type: String = row.get(10)?;
            let raw_content: String = row.get(11)?;
            let is_group = row.get::<_, i64>(5)? == 1;
            if message_noise::is_message_noise(Some(&msg_type), &raw_content, is_group) {
                return Ok(None);
            }
            let Some(content) = clean_message_content_for_ai(&msg_type, &raw_content) else {
                return Ok(None);
            };
            Ok(Some(AnalysisMessage {
                id: row.get(0)?,
                profile_id: row.get(1)?,
                platform: row.get(2)?,
                chat_id,
                chat_name: row.get(4)?,
                is_group,
                timestamp: row.get(6)?,
                sender_id,
                sender_name,
                is_me,
                time_text: row.get(9)?,
                msg_type,
                content: truncate_text(content, 220),
                partial: row.get::<_, i64>(12)? == 1,
            }))
        },
    )?;
    Ok(rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>())
}

pub(crate) fn load_existing_summary_topics(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<SummaryTopicContext>> {
    let raw = conn
        .query_row(
            "select value_json from daily_stats where day = ?1 and profile_id = ?2 and metric = 'topics'",
            params![day, profile_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let topics = serde_json::from_str::<Vec<serde_json::Value>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(summary_topic_context_from_value)
        .collect::<Vec<_>>();
    Ok(topics)
}

#[derive(Debug, Default)]
struct SummaryCandidateDraft {
    score: f64,
    count: i64,
    source_chats: HashSet<SummarySourceChat>,
    source_message_ids: HashSet<String>,
    snippets: Vec<String>,
    keywords: HashSet<String>,
    risk: bool,
}

pub(crate) fn summary_candidates_from_messages(
    messages: Vec<AnalysisMessage>,
) -> Vec<SummaryCandidate> {
    let (jieba, context_terms) = build_local_keyword_segmenter();
    let mut drafts = HashMap::<String, SummaryCandidateDraft>::new();

    for message in messages {
        let content = message.content.trim();
        if should_skip_ai_message(&message) {
            continue;
        }
        let cleaned_texts = keyword_texts_from_message_content(content);
        if cleaned_texts.is_empty() {
            continue;
        }
        let source_chat = SummarySourceChat {
            chat_name: message.chat_name.clone(),
            is_group: message.is_group,
        };
        let risk = cleaned_texts
            .iter()
            .any(|text| contains_any(text, ANALYSIS_RISK_TERMS));
        let mut seen = HashSet::<String>::new();
        for text in cleaned_texts {
            let snippet_text = truncate_text(text.clone(), 80);
            let mut candidates = local_keyword_candidates(&jieba, &text, &context_terms)
                .into_iter()
                .map(|candidate| candidate.text)
                .filter(|candidate| keyword_char_count(candidate) >= 2)
                .collect::<Vec<_>>();
            candidates.sort_by(|left, right| {
                right
                    .chars()
                    .count()
                    .cmp(&left.chars().count())
                    .then_with(|| left.cmp(right))
            });
            candidates.truncate(5);
            if candidates.is_empty()
                && (risk
                    || contains_any(&text, ANALYSIS_TASK_TERMS)
                    || contains_any(&text, ANALYSIS_URGENCY_TERMS))
            {
                candidates.push(truncate_text(text.clone(), 16));
            }

            for keyword in candidates {
                if !seen.insert(keyword.clone()) {
                    continue;
                }
                // 热门话题先按“聊天 + 关键词”成候选，避免“买东西/采购/确认”等泛词在本地预筛阶段跨聊天误合并。
                let candidate_key =
                    format!("{}:{}|{}", message.profile_id, message.chat_id, keyword);
                let draft = drafts.entry(candidate_key).or_default();
                draft.count += 1;
                draft.score += if risk { 2.3 } else { 1.0 };
                draft.score += if source_chat.is_group { 0.25 } else { 0.0 };
                draft.source_chats.insert(source_chat.clone());
                draft.source_message_ids.insert(message.id.clone());
                draft.keywords.insert(keyword);
                draft.risk |= risk;
                if draft.snippets.len() < MAX_SUMMARY_SNIPPETS_PER_CANDIDATE {
                    draft.snippets.push(format!(
                        "{}: {}",
                        truncate_text(message.sender_name.clone(), 18),
                        snippet_text
                    ));
                }
            }
        }
    }

    let mut ranked = drafts
        .into_iter()
        .filter(|(key, draft)| {
            draft.count >= 2
                || draft.risk
                || draft.source_chats.len() >= 2
                || summary_candidate_keyword(key)
                    .map(keyword_char_count)
                    .unwrap_or_default()
                    >= 4
        })
        .map(|(key, mut draft)| {
            draft.score += draft.source_chats.len().saturating_sub(1) as f64 * 0.8;
            (
                summary_candidate_keyword(&key).unwrap_or(&key).to_owned(),
                draft,
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .score
            .partial_cmp(&left.1.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.1.count.cmp(&left.1.count))
            .then_with(|| left.0.cmp(&right.0))
    });

    ranked
        .into_iter()
        .take(MAX_SUMMARY_CANDIDATES)
        .enumerate()
        .map(|(index, (keyword, draft))| {
            let mut source_chats = draft.source_chats.into_iter().collect::<Vec<_>>();
            source_chats.sort_by(|left, right| left.chat_name.cmp(&right.chat_name));
            source_chats.truncate(5);
            let mut source_message_ids = draft.source_message_ids.into_iter().collect::<Vec<_>>();
            source_message_ids.sort();
            let mut keywords = draft.keywords.into_iter().collect::<Vec<_>>();
            keywords.sort();
            keywords.truncate(4);
            SummaryCandidate {
                id: format!("topic_candidate_{}", index + 1),
                title_hint: keyword,
                keywords,
                count: draft.count,
                source_chats,
                source_message_ids,
                snippets: draft.snippets,
                risk: draft.risk,
            }
        })
        .collect()
}

fn summary_candidate_keyword(key: &str) -> Option<&str> {
    key.split_once('|').map(|(_, keyword)| keyword)
}

fn summary_topic_context_from_value(value: serde_json::Value) -> Option<SummaryTopicContext> {
    let title = value
        .get("title")
        .and_then(|inner| inner.as_str())
        .map(str::trim)
        .filter(|inner| !inner.is_empty())?
        .to_owned();
    let summary = value
        .get("summary")
        .and_then(|inner| inner.as_str())
        .unwrap_or_default()
        .trim()
        .to_owned();
    let id = value
        .get("id")
        .and_then(|inner| inner.as_str())
        .map(str::trim)
        .filter(|inner| !inner.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| summary_topic_id(&title, &summary));
    let mut source_message_ids = value
        .get("sourceMessageIds")
        .and_then(|inner| inner.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();
    source_message_ids.sort();
    let count = if source_message_ids.is_empty() {
        value
            .get("count")
            .and_then(|inner| inner.as_i64())
            .unwrap_or_default()
    } else {
        source_message_ids.len().try_into().unwrap_or_default()
    };
    let source_chats = summary_source_chats_from_value(&value);
    Some(SummaryTopicContext {
        id,
        title,
        summary,
        count,
        source_chats,
        source_message_ids,
    })
}

pub(crate) fn summary_topic_contexts_from_summary(summary: &AiSummary) -> Vec<SummaryTopicContext> {
    summary
        .topics
        .iter()
        .cloned()
        .filter_map(summary_topic_context_from_value)
        .collect()
}

pub(crate) fn summary_topic_context_to_value(topic: &SummaryTopicContext) -> serde_json::Value {
    serde_json::json!({
        "id": topic.id,
        "title": topic.title,
        "summary": topic.summary,
        "count": topic.count,
        "sourceChats": topic.source_chats,
        "sourceMessageIds": topic.source_message_ids,
    })
}

fn summary_source_chats_from_value(value: &serde_json::Value) -> Vec<SummarySourceChat> {
    value
        .get("sourceChats")
        .and_then(|inner| inner.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let chat_name = item
                        .get("chatName")
                        .or_else(|| item.get("chat_name"))
                        .and_then(|inner| inner.as_str())
                        .map(str::trim)
                        .filter(|inner| !inner.is_empty())?;
                    let is_group = item
                        .get("isGroup")
                        .or_else(|| item.get("is_group"))
                        .and_then(|inner| inner.as_bool())
                        .unwrap_or(false);
                    Some(SummarySourceChat {
                        chat_name: chat_name.to_owned(),
                        is_group,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn summary_topic_id(title: &str, summary: &str) -> String {
    format!("topic_{}", &hash_text(&format!("{title}|{summary}"))[..16])
}
