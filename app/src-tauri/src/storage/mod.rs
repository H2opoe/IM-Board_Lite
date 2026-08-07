use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64},
    Mutex,
};

use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Notify;

pub mod models;
use models::LocalModelDownloadProgress;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub app_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub sync_cancel_requested: AtomicBool,
    pub sync_job_running: AtomicBool,
    pub sync_bridge_pids: Mutex<Vec<u32>>,
    pub auto_sync_frequency_minutes: AtomicU64,
    pub auto_sync_schedule_changed: Notify,
    pub local_model_runtime: crate::local_runtime_supervisor::LocalRuntimeSupervisor,
    pub local_model_download_progress: Mutex<Option<LocalModelDownloadProgress>>,
    pub local_model_download_cancel_requested: AtomicBool,
    pub system_capabilities: crate::system_capabilities::SystemCapabilities,
}

const APP_DATA_DIR_NAME: &str = "IMBoard";

impl AppState {
    pub fn new() -> anyhow::Result<Self> {
        let app_dir = dirs::data_dir()
            .unwrap_or_else(|| std::env::current_dir().unwrap())
            .join(APP_DATA_DIR_NAME);
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| app_dir.join("Caches"))
            .join(APP_DATA_DIR_NAME);

        fs::create_dir_all(app_dir.join("Profiles"))?;
        fs::create_dir_all(&cache_dir)?;

        let db_path = app_dir.join("app.sqlite");
        let mut conn = Connection::open(&db_path)?;
        backup_database_before_upgrade(&conn, &app_dir)?;
        run_migrations(&mut conn)?;
        if let Err(error) = crate::security::credentials::migrate_legacy_ai_api_key(&conn) {
            eprintln!(
                "{}",
                crate::security::sanitize_log(&format!("迁移旧版AI凭据失败：{error:#}"))
            );
        }
        let auto_sync_frequency_minutes = stored_auto_sync_frequency_minutes(&conn)?
            .unwrap_or(DEFAULT_AUTO_SYNC_FREQUENCY_MINUTES);
        let system_capabilities = crate::system_capabilities::SystemCapabilities::detect(&app_dir);

        Ok(Self {
            db: Mutex::new(conn),
            app_dir,
            cache_dir,
            sync_cancel_requested: AtomicBool::new(false),
            sync_job_running: AtomicBool::new(false),
            sync_bridge_pids: Mutex::new(Vec::new()),
            auto_sync_frequency_minutes: AtomicU64::new(auto_sync_frequency_minutes),
            auto_sync_schedule_changed: Notify::new(),
            local_model_runtime: crate::local_runtime_supervisor::LocalRuntimeSupervisor::new()?,
            local_model_download_progress: Mutex::new(None),
            local_model_download_cancel_requested: AtomicBool::new(false),
            system_capabilities,
        })
    }
}

pub const DEFAULT_AUTO_SYNC_FREQUENCY_MINUTES: u64 = 15;
const AUTO_SYNC_FREQUENCY_META_KEY: &str = "auto_sync_frequency_minutes";

pub fn initialize_auto_sync_frequency_minutes(
    conn: &Connection,
    legacy_minutes: u64,
) -> anyhow::Result<u64> {
    if let Some(minutes) = stored_auto_sync_frequency_minutes(conn)? {
        return Ok(minutes);
    }
    let minutes = legacy_minutes.max(1);
    persist_auto_sync_frequency_minutes(conn, minutes)?;
    Ok(minutes)
}

pub fn persist_auto_sync_frequency_minutes(conn: &Connection, minutes: u64) -> anyhow::Result<u64> {
    let minutes = minutes.max(1);
    conn.execute(
        "insert into app_meta(key, value, updated_at)
         values(?1, ?2, datetime('now'))
         on conflict(key) do update set value = excluded.value, updated_at = excluded.updated_at",
        params![AUTO_SYNC_FREQUENCY_META_KEY, minutes.to_string()],
    )?;
    Ok(minutes)
}

