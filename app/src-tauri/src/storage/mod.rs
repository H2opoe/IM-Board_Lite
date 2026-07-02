use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64},
    Mutex,
};

use rusqlite::Connection;

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
    pub local_model_server_pid: Mutex<Option<u32>>,
    pub local_model_download_progress: Mutex<Option<LocalModelDownloadProgress>>,
    pub local_model_download_cancel_requested: AtomicBool,
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
        let conn = Connection::open(db_path)?;
        run_migrations(&conn)?;
        ensure_schema_columns(&conn)?;

        Ok(Self {
            db: Mutex::new(conn),
            app_dir,
            cache_dir,
            sync_cancel_requested: AtomicBool::new(false),
            sync_job_running: AtomicBool::new(false),
            sync_bridge_pids: Mutex::new(Vec::new()),
            auto_sync_frequency_minutes: AtomicU64::new(15),
            local_model_server_pid: Mutex::new(None),
            local_model_download_progress: Mutex::new(None),
            local_model_download_cancel_requested: AtomicBool::new(false),
        })
    }
}

fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))?;
    Ok(())
}

fn ensure_schema_columns(conn: &Connection) -> anyhow::Result<()> {
    let mut stmt = conn.prepare("pragma table_info(profiles)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|column| column == "sort_order") {
        conn.execute(
            "alter table profiles add column sort_order integer not null default 0",
            [],
        )?;
    }

    let mut stmt = conn.prepare("pragma table_info(daily_messages)")?;
    let message_columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !message_columns
        .iter()
        .any(|column| column == "topic_summarized_at")
    {
        conn.execute(
            "alter table daily_messages add column topic_summarized_at text",
            [],
        )?;
    }

    let mut stmt = conn.prepare("pragma table_info(ai_config)")?;
    let ai_columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !ai_columns.iter().any(|column| column == "analysis_prompt") {
        conn.execute(
            "alter table ai_config add column analysis_prompt text not null default ''",
            [],
        )?;
    }
    if !ai_columns.iter().any(|column| column == "summary_prompt") {
        conn.execute(
            "alter table ai_config add column summary_prompt text not null default ''",
            [],
        )?;
    }
    if !ai_columns.iter().any(|column| column == "user_prompt") {
        conn.execute(
            "alter table ai_config add column user_prompt text not null default ''",
            [],
        )?;
    }
    if !ai_columns
        .iter()
        .any(|column| column == "analysis_prompt_custom")
    {
        conn.execute(
            "alter table ai_config add column analysis_prompt_custom integer",
            [],
        )?;
    }
    if !ai_columns
        .iter()
        .any(|column| column == "summary_prompt_custom")
    {
        conn.execute(
            "alter table ai_config add column summary_prompt_custom integer",
            [],
        )?;
    }
    if !ai_columns
        .iter()
        .any(|column| column == "analysis_batch_size")
    {
        conn.execute(
            "alter table ai_config add column analysis_batch_size integer not null default 20",
            [],
        )?;
    }

    let mut stmt = conn.prepare("pragma table_info(ai_analysis_runs)")?;
    let ai_run_columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !ai_run_columns
        .iter()
        .any(|column| column == "diagnostic_json")
    {
        conn.execute(
            "alter table ai_analysis_runs add column diagnostic_json text",
            [],
        )?;
    }
    conn.execute_batch(
        "create table if not exists diagnostic_error_events (
           id text primary key,
           source text not null check(source in ('cli', 'bridge', 'ai', 'local_ai_runtime', 'sync')),
           category text not null,
           severity text not null check(severity in ('info', 'warning', 'error')),
           profile_id text,
           platform text,
           operation text not null,
           user_message text not null,
           raw_detail_json text not null default '{}',
           context_json text not null default '{}',
           created_at text not null
         );
         create index if not exists idx_diagnostic_error_events_created
         on diagnostic_error_events(created_at desc);",
    )?;
    Ok(())
}
