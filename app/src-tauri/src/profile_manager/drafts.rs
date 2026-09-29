use std::collections::HashMap;
use std::path::Path;

use crate::storage::{models::ImProfile, AppState};

pub fn register(state: &AppState, profile: ImProfile) -> Result<(), String> {
    let mut drafts = state.binding_drafts.lock().map_err(|err| err.to_string())?;
    // Abandoned modal sessions must not grow the in-memory registry indefinitely.
    if drafts.len() >= 128 {
        if let Some(id) = drafts
            .values()
            .min_by_key(|item| &item.created_at)
            .map(|item| item.id.clone())
        {
            drafts.remove(&id);
        }
    }
    drafts.insert(profile.id.clone(), profile);
    Ok(())
}

pub fn resolve_for_command(
    state: &AppState,
    id: &str,
    platform: &str,
    command: &str,
) -> Result<ImProfile, String> {
    let saved = {
        let conn = state.db.lock().map_err(|err| err.to_string())?;
        super::profile_by_id(&conn, id).map_err(|err| err.to_string())?
    };
    let drafts = state.binding_drafts.lock().map_err(|err| err.to_string())?;
    resolve(saved, &drafts, id, platform, command)
}

fn resolve(
    saved: Option<ImProfile>,
    drafts: &HashMap<String, ImProfile>,
    id: &str,
    platform: &str,
    command: &str,
) -> Result<ImProfile, String> {
    let profile = match saved {
        Some(profile) => profile,
        None if matches!(
            command,
            "auth-status" | "account-identity" | "get-self" | "list-chats"
        ) =>
        {
            drafts
                .get(id)
                .cloned()
                .ok_or_else(|| "绑定会话已失效，请重新添加账号。".to_owned())?
        }
        None => return Err("账号不存在或已被删除，请刷新后重试。".to_owned()),
    };
    if profile.platform != platform {
        return Err("账号平台与执行平台不一致。".to_owned());
    }
    Ok(profile)
}

pub fn record_deployment(
    state: &AppState,
    id: &str,
    platform: &str,
    cli_path: &Path,
    config_dir: &Path,
) -> Result<(), String> {
    let mut drafts = state.binding_drafts.lock().map_err(|err| err.to_string())?;
    if let Some(profile) = drafts.get_mut(id) {
        if profile.platform != platform {
            return Err("账号平台与部署平台不一致。".to_owned());
        }
        apply_deployment(profile, cli_path, config_dir);
    }
    Ok(())
}

fn apply_deployment(profile: &mut ImProfile, cli_path: &Path, config_dir: &Path) {
    let config = &mut profile.config_json;
    config["authType"] = serde_json::json!("cli_session");
    config["cliPath"] = serde_json::json!(cli_path);
    config["configDir"] = serde_json::json!(config_dir);
    config["configPath"] = serde_json::json!(config_dir.join(if profile.platform == "wecom" {
        "bot.enc"
    } else {
        "config.json"
    }));
    config["cacheDir"] = serde_json::json!(config_dir.join("cache"));
    config["tmpDir"] = serde_json::json!(config_dir.join("cache").join("tmp"));
    if profile.platform == "feishu" {
        config["larkConfigDir"] = serde_json::json!(config_dir.join(".lark-cli"));
        config["profileName"] = serde_json::json!(profile.id);
    }
    if profile.platform == "dingtalk" {
        config["authIdentity"] = serde_json::json!(profile.id);
        config["tenant"] = serde_json::json!(profile.id);
        config["dwsCacheDir"] = serde_json::json!(config_dir.join("cache"));
        config["dwsKeychainDir"] = serde_json::json!(config_dir.join("keychain"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draft() -> ImProfile {
        ImProfile {
            id: "feishu_test".into(),
            platform: "feishu".into(),
            label: "test".into(),
            enabled: true,
            config_json: serde_json::json!({}),
            status: "normal".into(),
            sort_order: 0,
            created_at: "2026-01-01".into(),
            updated_at: "2026-01-01".into(),
        }
    }
    #[test]
    fn unsaved_draft_can_verify_identity_but_cannot_read_messages() {
        let mut profile = draft();
        apply_deployment(
            &mut profile,
            Path::new("trusted-cli"),
            Path::new("trusted-config"),
        );
        let drafts = HashMap::from([(profile.id.clone(), profile)]);
        let verified = resolve(None, &drafts, "feishu_test", "feishu", "auth-status").unwrap();
        assert_eq!(verified.config_json["cliPath"], "trusted-cli");
        assert!(resolve(None, &drafts, "feishu_test", "feishu", "fetch-messages").is_err());
        assert!(resolve(None, &drafts, "forged", "feishu", "auth-status").is_err());
        assert!(resolve(None, &drafts, "feishu_test", "feishu", "list-chats").is_ok());
        assert!(resolve(None, &drafts, "feishu_test", "wecom", "auth-status").is_err());
        assert_eq!(
            verified.config_json["larkConfigDir"],
            serde_json::json!(Path::new("trusted-config").join(".lark-cli"))
        );
    }
    #[test]
    fn persisted_profile_takes_precedence_over_draft() {
        let mut saved = draft();
        saved.config_json["cliPath"] = serde_json::json!("persisted-cli");
        let drafts = HashMap::from([(saved.id.clone(), draft())]);
        let verified =
            resolve(Some(saved), &drafts, "feishu_test", "feishu", "auth-status").unwrap();
        assert_eq!(verified.config_json["cliPath"], "persisted-cli");
    }
}
