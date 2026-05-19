use crate::analysis::message_noise;

pub(crate) fn keyword_texts_from_message_content(content: &str) -> Vec<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if should_skip_keyword_message(trimmed, None) {
        return Vec::new();
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let mut texts = Vec::new();
        collect_message_content_strings(&value, None, &mut texts);
        let texts = texts
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !texts.is_empty() {
            return texts
                .into_iter()
                .flat_map(|text| sanitize_keyword_text(&text))
                .collect();
        }
    }

    let labeled_texts = extract_labeled_message_content(trimmed);
    if !labeled_texts.is_empty() {
        return labeled_texts
            .into_iter()
            .flat_map(|text| sanitize_keyword_text(&text))
            .collect();
    }

    sanitize_keyword_text(trimmed)
}

pub(crate) fn should_skip_keyword_message(content: &str, msg_type: Option<&str>) -> bool {
    let lower = content.to_ascii_lowercase();
    let trimmed = content.trim();
    let compact = trimmed
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let msg_type = msg_type.unwrap_or_default().trim().to_ascii_lowercase();
    is_system_keyword_message_type(&msg_type)
        || lower.contains("<sysmsg")
        || lower.contains("<revokemsg")
        || lower.contains("<appmsg")
        || lower.contains("<title>")
        || lower.contains("<videomsg")
        || lower.contains("<img ")
        || lower.contains("<emoji")
        || lower.contains("今日已签到")
        || lower.contains("连续签到")
        || lower.contains("积分商城")
        || lower.contains("点击领取")
        || lower.contains("点击进入")
        || lower.contains("点击查看您的答题记录")
        || lower.contains("积分商城")
        || looks_like_structural_payload(trimmed)
        || looks_like_pure_link_or_domain(trimmed)
        || compact.contains("撤回了一条消息")
        || compact.contains("修改群名")
        || message_noise::is_message_noise(Some(&msg_type), trimmed, true)
        || contains_wechat_touch_notice(trimmed)
        || contains_media_placeholder(trimmed)
        || contains_unsupported_client_notice(trimmed)
}

fn is_system_keyword_message_type(msg_type: &str) -> bool {
    matches!(
        msg_type,
        "sys" | "system" | "notice" | "notification" | "10000" | "10002"
    )
}

fn looks_like_structural_payload(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let trimmed = content.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return true;
    }
    lower.contains("<?xml")
        || lower.contains("<msg")
        || lower.contains("</msg>")
        || lower.contains("\"msg_type\"")
        || lower.contains("\"raw_json\"")
        || lower.contains("\"chat_id\"")
        || lower.contains("\"sender_id\"")
}

fn looks_like_pure_link_or_domain(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.split_whitespace().count() > 1 {
        return false;
    }
    starts_with_url_like_prefix(trimmed)
        || (trimmed.contains('.')
            && trimmed
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '/')))
}

fn contains_media_placeholder(content: &str) -> bool {
    let compact = content
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '[' | ']' | '【' | '】' | ':' | '：'))
        .collect::<String>();
    matches!(
        compact.as_str(),
        "图片"
            | "图片分享"
            | "分享图片"
            | "已分享图片"
            | "视频"
            | "视频分享"
            | "语音"
            | "文件"
            | "文件分享"
            | "链接"
            | "链接分享"
    )
}

pub(crate) fn contains_disallowed_media_content(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("<videomsg")
        || lower.contains("<img ")
        || lower.contains("<emoji")
        || lower.contains("<voip")
        || lower.contains("<appattach")
        || lower.contains("<recorditem")
        || contains_media_placeholder(content)
        || content.contains("[图片]")
        || content.contains("【图片】")
        || content.contains("[语音]")
        || content.contains("【语音】")
        || content.contains("[表情]")
        || content.contains("【表情】")
        || content.contains("[文件]")
        || content.contains("【文件】")
        || content.contains("[视频]")
        || content.contains("【视频】")
        || content.contains("[音视频通话]")
        || content.contains("【音视频通话】")
        || content.contains("[语音通话]")
        || content.contains("【语音通话】")
        || content.contains("[视频通话]")
        || content.contains("【视频通话】")
}

pub(crate) fn contains_unsupported_client_notice(content: &str) -> bool {
    message_noise::contains_client_compatibility_notice(content)
}

pub(crate) fn contains_group_membership_notice(content: &str) -> bool {
    message_noise::contains_group_membership_notice(content)
}

fn contains_wechat_touch_notice(content: &str) -> bool {
    let compact = content
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    compact.contains("拍一拍") || compact.contains("拍了拍")
}

fn sanitize_keyword_text(content: &str) -> Vec<String> {
    if should_skip_keyword_message(content, None) {
        return Vec::new();
    }

    let text = sanitize_readable_message_text(content);
    let text = text.trim();
    if text.is_empty() || should_skip_keyword_message(text, None) {
        Vec::new()
    } else {
        vec![text.to_owned()]
    }
}

