//! Shared read semantics. Zero rows are valid only after a successful, explicit
//! collection response. Platform protocols still own auth and pagination.
use super::{BridgeEnvelope, BridgeError};
use serde::Deserialize;
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Contract {
    commands: Vec<String>,
    collection_keys: Vec<String>,
    container_keys: Vec<String>,
    success_flags: Vec<String>,
    completion_flags: Vec<String>,
    incomplete_flags: Vec<String>,
    failure_lists: Vec<String>,
    failure_counts: Vec<String>,
    failure_codes: Vec<String>,
    messages: Messages,
    error_codes: Messages,
}
#[derive(Deserialize)]
struct Messages {
    invalid: String,
    incomplete: String,
    api: String,
}
fn contract() -> &'static Contract {
    static CONTRACT: OnceLock<Contract> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        serde_json::from_str(include_str!("../../../shared/read-result-contract.json"))
            .expect("valid shared read contract")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadProblem {
    Invalid,
    Incomplete,
    Api,
}
impl ReadProblem {
    pub(crate) fn message(self) -> &'static str {
        let messages = &contract().messages;
        match self {
            Self::Invalid => &messages.invalid,
            Self::Incomplete => &messages.incomplete,
            Self::Api => &messages.api,
        }
    }
    pub(crate) fn code(self) -> &'static str {
        let codes = &contract().error_codes;
        match self {
            Self::Invalid => &codes.invalid,
            Self::Incomplete => &codes.incomplete,
            Self::Api => &codes.api,
        }
    }
}

pub(crate) fn collection(data: &Value) -> Result<&Vec<Value>, ReadProblem> {
    let keys = contract()
        .collection_keys
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    collection_with_keys(data, &keys)
}
pub(crate) fn named_collection<'a>(
    data: &'a Value,
    field: &str,
) -> Result<&'a Vec<Value>, ReadProblem> {
    data.get(field)
        .and_then(Value::as_array)
        .ok_or(ReadProblem::Invalid)
}

pub(crate) fn collection_with_keys<'a>(
    data: &'a Value,
    keys: &[&str],
) -> Result<&'a Vec<Value>, ReadProblem> {
    fn find<'a>(
        data: &'a Value,
        keys: &[&str],
        depth: usize,
    ) -> Result<&'a Vec<Value>, ReadProblem> {
        if let Some(rows) = data.as_array() {
            return Ok(rows);
        }
        if depth > 8 || !data.is_object() {
            return Err(ReadProblem::Invalid);
        }
        for key in keys {
            if let Some(value) = data.get(*key) {
                if let Some(rows) = value.as_array() {
                    return Ok(rows);
                }
                if contract()
                    .container_keys
                    .iter()
                    .any(|container| container == key)
                    && value.is_object()
                {
                    continue;
                }
                return Err(ReadProblem::Invalid);
            }
        }
        for key in &contract().container_keys {
            if let Some(child) = data
                .get(key)
                .filter(|value| value.is_object() || value.is_array())
            {
                return find(child, keys, depth + 1);
            }
        }
        Err(ReadProblem::Invalid)
    }
    find(data, keys, 0)
}

fn scopes(data: &Value) -> Vec<&Value> {
    fn collect<'a>(data: &'a Value, depth: usize, output: &mut Vec<&'a Value>) {
        if depth > 8 || !data.is_object() {
            return;
        }
        output.push(data);
        for key in &contract().container_keys {
            if let Some(child) = data.get(key) {
                collect(child, depth + 1, output);
            }
        }
    }
    let mut output = Vec::new();
    collect(data, 0, &mut output);
    output
}

pub(crate) fn ensure_complete(data: &Value) -> Result<(), ReadProblem> {
    let mut incomplete = false;
    for scope in scopes(data) {
        if scope.get("error").is_some_and(|value| !value.is_null()) {
            return Err(ReadProblem::Api);
        }
        for key in &contract().failure_codes {
            if let Some(code) = scope.get(key).filter(|value| !value.is_null()) {
                match code {
                    Value::Number(value) if value.as_i64() == Some(0) => {}
                    Value::String(value) if value == "0" => {}
                    Value::Number(_) | Value::String(_) => return Err(ReadProblem::Api),
                    _ => return Err(ReadProblem::Invalid),
                }
            }
        }
        for key in &contract().success_flags {
            if let Some(value) = scope.get(key) {
                match value.as_bool() {
                    Some(true) => {}
                    Some(false) => return Err(ReadProblem::Api),
                    None => return Err(ReadProblem::Invalid),
                }
            }
        }
        for key in &contract().completion_flags {
            if let Some(value) = scope.get(key) {
                incomplete |= !value.as_bool().ok_or(ReadProblem::Invalid)?;
            }
        }
        for key in &contract().incomplete_flags {
            if let Some(value) = scope.get(key) {
                incomplete |= value.as_bool().ok_or(ReadProblem::Invalid)?;
            }
        }
        for key in &contract().failure_lists {
            if let Some(value) = scope.get(key).filter(|value| !value.is_null()) {
                incomplete |= !value.as_array().ok_or(ReadProblem::Invalid)?.is_empty();
            }
        }
        for key in &contract().failure_counts {
            if let Some(value) = scope.get(key) {
                incomplete |= value.as_u64().ok_or(ReadProblem::Invalid)? > 0;
            }
        }
    }
    if incomplete {
        Err(ReadProblem::Incomplete)
    } else {
        Ok(())
    }
}

