use std::collections::{HashMap, HashSet};

use chrono::{Local, NaiveDateTime, TimeZone};
use sha2::{Digest, Sha256};

use crate::storage::models::ImProfile;
use crate::sync::fetch::MessageImportWindow;

pub(crate) fn profile_remark(profile: &ImProfile) -> String {
    profile
        .config_json
        .get("remark")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&profile.label)
        .to_owned()
}

pub(crate) fn platform_label(platform: &str) -> &str {
    match platform {
        "wechat" => "微信",
        "wecom" => "企业微信",
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        _ => platform,
    }
}

#[derive(Debug)]
pub(crate) struct DailyMessage {
    pub(crate) id: String,
    pub(crate) day: String,
    pub(crate) profile_id: String,
    pub(crate) platform: String,
    pub(crate) chat_id: String,
    pub(crate) chat_name: String,
    pub(crate) is_group: bool,
    pub(crate) sender_id: String,
    pub(crate) sender_name: String,
    pub(crate) timestamp: i64,
    pub(crate) time_text: String,
    pub(crate) msg_type: String,
    pub(crate) content: String,
    pub(crate) raw_type: Option<String>,
    pub(crate) local_id: Option<String>,
    pub(crate) raw_json: String,
    pub(crate) content_hash: String,
    pub(crate) partial: bool,
}

pub(crate) fn normalize_message(
    profile: &ImProfile,
    value: &serde_json::Value,
    window: &MessageImportWindow,
    fallback_chat_id: &str,
    fallback_chat_name: &str,
    fallback_is_group: bool,
) -> Option<DailyMessage> {
    if let Some(line) = value.as_str() {
        return normalize_text_message(
            profile,
            line,
            window,
            fallback_chat_id,
            fallback_chat_name,
            fallback_is_group,
        );
    }

    let timestamp = number_value(
        value,
        &[
            "timestamp",
            "createTime",
            "CreateTime",
            "msgCreateTime",
            "time",
        ],
    )?;
    if !window.contains(timestamp) {
        return None;
    }

    let content = first_string(
        value,
        &["content", "text", "StrContent", "message", "msg", "summary"],
    )
    .unwrap_or_default();
    if content.trim().is_empty() {
        return None;
    }

    let chat_id = first_string(
        value,
        &["chatId", "chat_id", "talker", "Talker", "roomId", "room_id"],
    )
    .unwrap_or_else(|| fallback_chat_id.to_owned());
    if is_gh_account(&chat_id) {
        return None;
    }
    let chat_name = first_string(
        value,
        &["chatName", "chat_name", "roomName", "nickname", "nickName"],
    )
    .unwrap_or_else(|| fallback_chat_name.to_owned());
    let sender_id = first_string(
        value,
        &[
            "senderId",
            "sender_id",
            "sender",
            "fromUser",
            "from_user",
            "from",
            "senderUsername",
        ],
    )
    .unwrap_or_else(|| {
        if bool_value(value, &["isSelf", "is_self", "fromMe"]).unwrap_or(false) {
            "me".to_owned()
        } else {
            chat_id.clone()
        }
    });
    if is_gh_account(&sender_id) {
        return None;
    }
    let sender_name = first_string(
        value,
        &[
            "senderName",
            "sender_name",
            "senderNickname",
            "fromName",
            "displayName",
        ],
    )
    .unwrap_or_else(|| sender_id.clone());
    let raw_type = first_string(
        value,
        &["rawType", "raw_type", "type", "Type", "msgType", "MsgType"],
    );
    let msg_type = normalize_msg_type(raw_type.as_deref(), &content);
    let local_id = first_string(
        value,
        &[
            "localId", "local_id", "msgId", "msg_id", "MsgSvrID", "serverId", "id",
        ],
    );
    let content_hash = hash_text(&format!("{chat_id}|{timestamp}|{sender_id}|{content}"));
    let id = format!(
        "msg_{}",
        hash_text(&format!(
            "{}|{}|{content_hash}",
            profile.id,
            local_id.as_deref().unwrap_or("")
        ))
    );

    Some(DailyMessage {
        id,
        day: window.day.clone(),
        profile_id: profile.id.clone(),
        platform: profile.platform.clone(),
        chat_id,
        chat_name,
        is_group: fallback_is_group,
        sender_id,
        sender_name,
        timestamp,
        time_text: Local
            .timestamp_opt(timestamp, 0)
            .single()?
            .format("%H:%M")
            .to_string(),
        msg_type,
        content,
        raw_type,
        local_id,
        raw_json: serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned()),
        content_hash,
        partial: bool_value(
            value,
            &["partial", "contextIncomplete", "context_incomplete"],
        )
        .unwrap_or(false),
    })
}

