use std::collections::HashMap;

use crate::analysis::local_keywords::{
    clean_link_or_app_message_text, clean_plain_message_text, contains_any,
    contains_disallowed_media_content, contains_group_membership_notice,
    contains_unsupported_client_notice, is_generic_single_term, is_local_stopword,
    looks_like_noise_keyword, ANALYSIS_COMPLETION_TERMS, ANALYSIS_REPLY_TERMS, ANALYSIS_RISK_TERMS,
    ANALYSIS_TASK_TERMS, ANALYSIS_URGENCY_TERMS,
};

use super::*;

pub(crate) fn filter_analysis_messages(messages: Vec<AnalysisMessage>) -> Vec<AnalysisMessage> {
    let mut chat_index = HashMap::<String, usize>::new();
    let mut chat_groups = Vec::<Vec<AnalysisMessage>>::new();
    for message in messages {
        if let Some(index) = chat_index.get(&message.chat_id).copied() {
            chat_groups[index].push(message);
            continue;
        }
        chat_index.insert(message.chat_id.clone(), chat_groups.len());
        chat_groups.push(vec![message]);
    }

    chat_groups
        .into_iter()
        .filter(|group| group_has_analysis_signal(group))
        .flatten()
        .collect()
}

pub(super) fn group_has_analysis_signal(messages: &[AnalysisMessage]) -> bool {
    let has_inbound = messages.iter().any(|message| !message.is_me);
    let has_self_completion = messages
        .iter()
        .any(|message| message.is_me && contains_any(&message.content, ANALYSIS_COMPLETION_TERMS));

    messages.iter().any(|message| {
        let content = message.content.trim();
        if should_skip_ai_message(&message) {
            return false;
        }
        let lower = content.to_ascii_lowercase();
        if contains_any(content, ANALYSIS_RISK_TERMS)
            || contains_any(content, ANALYSIS_TASK_TERMS)
            || contains_any(content, ANALYSIS_URGENCY_TERMS)
        {
            return true;
        }
        if message.is_me && contains_any(content, ANALYSIS_COMPLETION_TERMS) {
            return true;
        }
        if !has_inbound && !has_self_completion {
            return false;
        }
        if is_inbound_single_chat_file_signal(message) {
            return true;
        }
        if !message.is_group && !message.is_me && looks_like_reply_request(content) {
            return true;
        }
        message.is_group
            && (content.contains('@') || lower.contains("at我") || lower.contains("@me"))
    })
}

pub(super) fn looks_like_reply_request(content: &str) -> bool {
    let value = content
        .trim()
        .trim_matches(|ch: char| ch.is_ascii_punctuation() || "，。！？、；：… ".contains(ch));
    content.contains('?')
        || content.contains('？')
        || contains_any(content, ANALYSIS_REPLY_TERMS)
        || value.ends_with("吗")
        || value.ends_with("呢")
}

pub(crate) fn should_skip_ai_message(message: &AnalysisMessage) -> bool {
    let content = message.content.trim();
    content.is_empty()
        || should_skip_nonsemantic_message(content)
        || !is_allowed_ai_message_type(&message.msg_type, content)
        || contains_disallowed_media_content(content)
        || contains_unsupported_client_notice(content)
        || contains_group_membership_notice(content)
        || is_call_record_message(message)
}

