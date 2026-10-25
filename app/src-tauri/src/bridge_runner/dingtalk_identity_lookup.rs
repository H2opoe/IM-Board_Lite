//! Resolve only the confirmed current user, never match by name. A search uses only the exact confirmed user ID.
use super::{wait_for_official_cli_output, OFFICIAL_CLI_COMMAND_TIMEOUT};
use crate::{bridge_registry::BridgeRegistry, messages::account_identity};
use serde_json::Value;

pub(super) async fn resolve(
    mut command: tokio::process::Command,
    raw: &Value,
    config: &Value,
    active: Option<&BridgeRegistry>,
) -> anyhow::Result<(Vec<String>, Option<String>)> {
    let mut ids = account_identity::dingtalk_self_ids(raw, config);
    if account_identity::dingtalk_self_message_id_available(raw, config) {
        return Ok((ids, None));
    }
    let Some(user_id) = account_identity::dingtalk_self_user_id(raw, config) else {
        return Ok((
            ids,
            Some("钉钉未返回可用于补全消息身份的本人用户 ID，未使用姓名猜测。".into()),
        ));
    };
    let mut search_command = copy_command_template(&command);
    command.args([
        "contact", "user", "get", "--ids", &user_id, "--format", "json",
    ]);
    let result = async {
        let details = query(command, config, active).await?;
        let aliases =
            account_identity::dingtalk_confirmed_message_aliases(&details, config, &user_id);
        if !aliases.is_empty() {
            return Ok(aliases);
        }
        // Some successful detail responses contain only organization IDs.
        // Search the confirmed staff ID, then require exact ID equality; no
        // name-based discovery or accepting the first result.
        search_command.args([
            "contact", "user", "search", "--query", &user_id, "--format", "json",
        ]);
        let details = query(search_command, config, active).await?;
        let aliases =
            account_identity::dingtalk_confirmed_message_aliases(&details, config, &user_id);
        anyhow::ensure!(!aliases.is_empty(), "no confirmed message identity");
        Ok::<_, anyhow::Error>(aliases)
    }
    .await;
    match result {
        Ok(aliases) => {
            ids.extend(aliases);
            ids.sort();
            ids.dedup();
            Ok((ids, None))
        }
        // Never include raw contact details, paths or IDs in a warning.
        Err(_) => Ok((
            ids,
            Some(
                "钉钉本人消息 ID 补全失败；保留已确认的账号标识，请检查通讯录读取授权后重试。"
                    .into(),
            ),
        )),
    }
}