fn stored_auto_sync_frequency_minutes(conn: &Connection) -> anyhow::Result<Option<u64>> {
    let value = conn
        .query_row(
            "select value from app_meta where key = ?1",
            params![AUTO_SYNC_FREQUENCY_META_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(value
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0))
}

const LATEST_SCHEMA_VERSION: i64 = 2;

fn backup_database_before_upgrade(
    conn: &Connection,
    app_dir: &std::path::Path,
) -> anyhow::Result<Option<PathBuf>> {
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
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let backup_path = backup_dir.join(format!(
        "app-v{current_version}-before-v{LATEST_SCHEMA_VERSION}-{timestamp}.sqlite"
    ));
    let mut destination = Connection::open(&backup_path)?;
    let backup = rusqlite::backup::Backup::new(conn, &mut destination)?;
    backup.run_to_completion(128, std::time::Duration::from_millis(25), None)?;
    drop(backup);
    destination.execute_batch("pragma integrity_check;")?;
    Ok(Some(backup_path))
}

fn run_migrations(conn: &mut Connection) -> anyhow::Result<()> {
    conn.execute_batch("pragma journal_mode = WAL; pragma foreign_keys = ON;")?;
    let current_version =
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?;
    if current_version > LATEST_SCHEMA_VERSION {
        anyhow::bail!(
            "数据库版本 {current_version} 高于当前应用支持的版本 {LATEST_SCHEMA_VERSION}，请升级应用后重试。"
        );
    }
    if current_version == LATEST_SCHEMA_VERSION {
        return Ok(());
    }

    let tx = conn.transaction()?;
    if current_version < 1 {
        tx.execute_batch(include_str!("../../migrations/001_init.sql"))?;
        tx.pragma_update(None, "user_version", 1)?;
    }
    if current_version < 2 {
        migrate_schema_v2(&tx)?;
        tx.pragma_update(None, "user_version", 2)?;
    }
    tx.commit()?;
    Ok(())
}

fn migrate_schema_v2(conn: &Connection) -> anyhow::Result<()> {
    ensure_column(
        conn,
        "profiles",
        "sort_order",
        "alter table profiles add column sort_order integer not null default 0",
    )?;
    ensure_column(
        conn,
        "daily_messages",
        "topic_summarized_at",
        "alter table daily_messages add column topic_summarized_at text",
    )?;
    for (column, sql) in [
        (
            "analysis_prompt",
            "alter table ai_config add column analysis_prompt text not null default ''",
        ),
        (
            "summary_prompt",
            "alter table ai_config add column summary_prompt text not null default ''",
        ),
        (
            "user_prompt",
            "alter table ai_config add column user_prompt text not null default ''",
        ),
        (
            "analysis_prompt_custom",
            "alter table ai_config add column analysis_prompt_custom integer",
        ),
        (
            "summary_prompt_custom",
            "alter table ai_config add column summary_prompt_custom integer",
        ),
        (
            "analysis_batch_size",
            "alter table ai_config add column analysis_batch_size integer not null default 20",
        ),
    ] {
        ensure_column(conn, "ai_config", column, sql)?;
    }
    ensure_column(
        conn,
        "ai_analysis_runs",
        "diagnostic_json",
        "alter table ai_analysis_runs add column diagnostic_json text",
    )?;
    conn.execute_batch(include_str!("../../migrations/002_reliability.sql"))?;
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    migration_sql: &str,
) -> anyhow::Result<()> {
    let mut stmt = conn.prepare(&format!("pragma table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|existing| existing == column) {
        conn.execute(migration_sql, [])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&mut conn).expect("run migrations");
        conn
    }

    #[test]
    fn migrations_set_the_latest_schema_version() {
        let conn = memory_db();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read schema version");
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn migrates_legacy_columns_in_one_versioned_upgrade() {
        let mut conn = Connection::open_in_memory().expect("open memory db");
        conn.execute_batch(
            "create table profiles(id text primary key);
             create table daily_messages(id text primary key);
             create table ai_config(id integer primary key);
             create table ai_analysis_runs(id text primary key, created_at text not null);
             pragma user_version = 1;",
        )
        .expect("create legacy schema");

        run_migrations(&mut conn).expect("upgrade legacy schema");

        for (table, column) in [
            ("profiles", "sort_order"),
            ("daily_messages", "topic_summarized_at"),
            ("ai_config", "analysis_prompt"),
            ("ai_config", "summary_prompt"),
            ("ai_config", "user_prompt"),
            ("ai_config", "analysis_prompt_custom"),
            ("ai_config", "summary_prompt_custom"),
            ("ai_config", "analysis_batch_size"),
            ("ai_analysis_runs", "diagnostic_json"),
        ] {
            let mut stmt = conn
                .prepare(&format!("pragma table_info({table})"))
                .expect("inspect table");
            let columns = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .expect("query columns")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect columns");
            assert!(columns.iter().any(|existing| existing == column));
        }
    }

    #[test]
    fn file_upgrade_creates_a_readable_backup_before_migration() {
        let temp = tempfile::tempdir().expect("temporary app dir");
        let database_path = temp.path().join("app.sqlite");
        let mut conn = Connection::open(&database_path).expect("open file database");
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .expect("legacy schema");
        conn.execute(
            "insert into app_meta(key, value, updated_at) values('upgrade-proof', 'kept', datetime('now'))",
            [],
        )
        .expect("legacy data");
        conn.pragma_update(None, "user_version", 1)
            .expect("legacy version");

        let backup_path = backup_database_before_upgrade(&conn, temp.path())
            .expect("backup old database")
            .expect("backup path");
        run_migrations(&mut conn).expect("upgrade database");

        let backup = Connection::open(backup_path).expect("open backup");
        let value: String = backup
            .query_row(
                "select value from app_meta where key = 'upgrade-proof'",
                [],
                |row| row.get(0),
            )
            .expect("read backup data");
        assert_eq!(value, "kept");
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("upgraded version");
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn rejects_databases_created_by_a_newer_app_version() {
        let mut conn = Connection::open_in_memory().expect("open memory db");
        conn.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION + 1)
            .expect("set future schema");
        let error = run_migrations(&mut conn).expect_err("future schema must fail");
        assert!(error.to_string().contains("高于当前应用支持"));
    }

    #[test]
    fn initializes_auto_sync_frequency_from_legacy_value_once() {
        let conn = memory_db();
        assert_eq!(
            initialize_auto_sync_frequency_minutes(&conn, 45).expect("initialize frequency"),
            45
        );
        assert_eq!(
            initialize_auto_sync_frequency_minutes(&conn, 10).expect("reload frequency"),
            45
        );
    }

    #[test]
    fn persists_normalized_auto_sync_frequency() {
        let conn = memory_db();
        assert_eq!(
            persist_auto_sync_frequency_minutes(&conn, 0).expect("persist frequency"),
            1
        );
        assert_eq!(stored_auto_sync_frequency_minutes(&conn).unwrap(), Some(1));
    }
}