fn strip_angle_bracket_markup(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_tag = false;
    for ch in content.chars() {
        match ch {
            '<' => {
                in_tag = true;
                result.push(' ');
            }
            '>' if in_tag => {
                in_tag = false;
                result.push(' ');
            }
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

fn strip_urls(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut index = 0usize;
    while index < content.len() {
        let rest = &content[index..];
        if starts_with_url_like_prefix(rest) {
            index += rest
                .char_indices()
                .find_map(|(offset, ch)| is_url_boundary(ch).then_some(offset))
                .unwrap_or(rest.len());
            result.push(' ');
            continue;
        }
        let Some(ch) = rest.chars().next() else {
            break;
        };
        result.push(ch);
        index += ch.len_utf8();
    }
    result
}

fn starts_with_url_like_prefix(value: &str) -> bool {
    if value.starts_with("http://") || value.starts_with("https://") || value.starts_with("www.") {
        return true;
    }
    let Some((scheme, _)) = value.split_once("://") else {
        return false;
    };
    (2..=24).contains(&scheme.len())
        && scheme
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn strip_ip_addresses(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut index = 0usize;
    while index < content.len() {
        let rest = &content[index..];
        let Some(ch) = rest.chars().next() else {
            break;
        };
        if ch.is_ascii_digit() {
            let candidate_len = rest
                .char_indices()
                .find_map(|(offset, ch)| (!ch.is_ascii_digit() && ch != '.').then_some(offset))
                .unwrap_or(rest.len());
            let candidate = &rest[..candidate_len];
            if looks_like_ip_address(candidate) {
                result.push(' ');
                index += candidate_len;
                continue;
            }
        }
        result.push(ch);
        index += ch.len_utf8();
    }
    result
}

fn looks_like_ip_address(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.chars().all(|ch| ch.is_ascii_digit())
                && part.parse::<u8>().is_ok()
        })
}

fn is_url_boundary(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '，' | '。' | '；' | '、' | '）' | ')' | ']' | '】' | '"' | '\''
        )
}

fn strip_mentions_and_reply_quotes(content: &str) -> String {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('@') {
                return None;
            }
            if let Some(index) = trimmed.find("↳ 回复") {
                return Some(trimmed[..index].trim().to_owned());
            }
            Some(trimmed.to_owned())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, Copy)]
struct StructuralLabel {
    key_start: usize,
    value_start: usize,
    is_message_content: bool,
}

fn collect_message_content_strings(
    value: &serde_json::Value,
    parent_key: Option<&str>,
    texts: &mut Vec<String>,
) {
    match value {
        serde_json::Value::String(text) => {
            if parent_key.is_some_and(is_message_content_key) {
                texts.push(text.to_owned());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_message_content_strings(item, parent_key, texts);
            }
        }
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                if is_metadata_content_key(key) {
                    continue;
                }
                collect_message_content_strings(child, Some(key), texts);
            }
        }
        _ => {}
    }
}

fn collect_link_app_strings(
    value: &serde_json::Value,
    parent_key: Option<&str>,
    texts: &mut Vec<String>,
) {
    match value {
        serde_json::Value::String(text) => {
            if parent_key.is_some_and(is_link_app_text_key) && !looks_like_url_or_ip(text) {
                texts.push(text.to_owned());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_link_app_strings(item, parent_key, texts);
            }
        }
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                if is_link_app_metadata_key(key) {
                    continue;
                }
                collect_link_app_strings(child, Some(key), texts);
            }
        }
        _ => {}
    }
}

fn is_link_app_text_key(key: &str) -> bool {
    let key = normalize_structural_key(key);
    matches!(
        key.as_str(),
        "title" | "des" | "desc" | "description" | "digest" | "summary"
    )
}

fn is_link_app_metadata_key(key: &str) -> bool {
    let key = normalize_structural_key(key);
    matches!(
        key.as_str(),
        "url"
            | "link"
            | "href"
            | "host"
            | "ip"
            | "appid"
            | "appname"
            | "thumburl"
            | "coverurl"
            | "imageurl"
            | "iconurl"
            | "sourceurl"
            | "pagepath"
            | "username"
    )
}

fn looks_like_url_or_ip(value: &str) -> bool {
    let trimmed = value.trim();
    starts_with_url_like_prefix(trimmed) || looks_like_ip_address(trimmed)
}

fn extract_xml_tag_values(content: &str, tag: &str) -> Vec<String> {
    let lower = content.to_ascii_lowercase();
    let open = format!("<{}>", tag.to_ascii_lowercase());
    let close = format!("</{}>", tag.to_ascii_lowercase());
    let mut values = Vec::new();
    let mut search_start = 0usize;
    while let Some(open_offset) = lower[search_start..].find(&open) {
        let value_start = search_start + open_offset + open.len();
        let Some(close_offset) = lower[value_start..].find(&close) else {
            break;
        };
        let value_end = value_start + close_offset;
        let value = content[value_start..value_end].trim();
        if !value.is_empty() && !looks_like_url_or_ip(value) {
            values.push(value.to_owned());
        }
        search_start = value_end + close.len();
    }
    values
}

