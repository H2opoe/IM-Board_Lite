use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::runtime_paths::{filter_unreferenced_paths, remove_owned_paths, validate_profile_id};
use crate::storage::models::ImProfile;

const PREFIX: &str = "pending_profile_cleanup:";

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Root {
    Profiles,
    Cache,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Target {
    root: Root,
    id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    version: u8,
    targets: Vec<Target>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dws_namespaces: Vec<String>,
}

// Store only relative managed directory names; never retain account configuration or credentials.
pub fn encode(paths: &[PathBuf], app_dir: &Path, cache_dir: &Path) -> anyhow::Result<String> {
    let roots = [
        (Root::Profiles, app_dir.join("Profiles")),
        (Root::Cache, cache_dir.to_path_buf()),
    ];
    let mut targets = Vec::new();
    for path in paths {
        let mut found = None;
        for (kind, root) in &roots {
            if let Ok(tail) = path.strip_prefix(root) {
                let id = tail
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("账号缓存目录名称无效。"))?;
                validate_profile_id(id).map_err(anyhow::Error::msg)?;
                found = Some(Target {
                    root: match kind {
                        Root::Profiles => Root::Profiles,
                        Root::Cache => Root::Cache,
                    },
                    id: id.into(),
                });
                break;
            }
        }
        targets.push(found.ok_or_else(|| anyhow::anyhow!("清理路径超出账号缓存根目录。"))?);
    }
    Ok(serde_json::to_string(&Pending {
        version: 1,
        targets,
        dws_namespaces: Vec::new(),
    })?)
}

// A namespace is a one-way identifier, not a credential or an absolute filesystem path.
pub fn encode_for_profile(
    paths: &[PathBuf],
    app_dir: &Path,
    cache_dir: &Path,
    profile: &ImProfile,
) -> anyhow::Result<String> {
    let mut pending: Pending = serde_json::from_str(&encode(paths, app_dir, cache_dir)?)?;
    if profile.platform == "dingtalk" {
        if let Some(namespace) = profile_namespace(profile) {
            pending.dws_namespaces.push(namespace);
        }
    }
    Ok(serde_json::to_string(&pending)?)
}

fn profile_namespace(profile: &ImProfile) -> Option<String> {
    let text = profile.config_json.get("configDir")?.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let path = if let Some(tail) = text.strip_prefix("~/") {
        dirs::home_dir()?.join(tail)
    } else {
        PathBuf::from(text)
    };
    Some(crate::runtime::dws_auth::namespace(&path))
}

fn validate_namespace(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("账号凭据清理标识无效。".into());
    }
    Ok(())
}

pub(super) fn enqueue(conn: &Connection, profile_id: &str, encoded: &str) -> anyhow::Result<()> {
    validate_profile_id(profile_id).map_err(anyhow::Error::msg)?;
    let mut pending: Pending = serde_json::from_str(encoded)?;
    if pending.version != 1 {
        anyhow::bail!("不支持的账号清理记录版本。");
    }
    for namespace in &pending.dws_namespaces {
        validate_namespace(namespace).map_err(anyhow::Error::msg)?;
    }
    let key = format!("{PREFIX}{profile_id}");
    let previous: Option<String> = conn
        .query_row("select value from app_meta where key=?1", [&key], |row| {
            row.get(0)
        })
        .optional()?;
    if let Some(previous) = previous {
        let previous: Pending = serde_json::from_str(&previous)?;
        if previous.version != 1 {
            anyhow::bail!("不支持的账号清理记录版本。");
        }
        for namespace in previous.dws_namespaces {
            validate_namespace(&namespace).map_err(anyhow::Error::msg)?;
            if !pending.dws_namespaces.contains(&namespace) {
                pending.dws_namespaces.push(namespace);
            }
        }
        for target in previous.targets {
            validate_profile_id(&target.id).map_err(anyhow::Error::msg)?;
            if !pending.targets.contains(&target) {
                pending.targets.push(target);
            }
        }
    }
    let encoded = serde_json::to_string(&pending)?;
    // The caller's account deletion and this write use the same transaction.
    conn.execute("insert into app_meta(key,value,updated_at) values(?1,?2,datetime('now')) on conflict(key) do update set value=excluded.value, updated_at=excluded.updated_at", params![format!("{PREFIX}{profile_id}"), encoded])?;
    Ok(())
}

