use std::collections::HashMap;
use std::path::Path;

use crate::storage::{models::ImProfile, AppState};

pub fn register(state: &AppState, profile: ImProfile) -> Result<(), String> {
    super::runtime_paths::validate_profile_id(&profile.id)?;
    let mut drafts = state.binding_drafts.lock().map_err(|err| err.to_string())?;
    insert(&mut drafts, profile)
}

fn insert(drafts: &mut HashMap<String, ImProfile>, profile: ImProfile) -> Result<(), String> {
    if drafts.len() >= 128 && !drafts.contains_key(&profile.id) {
        return Err("未结束的绑定会话过多，请关闭不再使用的绑定窗口后重试。".into());
    }
    drafts.insert(profile.id.clone(), profile);
    Ok(())
}

pub fn release(state: &AppState, id: &str) -> Result<(), String> {
    super::runtime_paths::validate_profile_id(id)?;
    // This invalidates only the in-memory draft, never a saved account or its files.
    state
        .binding_drafts
        .lock()
        .map_err(|err| err.to_string())?
        .remove(id);
    Ok(())
}

/// The caller holds DB -> drafts locks until the save and draft removal finish.
/// Only a live binding session may create a previously absent account.
pub fn save_registered(
    conn: &rusqlite::Connection,
    drafts: &mut HashMap<String, ImProfile>,
    profile: ImProfile,
) -> Result<ImProfile, String> {
    super::runtime_paths::validate_profile_id(&profile.id)?;
    let existing = super::profile_by_id(conn, &profile.id).map_err(|err| err.to_string())?;
    let source = existing
        .as_ref()
        .or_else(|| drafts.get(&profile.id))
        .ok_or_else(|| "账号已被删除或绑定会话已失效，请刷新后重新添加账号。".to_owned())?;
    if source.platform != profile.platform {
        return Err("账号平台与保存平台不一致。".to_owned());
    }
    if let Some(current) = existing.as_ref() {
        if current.updated_at != profile.updated_at {
            return Err("账号配置已发生变化，请刷新后重新修改。".to_owned());
        }
    }
    let saved = super::upsert_profile(conn, profile).map_err(|err| err.to_string())?;
    drafts.remove(&saved.id);
    Ok(saved)
}

