use super::LATEST_SCHEMA_VERSION;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn backup_database_before_upgrade(
    conn: &Connection,
    app_dir: &std::path::Path,
) -> anyhow::Result<Option<PathBuf>> {
    scrub_existing_backups(app_dir)?;
    let current_version =
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?;
    let user_table_count: i64 = conn.query_row(
        "select count(*) from sqlite_master where type = 'table' and name not like 'sqlite_%'",
        [],
        |row| row.get(0),
    )?;
    if current_version >= LATEST_SCHEMA_VERSION || user_table_count == 0 {
        return Ok(None);
    }

    let backup_dir = app_dir.join("Backups");
    fs::create_dir_all(&backup_dir)?;
    write_restore_notice(&backup_dir)?;
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let backup_path = backup_dir.join(format!(
        "app-v{current_version}-before-v{LATEST_SCHEMA_VERSION}-{timestamp}.sqlite"
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(&backup_path)?);
    let mut destination = Connection::open(&backup_path)?;
    let backup = rusqlite::backup::Backup::new(conn, &mut destination)?;
    backup.run_to_completion(128, std::time::Duration::from_millis(25), None)?;
    drop(backup);
    scrub_backup_key(&destination)?;
    let integrity: String =
        destination.pragma_query_value(None, "integrity_check", |row| row.get(0))?;
    if integrity != "ok" {
        anyhow::bail!("升级前数据库备份完整性检查失败：{integrity}");
    }
    prune_database_backups(&backup_dir, 5)?;
    Ok(Some(backup_path))
}

fn prune_database_backups(backup_dir: &std::path::Path, keep: usize) -> anyhow::Result<()> {
    let mut backups = fs::read_dir(backup_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("app-v") && name.ends_with(".sqlite"))
        })
        .collect::<Vec<_>>();
    backups.sort();
    let remove_count = backups.len().saturating_sub(keep);
    for path in backups.into_iter().take(remove_count) {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn scrub_backup_key(conn: &Connection) -> anyhow::Result<()> {
    let has_key_column = {
        let mut stmt = conn.prepare("pragma table_info(ai_config)")?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        columns.iter().any(|name| name == "api_key")
    };
    if !has_key_column {
        return Ok(());
    }
    let secrets: i64 = conn.query_row(
        "select count(*) from ai_config where api_key <> ''",
        [],
        |r| r.get(0),
    )?;
    if secrets == 0 {
        return Ok(());
    }
    conn.pragma_update(None, "secure_delete", "ON")?;
    conn.execute("update ai_config set api_key = '' where api_key <> ''", [])?;
    // Compact the backup so old secret bytes do not remain in freed pages.
    conn.execute_batch("pragma wal_checkpoint(truncate); vacuum;")?;
    Ok(())
}

fn scrub_existing_backups(app_dir: &Path) -> anyhow::Result<()> {
    let dir = app_dir.join("Backups");
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type()?.is_file() || !name.starts_with("app-v") || !name.ends_with(".sqlite")
        {
            continue;
        }
        let path = entry.path();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        let conn = Connection::open(&path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        scrub_backup_key(&conn)?;
    }
    write_restore_notice(&dir)?;
    Ok(())
}

fn write_restore_notice(dir: &Path) -> anyhow::Result<()> {
    fs::write(dir.join("README.txt"), "IM-Board 升级备份保留账号、消息和事项。API 密钥不写入备份；恢复后使用系统安全凭据，或在 AI 设置中重新输入密钥。\n")?;
    Ok(())
}

#[cfg(test)]
mod privacy_tests {
    use super::*;
    #[test]
    fn backup_restores_data_without_storing_legacy_secret_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("create table ai_config(id integer primary key, api_key text); insert into ai_config values(1,'AUDIT_BACKUP_SECRET_487539'); create table fixture(value text); insert into fixture values('preserve this data');").unwrap();
        let path = backup_database_before_upgrade(&conn, dir.path())
            .unwrap()
            .unwrap();
        assert!(!String::from_utf8_lossy(&fs::read(&path).unwrap())
            .contains("AUDIT_BACKUP_SECRET_487539"));
        assert!(path.parent().unwrap().join("README.txt").is_file());
        let restored = Connection::open(&path).unwrap();
        let value: String = restored
            .query_row("select value from fixture", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "preserve this data");
        let original: String = conn
            .query_row("select api_key from ai_config", [], |r| r.get(0))
            .unwrap();
        assert_eq!(original, "AUDIT_BACKUP_SECRET_487539");
        restored
            .execute(
                "update ai_config set api_key='OLD_BACKUP_SECRET_236479'",
                [],
            )
            .unwrap();
        drop(restored);
        scrub_existing_backups(dir.path()).unwrap();
        assert!(
            !String::from_utf8_lossy(&fs::read(path).unwrap()).contains("OLD_BACKUP_SECRET_236479")
        );
    }
}
