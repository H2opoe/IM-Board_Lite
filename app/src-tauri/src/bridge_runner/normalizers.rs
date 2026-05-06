use std::collections::HashMap;

use chrono::{Local, TimeZone};

pub(super) fn unwrap_wecom_cli_payload(raw: serde_json::Value) -> serde_json::Value {
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

pub(super) fn normalize_feishu_chats(raw: &serde_json::Value) -> serde_json::Value {
    let chats = first_json_array(raw, &["items", "chats", "data"]);
    serde_json::Value::Array(
        chats
            .into_iter()
            .filter_map(|chat| {
                let chat_id = json_string(
                    chat,
                    &["chat_id", "chatId", "chat_id_v2", "id", "open_chat_id"],
                )?;
                let chat_name =
                    json_string(chat, &["name", "chat_name", "chatName", "description"])
                        .unwrap_or_else(|| chat_id.clone());
                let chat_type =
                    json_string(chat, &["chat_type", "chatType", "type"]).unwrap_or_default();
                Some(serde_json::json!({
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "isGroup": !chat_type.eq_ignore_ascii_case("p2p"),
                    "chatType": chat_type,
                    "raw": chat
                }))
            })
            .collect(),
    )
}

pub(super) fn normalize_feishu_messages(
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

pub(super) fn normalize_feishu_message_sessions(raw: &serde_json::Value) -> serde_json::Value {
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

pub(super) fn normalize_dingtalk_chats(raw: &serde_json::Value) -> serde_json::Value {
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

pub(super) fn normalize_dingtalk_messages(
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

pub(super) fn dingtalk_is_group_message(message: &serde_json::Value) -> bool {
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

pub(super) fn dingtalk_message_content(
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

pub(super) fn dingtalk_json_timestamp(value: &serde_json::Value, keys: &[&str]) -> Option<i64> {
    let raw = json_string(value, keys)?;
    parse_dingtalk_timestamp(&raw)
}

pub(super) fn feishu_message_chat_id(message: &serde_json::Value) -> Option<String> {
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

pub(super) fn feishu_message_chat_name(message: &serde_json::Value) -> Option<String> {
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

pub(super) fn first_json_array<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Vec<&'a serde_json::Value> {
    if let Some(array) = value.as_array() {
        return array.iter().collect();
    }
    for key in keys {
        if let Some(array) = value.get(*key).and_then(|inner| inner.as_array()) {
            return array.iter().collect();
        }
        if let Some(array) = value
            .get("data")
            .and_then(|data| data.get(*key))
            .and_then(|inner| inner.as_array())
        {
            return array.iter().collect();
        }
        if let Some(array) = value
            .get("result")
            .and_then(|result| result.get(*key))
            .and_then(|inner| inner.as_array())
        {
            return array.iter().collect();
        }
    }
    Vec::new()
}

pub(super) fn normalize_wecom_chats(
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

pub(super) fn normalize_wecom_contacts(raw: &serde_json::Value) -> serde_json::Value {
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

pub(super) fn normalize_wecom_messages(
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

pub(super) fn wecom_message_content(message: &serde_json::Value, msg_type: &str) -> Option<String> {
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

pub(super) fn feishu_message_timestamp(message: &serde_json::Value) -> Option<i64> {
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

pub(super) fn feishu_message_content(
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

pub(super) fn json_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
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

pub(super) fn parse_feishu_timestamp(value: &str) -> Option<i64> {
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

pub(super) fn feishu_time_arg(value: &str) -> Option<String> {
    parse_feishu_timestamp(value).map(|timestamp| {
        Local
            .timestamp_opt(timestamp, 0)
            .single()
            .unwrap_or_else(Local::now)
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string()
    })
}

pub(super) fn parse_dingtalk_timestamp(value: &str) -> Option<i64> {
    parse_feishu_timestamp(value)
}

pub(super) fn dingtalk_time_arg(value: &str) -> Option<String> {
    parse_dingtalk_timestamp(value).map(|timestamp| {
        Local
            .timestamp_opt(timestamp, 0)
            .single()
            .unwrap_or_else(Local::now)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    })
}

pub(super) fn is_wecom_group_chat(
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

pub(super) fn parse_local_timestamp(value: &str) -> Option<i64> {
    ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
        .into_iter()
        .find_map(|format| {
            chrono::NaiveDateTime::parse_from_str(value, format)
                .ok()
                .and_then(|naive| naive.and_local_timezone(Local).single())
                .map(|datetime| datetime.timestamp())
        })
}

pub(super) fn today_start_text() -> String {
    Local::now().format("%Y-%m-%d 00:00:00").to_string()
}

pub(super) fn now_text() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub(super) fn remove_empty_json_fields(value: serde_json::Value) -> serde_json::Value {
    let Some(object) = value.as_object() else {
        return value;
    };
    serde_json::Value::Object(
        object
            .iter()
            .filter(|(_, value)| !value.as_str().is_some_and(str::is_empty))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}