pub fn retry_pending(
    conn: &mut Connection,
    drafts: &[ImProfile],
    app_dir: &Path,
    cache_dir: &Path,
    only_profile: Option<&str>,
) -> Result<(), String> {
    retry_with_cleanup(
        conn,
        drafts,
        app_dir,
        cache_dir,
        only_profile,
        crate::runtime::dws_auth::remove_namespace,
    )
}

fn retry_with_cleanup(
    conn: &mut Connection,
    drafts: &[ImProfile],
    app_dir: &Path,
    cache_dir: &Path,
    only_profile: Option<&str>,
    mut cleanup: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let rows = {
        let mut stmt = conn
            .prepare("select key,value from app_meta where substr(key,1,?1)=?2 order by key")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![PREFIX.len(), PREFIX], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    if rows.is_empty() {
        return Ok(());
    }
    let profiles = super::list_profiles(conn).map_err(|e| e.to_string())?;
    let protected_namespaces: BTreeSet<_> = profiles
        .iter()
        .chain(drafts)
        .filter_map(profile_namespace)
        .collect();
    let mut failed = 0;
    for (key, value) in rows {
        if only_profile.is_some_and(|id| key != format!("{PREFIX}{id}")) {
            continue;
        }
        let result = (|| -> Result<(), String> {
            let pending: Pending =
                serde_json::from_str(&value).map_err(|_| "账号清理记录格式无效。".to_owned())?;
            if pending.version != 1 {
                return Err("账号清理记录版本不受支持。".into());
            }
            for namespace in &pending.dws_namespaces {
                validate_namespace(namespace)?;
            }
            let mut paths = BTreeSet::new();
            for target in pending.targets {
                validate_profile_id(&target.id)?;
                let root = match target.root {
                    Root::Profiles => app_dir.join("Profiles"),
                    Root::Cache => cache_dir.to_path_buf(),
                };
                paths.insert(root.join(target.id));
            }
            let paths = filter_unreferenced_paths(
                paths,
                profiles.iter().chain(drafts),
                app_dir,
                cache_dir,
            )?;
            // All records and ownership are checked before either credential or file mutation.
            // A busy/failed credential cleanup retains the queue and files for startup/retry.
            for namespace in &pending.dws_namespaces {
                if !protected_namespaces.contains(namespace) {
                    cleanup(namespace)?;
                }
            }
            remove_owned_paths(&paths)?;
            conn.execute("delete from app_meta where key=?1", [&key])
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        if result.is_err() {
            failed += 1;
        }
    }
    if failed > 0 {
        return Err(format!(
            "账号记录已删除，{failed} 项本地缓存或凭据清理尚未完成；重试删除或重新启动应用后会继续清理。"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn database(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .unwrap();
        conn.execute("insert into profiles(id,platform,label,config_json,created_at,updated_at) values('one','wecom','fixture','{}','now','now')", []).unwrap();
        conn
    }
    fn count(conn: &Connection) -> i64 {
        conn.query_row(
            "select count(*) from app_meta where key like 'pending_profile_cleanup:%'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }
    #[test]
    fn deletion_and_queue_commit_together_and_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let path = app.join("Profiles/one");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("fixture"), "fixture").unwrap();
        let mut conn = database(&db);
        let encoded = encode(std::slice::from_ref(&path), &app, &cache).unwrap();
        super::super::delete_profile(&mut conn, "one", Some(&encoded)).unwrap();
        assert_eq!(count(&conn), 1);
        assert!(path.exists());
        drop(conn);
        let mut conn = Connection::open(db).unwrap();
        retry_pending(&mut conn, &[], &app, &cache, None).unwrap();
        assert_eq!(count(&conn), 0);
        assert!(!path.exists());
    }
    #[test]
    fn queue_failure_rolls_back_account_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(&dir.path().join("db"));
        conn.execute_batch("create trigger deny_cleanup before insert on app_meta when new.key like 'pending_profile_cleanup:%' begin select raise(abort,'fixture write failure'); end;").unwrap();
        let encoded = encode(&[], dir.path(), dir.path()).unwrap();
        assert!(super::super::delete_profile(&mut conn, "one", Some(&encoded)).is_err());
        assert!(super::super::profile_by_id(&conn, "one").unwrap().is_some());
        assert_eq!(count(&conn), 0);
    }
    #[test]
    fn failed_file_cleanup_is_retained_and_can_be_retried() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let path = app.join("Profiles/one");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not a directory").unwrap();
        let mut conn = database(&dir.path().join("db"));
        let encoded = encode(std::slice::from_ref(&path), &app, &cache).unwrap();
        super::super::delete_profile(&mut conn, "one", Some(&encoded)).unwrap();
        assert!(retry_pending(&mut conn, &[], &app, &cache, Some("one")).is_err());
        assert_eq!(count(&conn), 1);
        assert!(path.is_file());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        retry_pending(&mut conn, &[], &app, &cache, Some("one")).unwrap();
        assert_eq!(count(&conn), 0);
        assert!(!path.exists());
    }
    #[test]
    fn newly_referenced_directory_is_not_deleted_by_old_queue() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let path = app.join("Profiles/one");
        fs::create_dir_all(&path).unwrap();
        let mut conn = database(&dir.path().join("db"));
        let encoded = encode(std::slice::from_ref(&path), &app, &cache).unwrap();
        super::super::delete_profile(&mut conn, "one", Some(&encoded)).unwrap();
        conn.execute("insert into profiles(id,platform,label,config_json,created_at,updated_at) values('two','wecom','fixture',?1,'now','now')", [serde_json::json!({"configDir":path}).to_string()]).unwrap();
        retry_pending(&mut conn, &[], &app, &cache, None).unwrap();
        assert!(path.exists());
        assert_eq!(count(&conn), 0);
    }
    #[test]
    fn corrupt_or_traversing_queue_is_kept_without_removing_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(&dir.path().join("db"));
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        conn.execute("insert into app_meta(key,value,updated_at) values('pending_profile_cleanup:one',?1,'now')", [r#"{"version":1,"targets":[{"root":"profiles","id":"../outside"}]}"#]).unwrap();
        assert!(retry_pending(&mut conn, &[], dir.path(), dir.path(), None).is_err());
        assert!(outside.exists());
        assert_eq!(count(&conn), 1);
    }
    #[test]
    fn repeated_deletion_keeps_previously_pending_directories() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let mut conn = database(&dir.path().join("db"));
        let first = encode(&[app.join("Profiles/old")], &app, &cache).unwrap();
        enqueue(&conn, "one", &first).unwrap();
        let second = encode(&[app.join("Profiles/new")], &app, &cache).unwrap();
        super::super::delete_profile(&mut conn, "one", Some(&second)).unwrap();
        let value: String = conn
            .query_row(
                "select value from app_meta where key='pending_profile_cleanup:one'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let pending: Pending = serde_json::from_str(&value).unwrap();
        assert_eq!(pending.targets.len(), 2);
        assert!(!value.contains(app.to_str().unwrap()));
    }
    #[test]
    fn account_delete_failure_rolls_back_the_already_written_queue() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(&dir.path().join("db"));
        conn.execute_batch("create trigger deny_delete before delete on profiles begin select raise(abort,'fixture deletion failure'); end;").unwrap();
        let encoded = encode(&[], dir.path(), dir.path()).unwrap();
        assert!(super::super::delete_profile(&mut conn, "one", Some(&encoded)).is_err());
        assert!(super::super::profile_by_id(&conn, "one").unwrap().is_some());
        assert_eq!(count(&conn), 0);
    }
    #[test]
    fn an_active_draft_protects_a_directory_from_pending_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let path = app.join("Profiles/one");
        fs::create_dir_all(&path).unwrap();
        let mut conn = database(&dir.path().join("db"));
        let draft = super::super::profile_by_id(&conn, "one").unwrap().unwrap();
        let encoded = encode(std::slice::from_ref(&path), &app, &cache).unwrap();
        super::super::delete_profile(&mut conn, "one", Some(&encoded)).unwrap();
        retry_pending(&mut conn, &[draft], &app, &cache, None).unwrap();
        assert!(path.exists());
        assert_eq!(count(&conn), 0);
    }
    #[test]
    fn failed_credential_cleanup_is_durable_and_runs_before_files() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let path = app.join("Profiles/one");
        fs::create_dir_all(&path).unwrap();
        let db = dir.path().join("db");
        let mut conn = database(&db);
        let mut profile = super::super::profile_by_id(&conn, "one").unwrap().unwrap();
        profile.platform = "dingtalk".into();
        profile.config_json = serde_json::json!({"configDir":path.join("dingtalk")});
        let encoded =
            encode_for_profile(std::slice::from_ref(&path), &app, &cache, &profile).unwrap();
        assert!(!encoded.contains(app.to_str().unwrap()));
        super::super::delete_profile(&mut conn, "one", Some(&encoded)).unwrap();
        let mut calls = 0;
        assert!(retry_with_cleanup(&mut conn, &[], &app, &cache, None, |_| {
            calls += 1;
            Err("busy".into())
        })
        .is_err());
        assert_eq!(calls, 1);
        assert_eq!(count(&conn), 1);
        assert!(path.exists());
        drop(conn);
        let mut conn = Connection::open(db).unwrap();
        retry_with_cleanup(&mut conn, &[], &app, &cache, None, |value| {
            assert_eq!(value, profile_namespace(&profile).unwrap());
            Ok(())
        })
        .unwrap();
        assert_eq!(count(&conn), 0);
        assert!(!path.exists());
    }
    #[test]
    fn namespace_reused_by_saved_account_or_draft_is_preserved() {
        for saved in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut conn = database(&dir.path().join("db"));
            let mut profile = super::super::profile_by_id(&conn, "one").unwrap().unwrap();
            profile.platform = "dingtalk".into();
            profile.config_json = serde_json::json!({"configDir":"C:/fixture/adopted"});
            let encoded = encode_for_profile(&[], dir.path(), dir.path(), &profile).unwrap();
            super::super::delete_profile(&mut conn, "one", Some(&encoded)).unwrap();
            profile.id = "two".into();
            // Windows path identity is case-insensitive and accepts either slash.
            profile.config_json = serde_json::json!({"configDir":"c:\\FIXTURE\\adopted\\"});
            let drafts = if saved {
                conn.execute("insert into profiles(id,platform,label,config_json,created_at,updated_at) values('two','dingtalk','fixture',?1,'now','now')",[profile.config_json.to_string()]).unwrap();
                vec![]
            } else {
                vec![profile]
            };
            retry_with_cleanup(&mut conn, &drafts, dir.path(), dir.path(), None, |_| {
                panic!("referenced namespace must be preserved")
            })
            .unwrap();
            assert_eq!(count(&conn), 0);
        }
    }
    #[test]
    fn repeated_deletion_merges_credential_targets_and_invalid_namespace_blocks_all_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(&dir.path().join("db"));
        let mut profile = super::super::profile_by_id(&conn, "one").unwrap().unwrap();
        profile.platform = "dingtalk".into();
        for config in ["C:/fixture/first", "C:/fixture/second"] {
            profile.config_json = serde_json::json!({"configDir":config});
            enqueue(
                &conn,
                "one",
                &encode_for_profile(&[], dir.path(), dir.path(), &profile).unwrap(),
            )
            .unwrap();
        }
        super::super::delete_profile(&mut conn, "one", None).unwrap();
        let mut seen = BTreeSet::new();
        retry_with_cleanup(&mut conn, &[], dir.path(), dir.path(), None, |value| {
            seen.insert(value.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(seen.len(), 2);
        conn.execute("insert into app_meta(key,value,updated_at) values('pending_profile_cleanup:one',?1,'now')",[r#"{"version":1,"targets":[],"dws_namespaces":["../global"]}"#]).unwrap();
        assert!(
            retry_with_cleanup(&mut conn, &[], dir.path(), dir.path(), None, |_| panic!(
                "invalid namespace reached cleanup"
            ))
            .is_err()
        );
        assert_eq!(count(&conn), 1);
    }
}
