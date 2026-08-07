use std::collections::HashMap;

use chrono::{Local, TimeZone};

use super::shared::*;

pub(in crate::bridge_runner) fn normalize_feishu_messages(
    raw: &serde_json::Value,
    args: &HashMap<String, String>,
) -> serde_json::Value {
    let chat_id = args.get("chat").cloned().unwrap_or_default();
    let chat_name = args
        .get("chat_name")
        .cloned()
        .unwrap_or_else(|| chat_id.clone());
    let is_group = args
        .get("chat_type")
        .map(|value| value == "2" || value == "group")
        .unwrap_or(true);
    let messages = first_json_array(raw, &["items", "messages", "data"]);
    serde_json::Value::Array(
        messages
            .into_iter()
            .enumerate()
            .filter_map(|(index, message)| {
                let timestamp = feishu_message_timestamp(message)?;
                let msg_type = json_string(message, &["msg_type", "msgType", "message_type", "type"]).unwrap_or_else(|| "text".to_owned());
                let sender_id = message
                    .get("sender")
                    .and_then(|sender| json_string(sender, &["id", "open_id", "user_id", "sender_id"]))
                    .or_else(|| json_string(message, &["sender_id", "senderId"]))
                    .unwrap_or_default();
                let sender_name = message
                    .get("sender")
                    .and_then(|sender| json_string(sender, &["name", "display_name", "displayName"]))
                    .unwrap_or_else(|| sender_id.clone());
                let content = feishu_message_content(message, &msg_type)?;
                Some(serde_json::json!({
                    "timestamp": timestamp,
                    "time": timestamp,
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "isGroup": is_group,
                    "senderId": sender_id,
                    "senderName": sender_name,
                    "content": content,
                    "msgType": msg_type,
                    "localId": json_string(message, &["message_id", "messageId", "id"]).unwrap_or_else(|| format!("{chat_id}_{timestamp}_{index}")),
                    "raw": message
                }))
            })
            .collect(),
    )
}

pub(in crate::bridge_runner) fn normalize_feishu_message_sessions(
    raw: &serde_json::Value,
) -> serde_json::Value {
    let messages = first_json_array(raw, &["items", "messages", "data"]);
    let mut latest_by_chat = HashMap::<String, serde_json::Value>::new();
    for message in messages {
        let Some(chat_id) = feishu_message_chat_id(message) else {
            continue;
        };
        let timestamp = feishu_message_timestamp(message).unwrap_or(0);
        let current_timestamp = latest_by_chat
            .get(&chat_id)
            .and_then(|session| session.get("lastMessageTimestamp"))
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        if timestamp < current_timestamp {
            continue;
        }
        let chat_name = feishu_message_chat_name(message).unwrap_or_else(|| chat_id.clone());
        let chat_type = json_string(message, &["chat_type", "chatType"]).unwrap_or_default();
        latest_by_chat.insert(
            chat_id.clone(),
            serde_json::json!({
                "chatId": chat_id,
                "chatName": chat_name,
                "isGroup": !chat_type.eq_ignore_ascii_case("p2p"),
                "chatType": chat_type,
                "lastMessageTimestamp": timestamp,
                "source": "message_search",
                "raw": message
            }),
        );
    }
    serde_json::Value::Array(latest_by_chat.into_values().collect())
}

pub(in crate::bridge_runner) fn normalize_feishu_chats(
    raw: &serde_json::Value,
) -> serde_json::Value {
    let chats = first_json_array(raw, &["items", "chats", "groups", "data", "list"]);
    serde_json::Value::Array(
        chats
            .into_iter()
            .filter_map(|chat| {
                let chat_id = json_string(
                    chat,
                    &["chat_id", "chatId", "open_chat_id", "openChatId", "id"],
                )?;
                let chat_name = json_string(chat, &["name", "chat_name", "chatName", "title"])
                    .unwrap_or_else(|| chat_id.clone());
                let chat_type = json_string(chat, &["chat_type", "chatType"]).unwrap_or_default();
                Some(serde_json::json!({
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "isGroup": !chat_type.eq_ignore_ascii_case("p2p"),
                    "chatType": chat_type,
                    "lastMessageTimestamp": feishu_chat_active_timestamp(chat).unwrap_or(0),
                    "source": "chat_list",
                    "raw": chat
                }))
            })
            .collect(),
    )
}

pub(in crate::bridge_runner) fn feishu_message_chat_id(
    message: &serde_json::Value,
) -> Option<String> {
    json_string(
        message,
        &["chat_id", "chatId", "container_id", "containerId"],
    )
    .or_else(|| {
        message
            .get("chat")
            .and_then(|chat| json_string(chat, &["chat_id", "chatId", "id"]))
    })
    .or_else(|| {
        message.get("context").and_then(|context| {
            json_string(
                context,
                &["chat_id", "chatId", "container_id", "containerId"],
            )
        })
    })
}

pub(in crate::bridge_runner) fn feishu_message_chat_name(
    message: &serde_json::Value,
) -> Option<String> {
    json_string(message, &["chat_name", "chatName"])
        .or_else(|| {
            message
                .get("chat")
                .and_then(|chat| json_string(chat, &["name", "chat_name", "chatName"]))
        })
        .or_else(|| {
            message
                .get("context")
                .and_then(|context| json_string(context, &["chat_name", "chatName", "name"]))
        })
}

fn feishu_chat_active_timestamp(chat: &serde_json::Value) -> Option<i64> {
    json_string(
        chat,
        &[
            "last_message_time",
            "lastMessageTime",
            "last_active_time",
            "lastActiveTime",
            "active_time",
            "activeTime",
            "update_time",
            "updateTime",
            "create_time",
            "createTime",
        ],
    )
    .and_then(|value| parse_feishu_timestamp(&value))
}

pub(in crate::bridge_runner) fn feishu_message_timestamp(
    message: &serde_json::Value,
) -> Option<i64> {
    if let Some(value) = json_string(
        message,
        &[
            "create_time",
            "createTime",
            "update_time",
            "timestamp",
            "time",
            "msgCreateTime",
        ],
    ) {
        return parse_feishu_timestamp(&value);
    }
    None
}

pub(in crate::bridge_runner) fn feishu_message_content(
    message: &serde_json::Value,
    msg_type: &str,
) -> Option<String> {
    let raw = json_string(message, &["content", "text", "body", "summary"])?;
    if msg_type == "text" || raw.trim_start().starts_with('{') {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
            return json_string(&json, &["text", "content", "title"]).or(Some(raw));
        }
    }
    Some(raw)
}

pub(in crate::bridge_runner) fn parse_feishu_timestamp(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if let Ok(number) = trimmed.parse::<i64>() {
        return Some(if number > 10_000_000_000 {
            number / 1000
        } else {
            number
        });
    }
    chrono::DateTime::parse_from_rfc3339(trimmed)
        .ok()
        .map(|datetime| datetime.timestamp())
        .or_else(|| parse_local_timestamp(trimmed))
}

pub(in crate::bridge_runner) fn feishu_time_arg(value: &str) -> Option<String> {
    parse_feishu_timestamp(value).map(|timestamp| {
        Local
            .timestamp_opt(timestamp, 0)
            .single()
            .unwrap_or_else(Local::now)
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string()
    })
}
