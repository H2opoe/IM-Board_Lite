use std::collections::HashMap;

use super::shared::*;

pub(in crate::bridge_runner) fn unwrap_wecom_cli_payload(
    raw: serde_json::Value,
) -> serde_json::Value {
    if let Some(error) = raw.get("error") {
        return serde_json::json!({
            "errcode": -1,
            "errmsg": error
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("企业微信官方CLI返回错误。")
        });
    }

    let Some(result) = raw.get("result") else {
        return raw;
    };
    let Some(content) = result.get("content").and_then(|value| value.as_array()) else {
        return raw;
    };
    let Some(text) = content
        .iter()
        .find_map(|item| item.get("text").and_then(|value| value.as_str()))
    else {
        return raw;
    };
    serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({ "text": text }))
}

pub(in crate::bridge_runner) fn normalize_wecom_chats(
    raw: &serde_json::Value,
    config: &serde_json::Value,
) -> serde_json::Value {
    let Some(chats) = raw.get("chats").and_then(|value| value.as_array()) else {
        return serde_json::Value::Array(Vec::new());
    };
    serde_json::Value::Array(
        chats
            .iter()
            .filter_map(|chat| {
                let chat_id = json_string(chat, &["chat_id", "chatid", "chatId", "id"])?;
                let chat_name = json_string(chat, &["chat_name", "chatName", "name"]).unwrap_or_else(|| chat_id.clone());
                Some(serde_json::json!({
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "lastMessageTime": json_string(chat, &["last_msg_time", "lastMessageTime"]),
                    "lastMessageTimestamp": json_string(chat, &["last_msg_time", "lastMessageTime"]).and_then(|value| parse_local_timestamp(&value)),
                    "msgCount": chat.get("msg_count").or_else(|| chat.get("msgCount")).cloned().unwrap_or(serde_json::Value::Number(0.into())),
                    "isGroup": is_wecom_group_chat(&chat_id, &chat_name, config),
                    "raw": chat
                }))
            })
            .collect(),
    )
}

pub(in crate::bridge_runner) fn normalize_wecom_contacts(
    raw: &serde_json::Value,
) -> serde_json::Value {
    let Some(users) = raw.get("userlist").and_then(|value| value.as_array()) else {
        return serde_json::Value::Array(Vec::new());
    };
    serde_json::Value::Array(
        users
            .iter()
            .filter_map(|user| {
                let user_id = json_string(user, &["userid", "userId", "id"])?;
                let name = json_string(user, &["name", "displayName", "alias"])
                    .unwrap_or_else(|| user_id.clone());
                Some(serde_json::json!({
                    "chatId": user_id,
                    "chatName": name,
                    "isGroup": false,
                    "chatType": 1,
                    "source": "contact",
                    "raw": user
                }))
            })
            .collect(),
    )
}

pub(in crate::bridge_runner) fn normalize_wecom_messages(
    raw: &serde_json::Value,
    args: &HashMap<String, String>,
    config: &serde_json::Value,
) -> serde_json::Value {
    let chat_id = args.get("chat").cloned().unwrap_or_default();
    let chat_name = args
        .get("chat_name")
        .cloned()
        .unwrap_or_else(|| chat_id.clone());
    let is_group = args
        .get("chat_type")
        .map(|value| value == "2" || value == "group")
        .unwrap_or_else(|| is_wecom_group_chat(&chat_id, &chat_name, config));
    let Some(messages) = raw.get("messages").and_then(|value| value.as_array()) else {
        return serde_json::Value::Array(Vec::new());
    };
    serde_json::Value::Array(
        messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                let send_time = json_string(message, &["send_time", "sendTime"])?;
                let timestamp = parse_local_timestamp(&send_time)?;
                let msg_type = json_string(message, &["msgtype", "msgType"]).unwrap_or_else(|| "text".to_owned());
                let sender_id = json_string(message, &["userid", "userId", "sender"]).unwrap_or_else(|| chat_id.clone());
                let content = wecom_message_content(message, &msg_type)?;
                Some(serde_json::json!({
                    "timestamp": timestamp,
                    "time": timestamp,
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "isGroup": is_group,
                    "senderId": sender_id,
                    "senderName": sender_id,
                    "content": content,
                    "msgType": msg_type,
                    "localId": json_string(message, &["msgid", "msg_id", "id"]).unwrap_or_else(|| format!("{chat_id}_{timestamp}_{index}")),
                    "raw": message
                }))
            })
            .collect(),
    )
}

pub(in crate::bridge_runner) fn wecom_message_content(
    message: &serde_json::Value,
    msg_type: &str,
) -> Option<String> {
    if msg_type == "text" {
        return message
            .get("text")
            .and_then(|value| value.get("content"))
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .or_else(|| json_string(message, &["content", "summary"]));
    }
    if let Some(body) = message.get(msg_type).and_then(|value| value.as_object()) {
        let name = body
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        return Some(if name.is_empty() {
            format!("[{msg_type}]")
        } else {
            format!("[{msg_type}] {name}")
        });
    }
    Some(format!("[{msg_type}]"))
}

pub(in crate::bridge_runner) fn is_wecom_group_chat(
    chat_id: &str,
    chat_name: &str,
    config: &serde_json::Value,
) -> bool {
    if let Some(value) = config
        .get("chatTypeOverrides")
        .and_then(|overrides| overrides.get(chat_id))
    {
        if value == 2 || value == "2" || value == "group" {
            return true;
        }
        if value == 1 || value == "1" || value == "direct" {
            return false;
        }
    }
    let lowered = chat_id.to_ascii_lowercase();
    lowered.starts_with("wr") || lowered.starts_with("group") || chat_name.contains('群')
}
