use std::sync::LazyLock;

use regex::{Captures, Regex};

const SECRET_MARKERS: &[&str] = &[
    "enc_key",
    "api_key",
    "apikey",
    "authorization",
    "access_token",
    "refresh_token",
    "token",
    "secret",
    "password",
];

static BEARER_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]+").expect("valid bearer redaction regex")
});
static PREFIXED_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:sk|key|token)-[A-Za-z0-9._-]{8,}").expect("valid token redaction regex")
});

pub fn sanitize_log(input: &str) -> String {
    let mut output = BEARER_SECRET
        .replace_all(input, |captures: &Captures<'_>| {
            format!("{}***", &captures[1])
        })
        .into_owned();
    output = PREFIXED_SECRET.replace_all(&output, "***").into_owned();
    for marker in SECRET_MARKERS {
        output = mask_key_value(&output, marker);
    }
    output
}

pub fn redact_json_value(value: serde_json::Value, string_limit: usize) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .into_iter()
                .map(|item| redact_json_value(item, string_limit))
                .collect(),
        ),
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    if is_sensitive_key(&key) {
                        (key, serde_json::json!("***"))
                    } else {
                        (key, redact_json_value(value, string_limit))
                    }
                })
                .collect(),
        ),
        serde_json::Value::String(text) => {
            serde_json::Value::String(truncate_sanitized(&text, string_limit))
        }
        other => other,
    }
}

#[allow(dead_code)]
pub fn truncate_sanitized(value: &str, limit: usize) -> String {
    let sanitized = sanitize_log(value);
    if sanitized.chars().count() <= limit {
        return sanitized;
    }
    let mut output = sanitized.chars().take(limit).collect::<String>();
    output.push_str("...");
    output
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "authorization",
        "password",
        "cookie",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn mask_key_value(input: &str, key: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let key = key.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(relative) = lower[cursor..].find(&key) {
        let start = cursor + relative;
        let key_end = start + key.len();
        let before_ok = start == 0 || !lower.as_bytes()[start - 1].is_ascii_alphanumeric();
        let after_ok = key_end == lower.len() || !lower.as_bytes()[key_end].is_ascii_alphanumeric();
        if !before_ok || !after_ok {
            output.push_str(&input[cursor..key_end]);
            cursor = key_end;
            continue;
        }

        let suffix = &input[key_end..];
        let Some(separator_offset) = suffix.find([':', '=']) else {
            output.push_str(&input[cursor..key_end]);
            cursor = key_end;
            continue;
        };
        if separator_offset > 8 || suffix[..separator_offset].contains('\n') {
            output.push_str(&input[cursor..key_end]);
            cursor = key_end;
            continue;
        }

        let separator = key_end + separator_offset;
        let bytes = input.as_bytes();
        let mut value_start = separator + 1;
        while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        let quote = bytes
            .get(value_start)
            .copied()
            .filter(|byte| *byte == b'\'' || *byte == b'"');
        if quote.is_some() {
            value_start += 1;
        }
        let mut value_end = value_start;
        while value_end < bytes.len() {
            let byte = bytes[value_end];
            if quote.is_some_and(|quote| byte == quote)
                || (quote.is_none() && (byte.is_ascii_whitespace() || byte == b',' || byte == b';'))
            {
                break;
            }
            value_end += 1;
        }
        if value_end == value_start {
            output.push_str(&input[cursor..key_end]);
            cursor = key_end;
            continue;
        }
        output.push_str(&input[cursor..value_start]);
        output.push_str("***");
        cursor = value_end;
    }
    output.push_str(&input[cursor..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_structured_and_inline_secrets_without_erasing_context() {
        let input = r#"request failed: {\"apiKey\":\"sk-sensitive123\",\"status\":401} authorization=Bearer abc.def.ghi"#;
        let output = sanitize_log(input);
        assert!(!output.contains("sensitive123"));
        assert!(!output.contains("abc.def.ghi"));
        assert!(output.contains("request failed"));
        assert!(output.contains("status"));
    }

    #[test]
    fn redacts_bare_provider_keys() {
        assert_eq!(
            sanitize_log("upstream rejected sk-1234567890abcdef"),
            "upstream rejected ***"
        );
    }

    #[test]
    fn redacts_nested_json_and_truncates_sanitized_strings() {
        let value = serde_json::json!({
            "request": {
                "accessToken": "token-sensitive123",
                "message": "authorization=Bearer abc.def.ghi; a long diagnostic message"
            }
        });
        let output = redact_json_value(value, 32);
        assert_eq!(output["request"]["accessToken"], "***");
        let message = output["request"]["message"].as_str().unwrap();
        assert!(!message.contains("abc.def.ghi"));
        assert!(message.chars().count() <= 35);
    }
}
