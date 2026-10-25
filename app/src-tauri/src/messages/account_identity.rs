//! Save the user identity from a successful, profile-scoped CLI preflight.
use rusqlite::{params, Connection};

pub(crate) fn record_feishu_self_identity(
    conn: &Connection,
    profile_id: &str,
    open_id: &str,
) -> rusqlite::Result<usize> {
    if open_id.trim().is_empty() || open_id.len() > 256 {
        return Ok(0);
    }
    conn.execute(
        "update profiles set config_json = json_set(config_json, '$.feishuSelfOpenId', ?2)
         where id = ?1 and platform = 'feishu'
           and coalesce(json_extract(config_json, '$.feishuSelfOpenId'), '') <> ?2",
        params![profile_id, open_id],
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmed_user_identity_refreshes_only_its_profile_and_preserves_binding_metadata() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("create table profiles(id text,platform text,config_json text);
            insert into profiles values('one','feishu','{\"accountIdentity\":{\"userId\":\"old\"},\"remark\":\"kept\"}'),('two','feishu','{}'),('other','wecom','{}');").unwrap();
        assert_eq!(
            record_feishu_self_identity(&conn, "one", "ou_self").unwrap(),
            1
        );
        assert_eq!(
            record_feishu_self_identity(&conn, "one", "ou_self").unwrap(),
            0
        );
        assert_eq!(
            record_feishu_self_identity(&conn, "other", "ou_self").unwrap(),
            0
        );
        assert_eq!(record_feishu_self_identity(&conn, "two", "").unwrap(), 0);
        let json: String = conn
            .query_row("select config_json from profiles where id='one'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["feishuSelfOpenId"], "ou_self");
        assert_eq!(value["accountIdentity"]["userId"], "old");
        assert_eq!(value["remark"], "kept");
    }
}

const DINGTALK_OWN_ID_FIELDS: &[&str] = &[
    "openDingTalkId",
    "openDingtalkId",
    "userId",
    "user_id",
    "userid",
    "orgUserId",
    "staffId",
    "unionId",
    "uid",
];
fn own_ids(record: &serde_json::Value) -> std::collections::BTreeSet<String> {
    use serde_json::Value;
    let mut ids = std::collections::BTreeSet::new();
    // Never recurse into managers, departments or other users.
    for object in [record, &record["orgEmployeeModel"], &record["userModel"]] {
        for field in DINGTALK_OWN_ID_FIELDS {
            let id = match &object[*field] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => continue,
            };
            let id = id.trim();
            if !id.is_empty() && id.len() <= 256 {
                ids.insert(id.to_owned());
            }
        }
    }
    ids
}
fn dingtalk_identity_records(raw: &serde_json::Value) -> Vec<&serde_json::Value> {
    fn records(value: &serde_json::Value, depth: usize) -> Vec<&serde_json::Value> {
        if depth > 4 {
            return Vec::new();
        }
        if let Some(items) = value.as_array() {
            return items.iter().collect();
        }
        for key in ["result", "data", "userList", "items", "users"] {
            if let Some(child) = value
                .get(key)
                .filter(|child| child.is_object() || child.is_array())
            {
                return records(child, depth + 1);
            }
        }
        if value.is_object() {
            vec![value]
        } else {
            Vec::new()
        }
    }
    records(raw, 0)
}

// A successful details lookup can return only the staff ID. That is not a
// completed message identity mapping. Require an explicit OpenID on the same
// record that proves the queried user ID, never a name or another user's ID.
pub(crate) fn dingtalk_confirmed_message_aliases(
    raw: &serde_json::Value,
    config: &serde_json::Value,
    user_id: &str,
) -> Vec<String> {
    let aliases = dingtalk_confirmed_aliases(raw, config, user_id);
    if aliases.is_empty() {
        return aliases;
    }
    let has_message_id = dingtalk_identity_records(raw).into_iter().any(|record| {
        own_ids(record).contains(user_id)
            && [record, &record["orgEmployeeModel"], &record["userModel"]]
                .iter()
                .any(|object| {
                    ["openDingTalkId", "openDingtalkId"].iter().any(|field| {
                        object[*field].as_str().is_some_and(|id| {
                            !id.trim().is_empty() && aliases.iter().any(|alias| alias == id.trim())
                        })
                    })
                })
    });
    if has_message_id {
        aliases
    } else {
        Vec::new()
    }
}

fn dingtalk_self_record<'a>(
    raw: &'a serde_json::Value,
    config: &serde_json::Value,
) -> Option<&'a serde_json::Value> {
    use serde_json::Value;
    let records = dingtalk_identity_records(raw);
    let expected = config
        .pointer("/accountIdentity/tenantId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut candidates = Vec::new();
    for record in &records {
        let corp = record["orgEmployeeModel"]
            .get("corpId")
            .or_else(|| record.get("corpId"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if (!expected.is_empty() && !corp.is_empty() && expected != corp)
            || (records.len() > 1 && (expected.is_empty() || corp.is_empty()))
        {
            continue;
        }
        candidates.push(*record);
    }
    if candidates.len() > 1 {
        let expected_user = config
            .pointer("/accountIdentity/userId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())?;
        candidates.retain(|record| own_ids(record).contains(expected_user));
    }
    if candidates.len() != 1 {
        return None;
    }
    Some(candidates[0])
}

pub(crate) fn dingtalk_self_ids(
    raw: &serde_json::Value,
    config: &serde_json::Value,
) -> Vec<String> {
    dingtalk_self_record(raw, config)
        .map(|record| own_ids(record).into_iter().collect())
        .unwrap_or_default()
}
pub(crate) fn dingtalk_self_message_id_available(
    raw: &serde_json::Value,
    config: &serde_json::Value,
) -> bool {
    let Some(record) = dingtalk_self_record(raw, config) else {
        return false;
    };
    [record, &record["orgEmployeeModel"], &record["userModel"]]
        .iter()
        .any(|object| {
            ["openDingTalkId", "openDingtalkId"].iter().any(|field| {
                object[*field]
                    .as_str()
                    .is_some_and(|id| !id.trim().is_empty() && id.len() <= 256)
            })
        })
}
pub(crate) fn dingtalk_self_user_id(
    raw: &serde_json::Value,
    config: &serde_json::Value,
) -> Option<String> {
    let record = dingtalk_self_record(raw, config)?;
    for object in [&record["orgEmployeeModel"], record] {
        for field in ["userId", "user_id", "userid", "orgUserId", "staffId"] {
            if let Some(id) = object[field].as_str().map(str::trim).filter(|id| {
                !id.is_empty()
                    && id.len() <= 256
                    && !id.contains(',')
                    && !id.chars().any(char::is_control)
            }) {
                return Some(id.into());
            }
        }
    }
    None
}
pub(crate) fn dingtalk_confirmed_aliases(
    raw: &serde_json::Value,
    config: &serde_json::Value,
    user_id: &str,
) -> Vec<String> {
    use serde_json::Value;
    let records = dingtalk_identity_records(raw);
    let expected = config
        .pointer("/accountIdentity/tenantId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut matches = std::collections::BTreeSet::new();
    for record in records {
        let corp = record["orgEmployeeModel"]
            .get("corpId")
            .or_else(|| record.get("corpId"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !expected.is_empty() && !corp.is_empty() && corp != expected {
            continue;
        }
        let ids = own_ids(record);
        if ids.contains(user_id) {
            matches.insert(ids.into_iter().collect::<Vec<_>>());
        }
    }
    if matches.len() == 1 {
        matches.into_iter().next().unwrap_or_default()
    } else {
        Vec::new()
    }
}
pub(crate) fn record_dingtalk_self_identity(
    conn: &Connection,
    profile_id: &str,
    ids: &[String],
) -> rusqlite::Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let encoded = serde_json::json!(ids).to_string();
    conn.execute("update profiles set config_json=json_set(config_json,'$.dingtalkSelfIds',json(?2))
        where id=?1 and platform='dingtalk' and coalesce(json_extract(config_json,'$.dingtalkSelfIds'),'')<>?2",params![profile_id,encoded])
}
#[cfg(test)]
mod dingtalk_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn identity_aliases_are_scoped_to_the_current_organization_not_managers_or_other_accounts() {
        let raw = json!({"result":[
            {"openDingTalkId":"D_me","uid":123,"orgEmployeeModel":{"corpId":"corp","userId":"staff-me","orgMasterUserId":"manager","depts":[{"userId":"not-me"}]}},
            {"openDingTalkId":"D_other","orgEmployeeModel":{"corpId":"other","userId":"other-staff"}}
        ]});
        assert_eq!(
            dingtalk_self_ids(&raw, &json!({"accountIdentity":{"tenantId":"corp"}})),
            vec!["123", "D_me", "staff-me"]
        );
        assert!(dingtalk_self_ids(&raw, &json!({})).is_empty());
    }
    #[test]
    fn stored_aliases_never_modify_a_different_platform_or_profile() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("create table profiles(id text,platform text,config_json text);insert into profiles values('a','dingtalk','{}'),('b','feishu','{}');").unwrap();
        let ids = vec!["D_me".into(), "staff-me".into()];
        assert_eq!(record_dingtalk_self_identity(&conn, "a", &ids).unwrap(), 1);
        assert_eq!(record_dingtalk_self_identity(&conn, "a", &ids).unwrap(), 0);
        assert_eq!(record_dingtalk_self_identity(&conn, "b", &ids).unwrap(), 0);
    }
}

#[cfg(test)]
mod ambiguous_dingtalk_identity_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn same_enterprise_accounts_are_not_merged_and_ambiguous_identity_is_rejected() {
        let raw = json!({"result":[{"openDingTalkId":"D_one","orgEmployeeModel":{"corpId":"corp","userId":"one"}},{"openDingTalkId":"D_two","orgEmployeeModel":{"corpId":"corp","userId":"two"}}]});
        assert!(
            dingtalk_self_ids(&raw, &json!({"accountIdentity":{"tenantId":"corp"}})).is_empty()
        );
        assert_eq!(
            dingtalk_self_ids(
                &raw,
                &json!({"accountIdentity":{"tenantId":"corp","userId":"one"}})
            ),
            vec!["D_one", "one"]
        );
    }
}

#[cfg(test)]
mod dingtalk_message_id_mapping_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn exact_staff_identity_is_resolved_to_message_ids_without_accepting_another_user() {
        let config = json!({"accountIdentity":{"tenantId":"corp","userId":"staff-me"}});
        let raw = json!({"result":[{"orgEmployeeModel":{"corpId":"corp","userId":"staff-me"}}]});
        assert_eq!(
            dingtalk_self_user_id(&raw, &config).as_deref(),
            Some("staff-me")
        );
        let details = json!({"result":[{"openDingTalkId":"D_me","orgEmployeeModel":{"corpId":"corp","userId":"staff-me","orgMasterUserId":"manager"}}]});
        assert_eq!(
            dingtalk_confirmed_aliases(&details, &config, "staff-me"),
            vec!["D_me", "staff-me"]
        );
        assert!(dingtalk_confirmed_aliases(&details, &config, "other-user").is_empty());
        let wrapped = json!({"result":{"userList":[{"userId":"other","openDingTalkId":"D_other"},{"userId":"staff-me","openDingTalkId":"D_me"}]}});
        assert_eq!(
            dingtalk_confirmed_aliases(&wrapped, &config, "staff-me"),
            vec!["D_me", "staff-me"]
        );
    }
}

#[cfg(test)]
mod incomplete_details_regression_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn staff_only_details_do_not_claim_to_have_resolved_message_identity() {
        let config = json!({"accountIdentity":{"tenantId":"corp","userId":"staff-me"}});
        let details =
            json!({"result":[{"orgEmployeeModel":{"corpId":"corp","userId":"staff-me"}}]});
        assert!(dingtalk_confirmed_message_aliases(&details, &config, "staff-me").is_empty());
        let search = json!({"result":[{"userId":"staff-me","openDingTalkId":"D_me"}]});
        assert_eq!(
            dingtalk_confirmed_message_aliases(&search, &config, "staff-me"),
            vec!["D_me", "staff-me"]
        );
        let wrong = json!({"result":[{"userId":"someone-else","openDingTalkId":"D_me","name":"same-name"}]});
        assert!(dingtalk_confirmed_message_aliases(&wrong, &config, "staff-me").is_empty());
    }
    #[test]
    fn object_envelopes_are_supported_without_collecting_unrelated_nested_ids() {
        let raw = json!({"data":{"result":{"userId":"staff-me","manager":{"openDingTalkId":"D_manager"}}}});
        assert_eq!(
            dingtalk_self_user_id(&raw, &json!({})).as_deref(),
            Some("staff-me")
        );
        assert!(!dingtalk_self_message_id_available(&raw, &json!({})));
        assert_eq!(dingtalk_self_ids(&raw, &json!({})), vec!["staff-me"]);
    }
}