pub(crate) fn failure_messages(data: &Value) -> Vec<String> {
    let mut messages = Vec::new();
    for scope in scopes(data) {
        for key in &contract().failure_lists {
            if let Some(items) = scope.get(key).and_then(Value::as_array) {
                for item in items {
                    messages.push(
                        item.as_str()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(crate::security::sanitize_log)
                            .unwrap_or_else(|| ReadProblem::Incomplete.message().to_owned()),
                    );
                }
            }
        }
    }
    if messages.is_empty() {
        if let Err(problem) = ensure_complete(data) {
            messages.push(problem.message().to_owned());
        }
    }
    messages
}

pub(crate) fn enforce(command: &str, mut envelope: BridgeEnvelope) -> BridgeEnvelope {
    if !contract().commands.iter().any(|read| read == command) || !envelope.ok {
        return envelope;
    }
    if envelope.error.is_some() {
        envelope.ok = false;
        return envelope;
    }
    let problem = collection(&envelope.data)
        .err()
        .or_else(|| ensure_complete(&envelope.data).err());
    if let Some(problem) = problem {
        // A history reader can retain rows from readable shards while reporting
        // failed shards. Sync records these failures before retaining the rows.
        let partial_history = problem == ReadProblem::Incomplete
            && command == "fetch-messages"
            && collection(&envelope.data).is_ok_and(|rows| !rows.is_empty())
            && scopes(&envelope.data).iter().any(|scope| {
                contract().failure_lists.iter().any(|key| {
                    scope
                        .get(key)
                        .and_then(Value::as_array)
                        .is_some_and(|items| !items.is_empty())
                })
            });
        if !partial_history {
            envelope.ok = false;
            envelope.data = Value::Null;
            envelope.error = Some(BridgeError {
                code: problem.code().into(),
                message: problem.message().into(),
                recoverable: true,
            });
        }
    }
    envelope
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn result(data: Value) -> BridgeEnvelope {
        BridgeEnvelope {
            ok: true,
            data,
            warnings: vec![],
            error: None,
            meta: json!({}),
        }
    }
    #[test]
    fn shared_frontend_and_backend_fixtures_agree() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../../shared/read-result-fixtures.json"))
                .unwrap();
        for fixture in fixtures.as_array().unwrap() {
            let result =
                collection(&fixture["data"]).and_then(|_| ensure_complete(&fixture["data"]));
            let actual = match result {
                Ok(()) => "complete",
                Err(ReadProblem::Invalid) => "invalid",
                Err(ReadProblem::Incomplete) => "incomplete",
                Err(ReadProblem::Api) => "api",
            };
            assert_eq!(
                actual,
                fixture["expected"].as_str().unwrap(),
                "{}",
                fixture["name"]
            );
        }
    }
    #[test]
    fn every_read_command_accepts_explicit_empty_and_nonempty_collections() {
        for command in &contract().commands {
            for data in [
                json!([]),
                json!({"items":[]}),
                json!({"messages":[],"failures":null}),
                json!({"data":{"items":[]}}),
                json!([{"id":"row"}]),
            ] {
                let read = enforce(command, result(data));
                assert!(read.ok, "{command}: {read:?}");
                assert!(read.warnings.is_empty());
            }
        }
    }
    #[test]
    fn absent_invalid_denied_and_incomplete_never_become_empty_success() {
        for data in [
            Value::Null,
            json!({}),
            json!({"messages":null}),
            json!({"text":"private body"}),
            json!({"items":[],"success":false}),
            json!({"items":[],"error":{"code":403}}),
            json!({"items":[],"has_more":true}),
            json!({"items":[],"complete":false}),
            json!({"items":[],"failedCount":1}),
            json!({"items":[],"failures":"bad"}),
            json!({"items":[],"hasMore":"false"}),
        ] {
            for command in &contract().commands {
                let read = enforce(command, result(data.clone()));
                assert!(!read.ok, "{command}: {data}");
                assert!(!read.error.unwrap().message.contains("private body"));
            }
        }
    }
    #[test]
    fn structured_permission_errors_remain_authoritative_even_with_empty_data() {
        let mut read = result(json!([]));
        read.ok = false;
        read.error = Some(BridgeError {
            code: "MISSING_SCOPE".into(),
            message: "permission denied".into(),
            recoverable: true,
        });
        assert_eq!(
            enforce("search-messages", read).error.unwrap().code,
            "MISSING_SCOPE"
        );
    }
    #[test]
    fn partial_history_retains_rows_and_failure_details_without_claiming_empty_success() {
        let data =
            json!({"messages":["retained row"],"failures":["shard failed api_key=private-value"]});
        let read = enforce("fetch-messages", result(data));
        assert!(read.ok);
        let failures = failure_messages(&read.data);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("shard failed"));
        assert!(!failures[0].contains("private-value"));
        assert!(
            !enforce(
                "fetch-messages",
                result(json!({"messages":[],"failures":["shard failed"]}))
            )
            .ok
        );
    }
}
