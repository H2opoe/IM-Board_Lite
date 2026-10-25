//! Explicit import from the former shared directory. The source is read-only;
//! credentials and execution state never cross the edition boundary.
use rusqlite::{params, params_from_iter, Connection, OpenFlags, OptionalExtension};
use std::path::Path;

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub profiles: usize,
    pub messages: usize,
    pub actions: usize,
    pub topics: usize,
}
const ALLOWED: &str = "'wecom','feishu','dingtalk'";
const IMPORT_KEY: &str = "legacy_lite_import_v1";

pub fn import_from_path(
    destination: &mut Connection,
    source_path: &Path,
) -> anyhow::Result<ImportSummary> {
    if !source_path.is_file() {
        anyhow::bail!("未找到旧版数据库。原目录未修改。");
    }
    let mut source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    source.busy_timeout(std::time::Duration::from_secs(5))?;
    import_connections(destination, &mut source)
}

fn import_connections(
    destination: &mut Connection,
    source: &mut Connection,
) -> anyhow::Result<ImportSummary> {
    let snapshot = source.transaction()?;
    let version: i64 = snapshot.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if !(1..=5).contains(&version) {
        anyhow::bail!("旧数据库版本 {version} 暂不支持导入，请保留原库。");
    }
    let tx = destination.transaction()?;
    let imported: Option<String> = tx
        .query_row(
            "select value from app_meta where key=?1",
            [IMPORT_KEY],
            |r| r.get(0),
        )
        .optional()?;
    if imported.is_some() {
        anyhow::bail!("旧数据已导入，无需重复导入。");
    }
    for table in ["profiles", "daily_messages", "action_items", "daily_topics"] {
        let count: i64 =
            tx.query_row(&format!("select count(*) from {table}"), [], |r| r.get(0))?;
        if count > 0 {
            anyhow::bail!("当前 Lite 已有数据，导入已停止以避免覆盖。请保留旧目录。");
        }
    }
    let mut summary = ImportSummary::default();
    {
        let mut stmt = snapshot.prepare(&format!("select id,platform,label,config_json,sort_order,created_at,updated_at from profiles where platform in ({ALLOWED})"))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let config: serde_json::Value = serde_json::from_str(&row.get::<_, String>(3)?)?;
            let remark = config.get("remark").and_then(|v| v.as_str()).unwrap_or("");
            summary.profiles += tx.execute("insert into profiles(id,platform,label,enabled,config_json,status,sort_order,created_at,updated_at) values(?1,?2,?3,0,?4,'needs_auth',?5,?6,?7)", params![row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,serde_json::json!({"remark":remark}).to_string(),row.get::<_,i64>(4)?,row.get::<_,String>(5)?,row.get::<_,String>(6)?])?;
        }
    }
    if summary.profiles == 0 {
        anyhow::bail!("旧数据库没有可导入的企业微信、飞书或钉钉账号。");
    }
    summary.messages = copy_rows(&snapshot, &tx, "daily_messages", true)?;
    summary.actions = copy_rows(&snapshot, &tx, "action_items", true)?;
    summary.topics = copy_rows(&snapshot, &tx, "daily_topics", false)?;
    tx.execute(
        "insert into app_meta(key,value,updated_at) values(?1,?2,datetime('now'))",
        params![IMPORT_KEY, serde_json::to_string(&summary)?],
    )?;
    tx.commit()?;
    Ok(summary)
}