fn extract_labeled_message_content(content: &str) -> Vec<String> {
    let labels = structural_labels(content);
    let mut texts = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        if !label.is_message_content {
            continue;
        }
        let value_end = labels
            .iter()
            .skip(index + 1)
            .map(|next| next.key_start)
            .find(|next_start| *next_start > label.value_start)
            .unwrap_or(content.len());
        let text = trim_structural_value(&content[label.value_start..value_end]);
        if !text.is_empty() {
            texts.push(text);
        }
    }
    texts
}

fn structural_labels(content: &str) -> Vec<StructuralLabel> {
    let mut labels = Vec::new();
    for (separator_index, separator) in content.char_indices() {
        if !matches!(separator, ':' | '：') {
            continue;
        }
        let Some((key_start, key)) = structural_key_before(content, separator_index) else {
            continue;
        };
        let is_message_content = is_message_content_key(key);
        if !is_message_content && !is_metadata_content_key(key) {
            continue;
        }
        labels.push(StructuralLabel {
            key_start,
            value_start: separator_index + separator.len_utf8(),
            is_message_content,
        });
    }
    labels
}

fn structural_key_before(content: &str, separator_index: usize) -> Option<(usize, &str)> {
    let mut key_end = separator_index;
    while key_end > 0 {
        let ch = content[..key_end].chars().next_back()?;
        if ch.is_whitespace() || matches!(ch, '"' | '\'' | '`') {
            key_end -= ch.len_utf8();
        } else {
            break;
        }
    }

    let mut key_start = key_end;
    for (index, ch) in content[..key_end].char_indices().rev() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
            key_start = index;
        } else {
            break;
        }
    }
    if key_start == key_end {
        return None;
    }
    Some((key_start, &content[key_start..key_end]))
}

fn trim_structural_value(value: &str) -> String {
    value
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\''
                        | '`'
                        | ','
                        | '，'
                        | ';'
                        | '；'
                        | '{'
                        | '}'
                        | '['
                        | ']'
                        | '('
                        | ')'
                        | '（'
                        | '）'
                )
        })
        .to_owned()
}

fn is_message_content_key(key: &str) -> bool {
    let key = normalize_structural_key(key);
    matches!(
        key.as_str(),
        "content" | "text" | "message" | "msg" | "body"
    )
}

fn is_metadata_content_key(key: &str) -> bool {
    let key = normalize_structural_key(key);
    matches!(
        key.as_str(),
        "chat"
            | "chatid"
            | "chatname"
            | "chatroom"
            | "group"
            | "groupid"
            | "groupname"
            | "sender"
            | "senderid"
            | "sendername"
            | "user"
            | "userid"
            | "username"
            | "nickname"
            | "displayname"
            | "profile"
            | "profileid"
            | "platform"
            | "type"
            | "msgtype"
            | "rawtype"
            | "rawjson"
            | "timestamp"
            | "time"
            | "timetext"
            | "localid"
            | "contenthash"
    )
}

fn normalize_structural_key(key: &str) -> String {
    key.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn strip_structural_field_labels(content: &str) -> String {
    content
        .split_whitespace()
        .filter(|part| {
            let key = part
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | '-'));
            !is_metadata_content_key(key) && !is_message_content_key(key)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn clean_link_or_app_message_text(content: &str) -> String {
    let mut texts = Vec::<String>::new();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
        collect_link_app_strings(&value, None, &mut texts);
    }
    for tag in ["title", "des", "desc", "description", "digest", "summary"] {
        texts.extend(extract_xml_tag_values(content, tag));
    }
    if texts.is_empty() {
        texts.extend(extract_labeled_message_content(content));
    }
    if texts.is_empty() {
        texts.push(strip_angle_bracket_markup(content));
    }

    texts
        .into_iter()
        .map(|text| sanitize_readable_message_text(&text))
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn sanitize_readable_message_text(content: &str) -> String {
    let without_xml = strip_angle_bracket_markup(content);
    let without_urls = strip_urls(&without_xml);
    let without_ips = strip_ip_addresses(&without_urls);
    let without_mentions = strip_mentions_and_reply_quotes(&without_ips);
    strip_structural_field_labels(&without_mentions)
}

pub(crate) fn clean_plain_message_text(content: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
        let mut texts = Vec::new();
        collect_message_content_strings(&value, None, &mut texts);
        let cleaned = texts
            .into_iter()
            .map(|text| sanitize_readable_message_text(&text))
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>();
        if !cleaned.is_empty() {
            return cleaned.join(" ");
        }
    }

    let labeled_texts = extract_labeled_message_content(content);
    if !labeled_texts.is_empty() {
        let cleaned = labeled_texts
            .into_iter()
            .map(|text| sanitize_readable_message_text(&text))
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>();
        if !cleaned.is_empty() {
            return cleaned.join(" ");
        }
    }

    sanitize_readable_message_text(content)
}
