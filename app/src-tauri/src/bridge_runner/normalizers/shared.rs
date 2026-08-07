use chrono::Local;

pub(in crate::bridge_runner) fn first_json_array<'a>(
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

pub(in crate::bridge_runner) fn today_start_text() -> String {
    Local::now().format("%Y-%m-%d 00:00:00").to_string()
}

pub(in crate::bridge_runner) fn now_text() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub(in crate::bridge_runner) fn remove_empty_json_fields(
    value: serde_json::Value,
) -> serde_json::Value {
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
