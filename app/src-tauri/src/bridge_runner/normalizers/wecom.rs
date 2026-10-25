use std::collections::HashMap;

use super::shared::*;

pub(in crate::bridge_runner) fn normalize_wecom_contacts(
    raw: &serde_json::Value,
) -> serde_json::Value {
    let Some(users) = raw
        .get("users")
        .or_else(|| raw.get("userlist"))
        .and_then(|value| value.as_array())
    else {
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
                let msg_type = json_string(message, &["msg_type", "msgtype", "msgType"]).unwrap_or_else(|| "text".to_owned());
                let sender_id = json_string(message, &["userid", "userId", "sender"]).unwrap_or_else(|| chat_id.clone());
                let content = wecom_message_content(message, &msg_type)?;
                Some(serde_json::json!({
                    "timestamp": timestamp,
                    "time": timestamp,
                    "chatId": chat_id,
                    "chatName": chat_name,
                    "isGroup": is_group,
                    "senderId": sender_id,
                    "senderName": json_string(message, &["user_name", "senderName"])
                        .filter(|name| name != &sender_id)
                        .or_else(|| raw.get("sender_names").and_then(|names| names.get(&sender_id)).and_then(|name| name.as_str()).filter(|name| !name.trim().is_empty()).map(str::to_owned))
                        .unwrap_or_else(|| sender_id.clone()),
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

#[cfg(test)]
mod current_contract_tests {
    use super::*;
    #[test]
    fn current_contacts_and_message_type_fields_are_understood() {
        assert_eq!(
            normalize_wecom_contacts(
                &serde_json::json!({"users":[{"userid":"u1","name":"Alice"}]})
            )[0]["chatId"],
            "u1"
        );
        let result = normalize_wecom_messages(
            &serde_json::json!({"messages":[{"send_time":"2026-10-24 12:00:00","msg_type":"image","image":{"media_id":"fixture"},"userid":"u1","user_name":"Alice"}]}),
            &HashMap::new(),
            &serde_json::json!({}),
        );
        assert_eq!(result[0]["msgType"], "image");
        assert_eq!(result[0]["senderName"], "Alice");
    }
}

#[cfg(test)]
mod sender_name_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn name_priority_is_message_then_directory_then_exact_sender_id() {
        let raw = json!({"sender_names":{"u1":"通讯录姓名"},"messages":[
            {"userid":"u1","user_name":"消息姓名","send_time":"2026-10-24 12:00:00","text":{"content":"one"}},
            {"userid":"u1","send_time":"2026-10-24 12:00:01","text":{"content":"two"}},
            {"userid":"unknown","send_time":"2026-10-24 12:00:02","text":{"content":"three"}},
            {"userid":"u1","senderName":"u1","send_time":"2026-10-24 12:00:03","text":{"content":"four"}}
        ]});
        let messages = normalize_wecom_messages(
            &raw,
            &HashMap::from([
                ("chat".into(), "group-id".into()),
                ("chat_type".into(), "2".into()),
            ]),
            &json!({}),
        );
        assert_eq!(messages[0]["senderName"], "消息姓名");
        assert_eq!(messages[1]["senderName"], "通讯录姓名");
        assert_eq!(messages[2]["senderName"], "unknown");
        assert_eq!(messages[3]["senderName"], "通讯录姓名");
        assert_eq!(messages[1]["senderId"], "u1");
    }
}
