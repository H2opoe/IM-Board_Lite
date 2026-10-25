// Raw type metadata survives old imports even when msg_type was reduced to text/file.
// Read only known producer fields; message content is never interpreted as metadata.
pub(crate) fn resolve_stored_message_type(
    platform: &str,
    msg_type: &str,
    raw_type: Option<&str>,
    raw_json: Option<&str>,
    _content: &str,
) -> String {
    if let Some(raw_type) = raw_type.filter(|kind| !kind.trim().is_empty()) {
        return canonical_system_type(platform, raw_type)
            .unwrap_or(msg_type)
            .to_owned();
    }
    if let Some(value) =
        raw_json.and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
    {
        let raw_type =
            producer_raw_type(&value).or_else(|| value.get("raw").and_then(producer_raw_type));
        if let Some(raw_type) = raw_type {
            return canonical_system_type(platform, &raw_type)
                .unwrap_or(msg_type)
                .to_owned();
        }
    }
    canonical_system_type(platform, msg_type)
        .unwrap_or(msg_type)
        .to_owned()
}

fn producer_raw_type(value: &serde_json::Value) -> Option<String> {
    [
        "rawType",
        "raw_type",
        "msgType",
        "msg_type",
        "MsgType",
        "message_type",
        "messageType",
        "msgtype",
        "type",
        "Type",
    ]
    .iter()
    .find_map(|key| {
        let kind = value.get(*key)?;
        kind.as_str()
            .filter(|kind| !kind.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| kind.as_i64().map(|number| number.to_string()))
    })
}

pub(crate) fn canonical_system_type(_platform: &str, raw_type: &str) -> Option<&'static str> {
    let kind = raw_type.trim().to_ascii_lowercase();
    if matches!(kind.as_str(), "sys" | "system" | "notice" | "notification") {
        return Some("system");
    }

    None
}

pub(crate) fn is_system_message_type(msg_type: &str) -> bool {
    matches!(
        msg_type.trim().to_ascii_lowercase().as_str(),
        "sys" | "system" | "notice" | "notification"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_type_boundary_raw_metadata_has_explicit_priority() {
        assert_eq!(
            resolve_stored_message_type(
                "feishu",
                "file",
                Some("system"),
                Some(r#"{"msgType":"text"}"#),
                "invoice.pdf"
            ),
            "system"
        );
        assert_eq!(
            resolve_stored_message_type(
                "feishu",
                "text",
                Some("text"),
                Some(r#"{"raw":{"msg_type":"system"}}"#),
                "群公告"
            ),
            "text"
        );
        assert_eq!(
            resolve_stored_message_type(
                "wecom",
                "text",
                None,
                Some(r#"{"raw":{"msgtype":"system"}}"#),
                "后台配置已更新"
            ),
            "system"
        );
        assert_eq!(
            resolve_stored_message_type(
                "dingtalk",
                "text",
                None,
                Some(r#"{"content":{"msg_type":"system"}}"#),
                "群公告"
            ),
            "text"
        );
        assert_eq!(
            resolve_stored_message_type(
                "feishu",
                "text",
                None,
                Some("invalid JSON"),
                "拍一拍功能需要交付"
            ),
            "text"
        );
        assert_eq!(
            resolve_stored_message_type(
                "feishu",
                "text",
                None,
                Some(r#""[系统] 讨论群公告""#),
                "[系统] 讨论群公告"
            ),
            "text"
        );
        for platform in ["feishu", "dingtalk", "wecom", "vendor_extension"] {
            for kind in ["12345", "67890"] {
                assert_eq!(
                    resolve_stored_message_type(
                        platform,
                        "text",
                        Some(kind),
                        None,
                        "会议排期需要确认"
                    ),
                    "text"
                );
            }
        }
    }
}