fn columns(conn: &Connection, table: &str) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("pragma table_info({table})"))?;
    let values = stmt
        .query_map([], |r| r.get(1))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(values)
}
fn copy_rows(
    source: &Connection,
    destination: &Connection,
    table: &str,
    has_platform: bool,
) -> anyhow::Result<usize> {
    let source_columns = columns(source, table)?;
    if source_columns.is_empty() {
        return Ok(0);
    }
    let common: Vec<String> = columns(destination, table)?
        .into_iter()
        .filter(|c| source_columns.contains(c))
        .collect();
    let fields = common
        .iter()
        .map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");
    let scope = if has_platform {
        format!("and platform in ({ALLOWED}) and platform=(select platform from profiles p where p.id={table}.profile_id)")
    } else {
        String::new()
    };
    let query = format!("select {fields} from {table} where profile_id in (select id from profiles where platform in ({ALLOWED})) {scope}");
    let placeholders = vec!["?"; common.len()].join(",");
    let mut insert = destination.prepare(&format!(
        "insert into {table}({fields}) values({placeholders})"
    ))?;
    let mut select = source.prepare(&query)?;
    let mut rows = select.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let values = (0..common.len())
            .map(|i| row.get::<_, rusqlite::types::Value>(i))
            .collect::<Result<Vec<_>, _>>()?;
        count += insert.execute(params_from_iter(values))?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        super::super::run_migrations(&mut conn).unwrap();
        conn
    }
    fn source_fixture() -> Connection {
        let conn = database();
        // Synthetic unsupported connector verifies the allowlist without shipping its implementation.
        conn.pragma_update(None, "ignore_check_constraints", true)
            .unwrap();
        for (id, platform) in [("keep", "feishu"), ("exclude", "unsupported")] {
            conn.execute("insert into profiles(id,platform,label,config_json,created_at,updated_at) values(?1,?2,'fixture','{\"remark\":\"kept\",\"token\":\"SECRET_FIXTURE\",\"configPath\":\"legacy\"}','created','updated')",params![id,platform]).unwrap();
            conn.execute("insert into daily_messages(id,day,profile_id,platform,chat_id,chat_name,sender_id,sender_name,timestamp,time_text,msg_type,content,content_hash) values(?1,'2026-01-01',?1,?2,'chat','chat','sender','sender',1,'time','text','fixture','hash')",params![id,platform]).unwrap();
            conn.execute("insert into action_items(id,type,status,priority,title,description,profile_id,platform,chat_id,chat_name,first_detected_at,last_updated_at) values(?1,'task','open','high','keep task','fixture',?1,?2,'chat','chat','first','last')",params![id,platform]).unwrap();
        }
        conn.execute(
            "insert into ai_config(id,api_key,updated_at) values(1,'PRIVATE_KEY_FIXTURE','now')",
            [],
        )
        .unwrap();
        conn
    }
    #[test]
    fn imports_only_supported_history_without_credentials_or_execution_state() {
        let mut source = source_fixture();
        let mut dest = database();
        let summary = import_connections(&mut dest, &mut source).unwrap();
        assert_eq!(
            (summary.profiles, summary.messages, summary.actions),
            (1, 1, 1)
        );
        let (config, enabled, status): (String, i64, String) = dest
            .query_row("select config_json,enabled,status from profiles", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&config).unwrap(),
            serde_json::json!({"remark":"kept"})
        );
        let secret_count: i64 = dest
            .query_row(
                "select count(*) from ai_config where api_key <> ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(secret_count, 0);
        assert_eq!(enabled, 0);
        assert_eq!(status, "needs_auth");
        let original: String = source
            .query_row(
                "select config_json from profiles where id='keep'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(original.contains("SECRET_FIXTURE"));
        assert!(import_connections(&mut dest, &mut source).is_err());
        let count: i64 = dest
            .query_row("select count(*) from profiles", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
    #[test]
    fn failed_copy_rolls_back_every_imported_row() {
        let mut source = source_fixture();
        let mut dest = database();
        source
            .execute("update action_items set status='unsupported'", [])
            .unwrap();
        assert!(import_connections(&mut dest, &mut source).is_err());
        for table in ["profiles", "daily_messages", "action_items"] {
            let count: i64 = dest
                .query_row(&format!("select count(*) from {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0);
        }
        let marker: i64 = dest
            .query_row(
                "select count(*) from app_meta where key=?1",
                [IMPORT_KEY],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(marker, 0);
    }
    #[test]
    fn file_source_is_read_only_and_future_versions_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.sqlite");
        let mut fixture = source_fixture();
        let mut file = Connection::open(&path).unwrap();
        rusqlite::backup::Backup::new(&fixture, &mut file)
            .unwrap()
            .run_to_completion(128, std::time::Duration::from_millis(1), None)
            .unwrap();
        drop(file);
        let before = std::fs::read(&path).unwrap();
        let mut dest = database();
        import_from_path(&mut dest, &path).unwrap();
        assert_eq!(before, std::fs::read(path).unwrap());
        fixture.pragma_update(None, "user_version", 99).unwrap();
        assert!(import_connections(&mut database(), &mut fixture).is_err());
    }
}

#[cfg(test)]
mod conflict_tests {
    use super::*;
    #[test]
    fn existing_lite_accounts_are_never_overwritten() {
        let mut source = Connection::open_in_memory().unwrap();
        let mut destination = Connection::open_in_memory().unwrap();
        super::super::run_migrations(&mut source).unwrap();
        super::super::run_migrations(&mut destination).unwrap();
        destination.execute("insert into profiles(id,platform,label,created_at,updated_at) values('existing','feishu','preserved','old','old')",[]).unwrap();
        assert!(import_connections(&mut destination, &mut source).is_err());
        let label: String = destination
            .query_row("select label from profiles where id='existing'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(label, "preserved");
    }
}

#[cfg(test)]
#[path = "legacy_import_day_tests.rs"]
mod action_day_tests;