pub fn resolve_for_command(
    state: &AppState,
    id: &str,
    platform: &str,
    command: &str,
) -> Result<ImProfile, String> {
    super::runtime_paths::validate_profile_id(id)?;
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
        "credentials.enc"
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
    #[test]
    fn full_registry_rejects_new_sessions_without_evicting_active_ones() {
        let mut drafts = HashMap::new();
        for n in 0..128 {
            let mut p = draft();
            p.id = format!("fixture_{n}");
            insert(&mut drafts, p).unwrap();
        }
        let mut extra = draft();
        extra.id = "new_session".into();
        assert!(insert(&mut drafts, extra).is_err());
        assert_eq!(drafts.len(), 128);
        assert!(drafts.contains_key("fixture_0"));
        let mut existing = draft();
        existing.id = "fixture_0".into();
        existing.label = "updated".into();
        insert(&mut drafts, existing).unwrap();
        assert_eq!(drafts["fixture_0"].label, "updated");
        drafts.remove("fixture_0");
        insert(&mut drafts, draft()).unwrap();
        assert_eq!(drafts.len(), 128);
    }

    #[test]
    fn releasing_a_draft_invalidates_only_unsaved_resolution() {
        let saved = draft();
        let mut drafts = HashMap::from([(saved.id.clone(), saved.clone())]);
        drafts.remove(&saved.id);
        drafts.remove(&saved.id);
        assert!(resolve(None, &drafts, &saved.id, "feishu", "auth-status").is_err());
        assert!(resolve(
            Some(saved.clone()),
            &drafts,
            &saved.id,
            "feishu",
            "auth-status"
        )
        .is_ok());
    }
    fn fixture_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .unwrap();
        conn
    }

    #[test]
    fn deleted_account_cannot_be_recreated_by_stale_save() {
        let mut conn = fixture_db();
        let p = draft();
        let mut drafts = HashMap::from([(p.id.clone(), p.clone())]);
        let saved = save_registered(&conn, &mut drafts, p).unwrap();
        super::super::delete_profile(&mut conn, &saved.id, None).unwrap();
        assert!(save_registered(&conn, &mut drafts, saved.clone()).is_err());
        assert!(super::super::profile_by_id(&conn, &saved.id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn released_or_unknown_draft_cannot_create_account() {
        let conn = fixture_db();
        let p = draft();
        let mut drafts = HashMap::from([(p.id.clone(), p.clone())]);
        drafts.remove(&p.id);
        assert!(save_registered(&conn, &mut drafts, p.clone()).is_err());
        assert_eq!(
            conn.query_row("select count(*) from profiles", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn live_draft_is_consumed_and_saved_account_can_be_updated() {
        let conn = fixture_db();
        let p = draft();
        let mut drafts = HashMap::from([(p.id.clone(), p.clone())]);
        let mut saved = save_registered(&conn, &mut drafts, p).unwrap();
        assert!(drafts.is_empty());
        saved.label = "edited".into();
        assert_eq!(
            save_registered(&conn, &mut drafts, saved).unwrap().label,
            "edited"
        );
    }

    #[test]
    fn platform_changes_are_rejected_without_consuming_draft_or_modifying_account() {
        let conn = fixture_db();
        let p = draft();
        let mut drafts = HashMap::from([(p.id.clone(), p.clone())]);
        let mut wrong = p.clone();
        wrong.platform = "wecom".into();
        assert!(save_registered(&conn, &mut drafts, wrong.clone()).is_err());
        assert!(drafts.contains_key(&p.id));
        save_registered(&conn, &mut drafts, p.clone()).unwrap();
        assert!(save_registered(&conn, &mut drafts, wrong).is_err());
        assert_eq!(
            super::super::profile_by_id(&conn, &p.id)
                .unwrap()
                .unwrap()
                .platform,
            "feishu"
        );
    }
    #[test]
    fn database_failure_keeps_live_draft_for_retry() {
        let conn = fixture_db();
        let p = draft();
        let mut drafts = HashMap::from([(p.id.clone(), p.clone())]);
        conn.execute_batch("create trigger fail_save before insert on profiles begin select raise(abort, 'fixture'); end;").unwrap();
        assert!(save_registered(&conn, &mut drafts, p.clone()).is_err());
        assert!(drafts.contains_key(&p.id));
        assert!(super::super::profile_by_id(&conn, &p.id).unwrap().is_none());
        conn.execute_batch("drop trigger fail_save;").unwrap();
        assert!(save_registered(&conn, &mut drafts, p).is_ok());
        assert!(drafts.is_empty());
    }
    #[test]
    fn stale_update_cannot_overwrite_newer_configuration() {
        let conn = fixture_db();
        let p = draft();
        let mut drafts = HashMap::from([(p.id.clone(), p.clone())]);
        let stale = save_registered(&conn, &mut drafts, p).unwrap();
        let mut fresh = stale.clone();
        fresh.config_json["configDir"] = serde_json::json!("new-directory");
        let saved = save_registered(&conn, &mut drafts, fresh).unwrap();
        let mut toggle = stale;
        toggle.enabled = false;
        assert!(save_registered(&conn, &mut drafts, toggle).is_err());
        let current = super::super::profile_by_id(&conn, &saved.id)
            .unwrap()
            .unwrap();
        assert_eq!(current.config_json["configDir"], "new-directory");
        assert!(current.enabled);
        assert_eq!(current.updated_at, saved.updated_at);
    }

    #[test]
    fn save_after_clock_rollback_still_advances_version() {
        let conn = fixture_db();
        let p = draft();
        let mut drafts = HashMap::from([(p.id.clone(), p.clone())]);
        let mut saved = save_registered(&conn, &mut drafts, p).unwrap();
        saved.updated_at = "2099-01-01T00:00:00+00:00".into();
        conn.execute(
            "update profiles set updated_at = ?1 where id = ?2",
            rusqlite::params![saved.updated_at, saved.id],
        )
        .unwrap();
        let next = save_registered(&conn, &mut drafts, saved.clone()).unwrap();
        assert!(
            chrono::DateTime::parse_from_rfc3339(&next.updated_at).unwrap()
                > chrono::DateTime::parse_from_rfc3339(&saved.updated_at).unwrap()
        );
        assert!(save_registered(&conn, &mut drafts, saved).is_err());
    }
}
