use std::collections::HashMap;

use chrono::{Local, TimeZone};

use super::feishu::parse_feishu_timestamp;
use super::shared::*;

pub(in crate::bridge_runner) fn normalize_dingtalk_chats(
    raw: &serde_json::Value,
) -> serde_json::Value {
    let chats = first_json_array(
        raw,
        &["items", "conversations", "value", "list", "data", "result"],
    );
    serde_json::Value::Array(
        chats
            .into_iter()
            .filter_map(|chat| {
                let chat_id = json_string(
                    chat,
                    &[
                        "openConversationId",
                        "openconversation_id",
                        "conversationId",
                        "conversation_id",
                        "chatId",
                        "chat_id",
                        "id",
                    ],
                )?;
                let chat_name = json_string(chat, &["title", "name", "chatName", "chat_name", "conversationName"])
                    .unwrap_or_else(|| chat_id.clone());
                Some(serde_json::json!({
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "isGroup": true,
                    "chatType": json_string(chat, &["type", "conversationType", "conversation_type"]).unwrap_or_else(|| "group".to_owned()),
                    "lastMessageTimestamp": dingtalk_json_timestamp(chat, &["lastMessageTime", "last_message_time", "time", "updatedAt"]),
                    "raw": chat
                }))
            })
            .collect(),
    )
}

pub(in crate::bridge_runner) fn normalize_dingtalk_messages(
    raw: &serde_json::Value,
    args: &HashMap<String, String>,
) -> serde_json::Value {
    let chat_id = args.get("chat").cloned().unwrap_or_default();
    let chat_name = args
        .get("chat_name")
        .cloned()
        .unwrap_or_else(|| chat_id.clone());
    let messages = first_json_array(
        raw,
        &[
            "items",
            "messages",
            "messageList",
            "conversationMessagesList",
            "list",
            "value",
            "data",
            "result",
        ],
    );
    serde_json::Value::Array(
        messages
            .into_iter()
            .enumerate()
            .filter_map(|(index, message)| {
                let timestamp = dingtalk_json_timestamp(
                    message,
                    &["timestamp", "createTime", "createdAt", "sendTime", "msgCreateTime", "time"],
                )?;
                let msg_type = json_string(message, &["msgType", "msg_type", "messageType", "type"]).unwrap_or_else(|| "text".to_owned());
                let sender_id = json_string(
                    message,
                    &["senderId", "sender_id", "senderStaffId", "sender", "fromUserId", "from"],
                )
                .unwrap_or_default();
                let sender_name = json_string(message, &["senderName", "sender_name", "fromName", "displayName"])
                    .unwrap_or_else(|| sender_id.clone());
                let content = dingtalk_message_content(message, &msg_type)?;
                let normalized_chat_id = json_string(message, &["openConversationId", "conversationId", "chatId", "chat_id"])
                    .unwrap_or_else(|| chat_id.clone());
                let normalized_chat_name = json_string(message, &["conversationTitle", "chatName", "chat_name"])
                    .unwrap_or_else(|| chat_name.clone());
                Some(serde_json::json!({
                    "timestamp": timestamp,
                    "time": timestamp,
                    "chatId": normalized_chat_id,
                    "chatName": normalized_chat_name,
                    "isGroup": dingtalk_is_group_message(message),
                    "senderId": sender_id,
                    "senderName": sender_name,
                    "content": content,
                    "msgType": msg_type,
                    "localId": json_string(message, &["openMessageId", "msgId", "messageId", "id"]).unwrap_or_else(|| format!("{chat_id}_{timestamp}_{index}")),
                    "raw": message
                }))
            })
            .collect(),
    )
}

pub(in crate::bridge_runner) fn dingtalk_is_group_message(message: &serde_json::Value) -> bool {
    if let Some(value) = message
        .get("isGroup")
        .or_else(|| message.get("is_group"))
        .and_then(|value| value.as_bool())
    {
        return value;
    }
    let kind = json_string(
        message,
        &[
            "conversationType",
            "conversation_type",
            "chatType",
            "chat_type",
            "type",
        ],
    )
    .unwrap_or_default()
    .to_ascii_lowercase();
    !(kind.contains("single") || kind.contains("private") || kind.contains("p2p") || kind == "1")
}

pub(in crate::bridge_runner) fn dingtalk_message_content(
    message: &serde_json::Value,
    msg_type: &str,
) -> Option<String> {
    if let Some(text) = json_string(message, &["content", "text", "summary", "body"]) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            return json_string(&json, &["text", "content", "title"]).or(Some(text));
        }
        return Some(text);
    }
    message.get(msg_type).and_then(|value| {
        json_string(value, &["text", "content", "title", "name"])
            .or_else(|| Some(format!("[{msg_type}]")))
    })
}

pub(in crate::bridge_runner) fn dingtalk_json_timestamp(
    value: &serde_json::Value,
    keys: &[&str],
) -> Option<i64> {
    let raw = json_string(value, keys)?;
    parse_dingtalk_timestamp(&raw)
}

pub(in crate::bridge_runner) fn parse_dingtalk_timestamp(value: &str) -> Option<i64> {
    parse_feishu_timestamp(value)
}

pub(in crate::bridge_runner) fn dingtalk_time_arg(value: &str) -> Option<String> {
    parse_dingtalk_timestamp(value).map(|timestamp| {
        Local
            .timestamp_opt(timestamp, 0)
            .single()
            .unwrap_or_else(Local::now)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    })
}
