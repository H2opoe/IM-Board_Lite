use chrono::Local;

pub(in crate::bridge_runner) fn first_json_array<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Vec<&'a serde_json::Value> {
    super::super::read_result::collection_with_keys(value, keys)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

pub(in crate::bridge_runner) fn json_string(
    value: &serde_json::Value,
    keys: &[&str],
) -> Option<String> {
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

pub(in crate::bridge_runner) fn parse_local_timestamp(value: &str) -> Option<i64> {
    ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
        .into_iter()
        .find_map(|format| {
            chrono::NaiveDateTime::parse_from_str(value, format)
                .ok()
                .and_then(|naive| naive.and_local_timezone(Local).single())
                .map(|datetime| datetime.timestamp())
        })
}