pub(crate) fn clean_message_content_for_ai(msg_type: &str, content: &str) -> Option<String> {
    let trimmed = content.trim();
    if is_file_message_type(msg_type) {
        return clean_file_message_text(trimmed);
    }
    if trimmed.is_empty()
        || should_skip_nonsemantic_message(trimmed)
        || !is_allowed_ai_message_type(msg_type, trimmed)
        || contains_disallowed_media_content(trimmed)
        || contains_unsupported_client_notice(trimmed)
        || contains_group_membership_notice(trimmed)
        || is_call_record_text(msg_type, trimmed)
    {
        return None;
    }

    let text = if is_link_or_app_message_type(msg_type) || looks_like_link_or_app_payload(trimmed) {
        clean_link_or_app_message_text(trimmed)
    } else {
        clean_plain_message_text(trimmed)
    };
    let text = text.trim().to_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

pub(crate) fn is_disallowed_dashboard_topic(title: &str, summary: &str) -> bool {
    let text = format!("{}\n{}", title.trim(), summary.trim());
    is_call_record_text("text", &text)
        || contains_disallowed_media_content(&text)
        || contains_group_membership_notice(&text)
}

pub(crate) fn is_disallowed_dashboard_keyword(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    normalized.is_empty()
        || is_call_record_text("text", text)
        || contains_disallowed_media_content(text)
        || looks_like_noise_keyword(&normalized)
        || is_local_stopword(&normalized)
        || is_generic_single_term(text.trim())
        || crate::analysis::local_keywords::is_low_semantic_keyword(text.trim())
        || matches!(
            normalized.as_str(),
            "dmg"
                | "pkg"
                | "exe"
                | "msi"
                | "zip"
                | "rar"
                | "7z"
                | "aarch64"
                | "x86_64"
                | "arm64"
                | "x64"
                | "amd64"
        )
}

pub(super) fn is_allowed_ai_message_type(msg_type: &str, content: &str) -> bool {
    if is_excluded_ai_message_type(msg_type) {
        return false;
    }
    let kind = normalized_message_type(msg_type);
    matches!(
        kind.as_str(),
        "" | "1"
            | "text"
            | "location"
            | "48"
            | "link"
            | "url"
            | "app"
            | "appmsg"
            | "application"
            | "miniprogram"
            | "miniapp"
            | "news"
            | "card"
            | "49"
            | "file"
            | "document"
            | "system"
            | "sys"
            | "notice"
            | "notification"
            | "10000"
            | "10002"
    ) || (kind.is_empty() && !contains_disallowed_media_content(content))
}

pub(super) fn is_excluded_ai_message_type(msg_type: &str) -> bool {
    let kind = normalized_message_type(msg_type);
    kind.contains("image")
        || kind.contains("图片")
        || kind == "3"
        || kind.contains("voice")
        || kind.contains("语音")
        || kind == "34"
        || kind.contains("audio")
        || kind.contains("video")
        || kind.contains("视频")
        || kind == "43"
        || kind == "62"
        || kind.contains("emoji")
        || kind.contains("表情")
        || kind == "47"
        || kind.contains("call")
        || kind.contains("voip")
        || kind.contains("通话")
}

fn is_file_message_type(msg_type: &str) -> bool {
    let kind = normalized_message_type(msg_type);
    kind == "file" || kind == "document" || kind.contains("文件")
}

fn is_inbound_single_chat_file_signal(message: &AnalysisMessage) -> bool {
    !message.is_group
        && !message.is_me
        && is_file_message_type(&message.msg_type)
        && message.content.trim_start().starts_with("文件：")
}

fn clean_file_message_text(content: &str) -> Option<String> {
    let cleaned = content
        .trim()
        .trim_start_matches("[文件]")
        .trim_start_matches("【文件】")
        .trim_start_matches("文件")
        .trim_matches(|ch: char| ch.is_whitespace() || matches!(ch, ':' | '：' | '-' | '—'))
        .trim();
    if cleaned.is_empty() || cleaned == content.trim() && contains_disallowed_media_content(content)
    {
        return None;
    }
    Some(format!("文件：{}", truncate_text(cleaned.to_owned(), 240)))
}

pub(super) fn is_link_or_app_message_type(msg_type: &str) -> bool {
    let kind = normalized_message_type(msg_type);
    matches!(
        kind.as_str(),
        "link"
            | "url"
            | "app"
            | "appmsg"
            | "application"
            | "miniprogram"
            | "miniapp"
            | "news"
            | "card"
            | "49"
    )
}

pub(super) fn normalized_message_type(msg_type: &str) -> String {
    msg_type.trim().to_ascii_lowercase()
}

pub(super) fn looks_like_link_or_app_payload(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("<appmsg")
        || lower.contains("<title>")
        || lower.contains("\"title\"")
        || lower.contains("'title'")
        || lower.contains("description")
}

pub(super) fn should_skip_nonsemantic_message(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("<sysmsg")
        || lower.contains("<revokemsg")
        || lower.contains("今日已签到")
        || lower.contains("连续签到")
        || lower.contains("积分商城")
        || lower.contains("点击领取")
        || lower.contains("点击进入")
        || lower.contains("点击查看您的答题记录")
}

pub(super) fn is_call_record_message(message: &AnalysisMessage) -> bool {
    is_call_record_text(&message.msg_type, &message.content)
}

pub(super) fn is_call_record_text(msg_type: &str, content: &str) -> bool {
    let msg_type = normalized_message_type(msg_type);
    if msg_type.contains("call") || msg_type.contains("voip") || msg_type.contains("通话") {
        return true;
    }

    let content = content.trim();
    if content.is_empty() {
        return false;
    }
    let lower = content.to_ascii_lowercase();
    if lower.contains("voip") || lower.contains("voice call") || lower.contains("video call") {
        return true;
    }
    if content.contains("通话时长") || content.contains("通话记录") {
        return true;
    }
    let mentions_call = content.contains("语音通话")
        || content.contains("视频通话")
        || content.contains("[通话]")
        || content.contains("【通话】");
    mentions_call
        && contains_any(
            content,
            &[
                "已取消",
                "已拒绝",
                "未接通",
                "已结束",
                "通话结束",
                "通话时长",
                "对方无应答",
                "无人接听",
            ],
        )
}