// Copy only the already prepared executable, environment and working directory.
// Every query still passes through the platform's managed process launcher.
fn copy_command_template(command: &tokio::process::Command) -> tokio::process::Command {
    let source = command.as_std();
    let mut next = tokio::process::Command::new(source.get_program());
    next.args(source.get_args());
    for (key, value) in source.get_envs() {
        if let Some(value) = value {
            next.env(key, value);
        } else {
            next.env_remove(key);
        }
    }
    if let Some(path) = source.get_current_dir() {
        next.current_dir(path);
    }
    next.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    next
}
async fn query(
    command: tokio::process::Command,
    _config: &Value,
    active: Option<&BridgeRegistry>,
) -> anyhow::Result<Value> {
    let child = super::super::command::spawn_bridge(command)?;
    let output = wait_for_official_cli_output(child, active, OFFICIAL_CLI_COMMAND_TIMEOUT).await?;
    anyhow::ensure!(output.status.success(), "identity lookup failed");
    let details: Value = serde_json::from_slice(&output.stdout)?;
    anyhow::ensure!(
        details.get("error").is_none_or(Value::is_null)
            && details.get("success").and_then(Value::as_bool) != Some(false),
        "identity lookup error"
    );
    Ok(details)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn confirmed_open_id_does_not_start_an_additional_lookup() {
        let raw =
            json!({"result":[{"openDingTalkId":"D_me","orgEmployeeModel":{"userId":"staff-me"}}]});
        let (ids, warning) = resolve(
            tokio::process::Command::new("/fixture/never-run"),
            &raw,
            &json!({}),
            None,
        )
        .await
        .unwrap();
        assert!(ids.contains(&"D_me".into()));
        assert!(warning.is_none());
    }
    #[cfg(unix)]
    async fn fake_lookup(body: &str) -> (Vec<String>, Option<String>) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cli");
        std::fs::write(&file,format!("#!/bin/sh\n[ \"$DWS_AUTH_IDENTITY\" = fixture ] || exit 9\n[ \"$1 $2\" = 'contact user' ] && [ \"$5\" = staff-me ] || exit 8\n{body}\n")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut command = tokio::process::Command::new(file);
        command
            .env("DWS_AUTH_IDENTITY", "fixture")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        resolve(
            command,
            &json!({"result":[{"orgEmployeeModel":{"userId":"staff-me"}}]}),
            &json!({}),
            None,
        )
        .await
        .unwrap()
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn exact_current_user_lookup_keeps_the_isolated_namespace_and_resolves_message_ids() {
        let (ids, warning) = fake_lookup(
            "printf '%s' '{\"result\":[{\"userId\":\"staff-me\",\"openDingTalkId\":\"D_me\"}]}'",
        )
        .await;
        assert_eq!(ids, vec!["D_me", "staff-me"]);
        assert!(warning.is_none());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn failed_lookup_retains_known_ids_and_never_echoes_contact_output() {
        let (ids, warning) = fake_lookup("printf '%s' 'PRIVATE_FIXTURE'; exit 1").await;
        assert_eq!(ids, vec!["staff-me"]);
        let warning = warning.unwrap();
        assert!(!warning.contains("PRIVATE"));
        assert!(!warning.contains("staff-me"));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn staff_only_details_use_exact_id_search_to_resolve_message_open_id() {
        let (ids, warning) = fake_lookup("if [ \"$3 $4\" = 'get --ids' ]; then printf '%s' '{\"result\":[{\"orgEmployeeModel\":{\"userId\":\"staff-me\"}}]}'; elif [ \"$3 $4\" = 'search --query' ]; then printf '%s' '{\"result\":[{\"userId\":\"staff-me\",\"openDingTalkId\":\"D_me\"}]}'; else exit 7; fi").await;
        assert_eq!(ids, vec!["D_me", "staff-me"]);
        assert!(warning.is_none());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn unrelated_search_result_or_staff_only_success_does_not_claim_completion() {
        for body in [
            "printf '%s' '{\"result\":[{\"userId\":\"staff-me\"}]}'",
            "if [ \"$3\" = get ]; then printf '%s' '{\"result\":[{\"userId\":\"staff-me\"}]}'; else printf '%s' '{\"result\":[{\"userId\":\"other\",\"openDingTalkId\":\"D_wrong\"}]}'; fi",
        ] {
            let (ids,warning)=fake_lookup(body).await;
            assert_eq!(ids,vec!["staff-me"]);
            assert!(warning.is_some());
        }
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "requires explicit authorization for scoped platform reads and local metadata counts"]
    async fn approved_readonly_current_identity_matches_local_message_metadata() {
        assert_eq!(
            std::env::var("IMBOARD_APPROVED_DINGTALK_METADATA_PROBE").as_deref(),
            Ok("1")
        );
        let base = std::path::PathBuf::from(std::env::var_os("HOME").unwrap())
            .join("Library/Application Support/IMBoard");
        let conn = rusqlite::Connection::open_with_flags(
            base.join("app.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        conn.execute_batch("pragma query_only=on").unwrap();
        let (profile_id, encoded): (String, String) = conn
            .query_row(
                "select id,config_json from profiles where platform='dingtalk'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let config: Value = serde_json::from_str(&encoded).unwrap();
        let mut command = tokio::process::Command::new(base.join(
            "OfficialCli/dingtalk/node_modules/dingtalk-workspace-cli/vendor/dws-darwin-arm64",
        ));
        for (field, key) in [
            ("configDir", "DWS_CONFIG_DIR"),
            ("dwsCacheDir", "DWS_CACHE_DIR"),
            ("dwsKeychainDir", "DWS_KEYCHAIN_DIR"),
            ("authIdentity", "DWS_AUTH_IDENTITY"),
            ("tenant", "DWS_TENANT"),
            ("authIdentity", "DINGTALK_DWS_AGENTCODE"),
        ] {
            if let Some(value) = config[field].as_str() {
                command.env(key, value);
            }
        }
        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut current = copy_command_template(&command);
        current.args(["contact", "user", "get-self", "--format", "json"]);
        let raw = query(current, &config, None).await.unwrap();
        let (ids, warning) = resolve(command, &raw, &config, None).await.unwrap();
        assert!(warning.is_none(), "confirmed message identity unavailable");
        let encoded = serde_json::json!(ids).to_string();
        let (self_messages, reciprocal):(i64,i64)=conn.query_row("with messages as (select chat_id,is_group,sender_id in (select value from json_each(?2)) as is_self from daily_messages where platform='dingtalk' and profile_id=?1 and day='2026-10-25'), chats as (select chat_id from messages where is_group=0 group by chat_id having sum(is_self)>0 and sum(not is_self)>0) select (select sum(is_self) from messages),(select count(*) from chats)",rusqlite::params![profile_id,encoded],|r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        println!("metadata_only: self_messages={self_messages}, reciprocal_direct_chats={reciprocal}, user_database_modified=false");
        assert!(
            self_messages > 0 && reciprocal > 0,
            "confirmed identity still does not establish a reciprocal chat"
        );
    }
}