fn normalize_text_message(
    profile: &ImProfile,
    line: &str,
    window: &MessageImportWindow,
    fallback_chat_id: &str,
    fallback_chat_name: &str,
    fallback_is_group: bool,
) -> Option<DailyMessage> {
    if is_gh_account(fallback_chat_id) {
        return None;
    }

    let (time_part, body) = line.strip_prefix('[')?.split_once("] ")?;
    let naive = NaiveDateTime::parse_from_str(time_part, "%Y-%m-%d %H:%M").ok()?;
    let timestamp = Local.from_local_datetime(&naive).single()?.timestamp();
    if !window.contains(timestamp) {
        return None;
    }

    let (sender_name, content) = body
        .split_once(": ")
        .map(|(sender, content)| (sender.trim(), content.trim()))
        .unwrap_or(("", body.trim()));
    if content.is_empty() {
        return None;
    }

    let sender_id = if sender_name.is_empty() {
        fallback_chat_id.to_owned()
    } else {
        sender_name.to_owned()
    };
    if is_gh_account(&sender_id) {
        return None;
    }
    let content_hash = hash_text(&format!(
        "{fallback_chat_id}|{timestamp}|{sender_id}|{content}"
    ));
    let id = format!(
        "msg_{}",
        hash_text(&format!(
            "{}|{}|{content_hash}",
            profile.id, fallback_chat_id
        ))
    );

    Some(DailyMessage {
        id,
        day: window.day.clone(),
        profile_id: profile.id.clone(),
        platform: profile.platform.clone(),
        chat_id: fallback_chat_id.to_owned(),
        chat_name: fallback_chat_name.to_owned(),
        is_group: fallback_is_group,
        sender_id,
        sender_name: if sender_name.is_empty() {
            fallback_chat_name.to_owned()
        } else {
            sender_name.to_owned()
        },
        timestamp,
        time_text: naive.format("%H:%M").to_string(),
        msg_type: normalize_msg_type(None, content),
        content: content.to_owned(),
        raw_type: None,
        local_id: None,
        raw_json: serde_json::to_string(line).unwrap_or_else(|_| "\"\"".to_owned()),
        content_hash,
        partial: false,
    })
}

pub(crate) fn value_array(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }
    for key in ["items", "messages", "sessions", "data", "rows", "results"] {
        if let Some(items) = value.get(key).and_then(|inner| inner.as_array()) {
            return items.iter().collect();
        }
    }
    Vec::new()
}

pub(crate) fn dedupe_sessions(sessions: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for session in sessions {
        let Some(chat_id) = first_string(
            &session,
            &["chatId", "chat_id", "username", "userName", "talker", "id"],
        ) else {
            continue;
        };
        if seen.insert(chat_id) {
            deduped.push(session);
        }
    }
    deduped
}

pub(crate) fn should_sync_session(value: &serde_json::Value) -> bool {
    let username = first_string(value, &["username", "userName", "chatId", "chat_id", "id"])
        .unwrap_or_default();
    !is_gh_account(&username) && !is_wechat_pseudo_session(&username)
}

fn is_gh_account(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("gh_")
}

fn is_wechat_pseudo_session(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "brandsessionholder" | "@placeholder_foldgroup"
    )
}

pub(crate) fn should_skip_chat_history(
    local_latest_by_chat: &HashMap<String, i64>,
    chat_id: &str,
    remote_latest_timestamp: Option<i64>,
) -> bool {
    let Some(remote_latest_timestamp) = remote_latest_timestamp else {
        return false;
    };
    let Some(local_latest_timestamp) = local_latest_by_chat.get(chat_id) else {
        return false;
    };
    *local_latest_timestamp >= remote_latest_timestamp
}

pub(crate) fn session_last_message_timestamp(value: &serde_json::Value) -> Option<i64> {
    number_value(
        value,
        &[
            "lastMessageTimestamp",
            "last_message_timestamp",
            "lastMessageTime",
            "last_message_time",
            "lastTimestamp",
            "last_timestamp",
            "lastTime",
            "last_time",
            "timestamp",
            "time",
        ],
    )
}

pub(crate) fn first_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = value.get(*key).and_then(|inner| inner.as_str()) {
            if !text.trim().is_empty() {
                return Some(text.to_owned());
            }
        }
        if let Some(number) = value.get(*key).and_then(|inner| inner.as_i64()) {
            return Some(number.to_string());
        }
    }
    None
}

pub(crate) fn bool_value(value: &serde_json::Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|inner| inner.as_bool()))
}

fn number_value(value: &serde_json::Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if let Some(number) = value.get(*key).and_then(|inner| inner.as_i64()) {
            return Some(if number > 10_000_000_000 {
                number / 1000
            } else {
                number
            });
        }
        if let Some(text) = value.get(*key).and_then(|inner| inner.as_str()) {
            if let Ok(number) = text.parse::<i64>() {
                return Some(if number > 10_000_000_000 {
                    number / 1000
                } else {
                    number
                });
            }
        }
    }
    None
}

fn normalize_msg_type(raw_type: Option<&str>, content: &str) -> String {
    let raw = raw_type.unwrap_or("").to_ascii_lowercase();
    let content_lower = content.to_ascii_lowercase();
    if raw.contains("image") || raw.contains("图片") || raw == "3" || content.contains("[图片]")
    {
        "image"
    } else if raw.contains("voice")
        || raw.contains("语音")
        || raw == "34"
        || content.contains("[语音]")
    {
        "voice"
    } else if raw.contains("video")
        || raw.contains("视频")
        || raw == "43"
        || raw == "62"
        || content.contains("[视频]")
    {
        "video"
    } else if raw.contains("emoji")
        || raw.contains("表情")
        || raw == "47"
        || content.contains("[表情]")
    {
        "emoji"
    } else if raw.contains("location")
        || raw.contains("位置")
        || raw == "48"
        || content.contains("[位置]")
    {
        "location"
    } else if raw.contains("link")
        || raw.contains("链接")
        || content.starts_with("<?xml")
        || content.contains("<appmsg")
    {
        "link"
    } else if raw.contains("file")
        || raw.contains("文件")
        || content_lower.ends_with(".pdf")
        || content_lower.ends_with(".doc")
        || content_lower.ends_with(".docx")
        || content_lower.ends_with(".xls")
        || content_lower.ends_with(".xlsx")
        || content_lower.ends_with(".ppt")
        || content_lower.ends_with(".pptx")
        || content_lower.ends_with(".zip")
        || content_lower.ends_with(".rar")
    {
        "file"
    } else {
        "text"
    }
    .to_owned()
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}
