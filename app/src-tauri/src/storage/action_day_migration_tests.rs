use super::*;

const PREVIOUS_SCHEMA_VERSION: i64 = 3;

fn previous_database(version: i64) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .unwrap();
    if version >= 2 {
        migrate_schema_v2(&conn).unwrap();
    }
    if version >= 3 {
        migrate_schema_v3(&conn).unwrap();
    }
    conn.pragma_update(None, "user_version", version).unwrap();
    conn.execute("insert into action_items(id,type,status,priority,title,description,profile_id,platform,chat_id,chat_name,first_detected_at,last_updated_at) values('old','task','done','medium','fixture','','p','wecom','c','fixture','2026-09-30 01:15:00','2026-09-30T01:15:00+08:00')",[]).unwrap();
    conn
}

fn schema(conn: &Connection) -> Vec<(String, String)> {
    conn.prepare("select name,sql from sqlite_master where sql is not null order by name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn every_supported_old_version_upgrades_without_guessing_detection_day() {
    let mut empty = Connection::open_in_memory().unwrap();
    run_migrations(&mut empty).unwrap();
    for version in 0..=PREVIOUS_SCHEMA_VERSION {
        let mut conn = previous_database(version);
        run_migrations(&mut conn).unwrap();
        let (first, updated, detected): (String, String, Option<String>) = conn
            .query_row("select first_detected_at,last_updated_at,detected_day from action_items where id='old'", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)))
            .unwrap();
        assert_eq!(first, "2026-09-30 01:15:00");
        assert_eq!(updated, "2026-09-30T01:15:00+08:00");
        assert_eq!(detected, None);
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            5
        );
        run_migrations(&mut conn).unwrap();
        assert_eq!(
            conn.query_row("select count(*) from action_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

#[test]
fn failed_v5_step_rolls_back_the_entire_old_upgrade() {
    let mut conn = previous_database(1);
    conn.execute("drop table action_items", []).unwrap();
    let before = schema(&conn);
    assert!(run_migrations(&mut conn).is_err());
    assert_eq!(schema(&conn), before);
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(conn.is_autocommit());
}

#[test]
fn future_schema_is_refused_without_schema_or_row_changes() {
    let mut conn = previous_database(PREVIOUS_SCHEMA_VERSION);
    conn.pragma_update(None, "user_version", 6).unwrap();
    let before = schema(&conn);
    assert!(run_migrations(&mut conn)
        .unwrap_err()
        .to_string()
        .contains("高于"));
    assert_eq!(schema(&conn), before);
    assert_eq!(
        conn.query_row("select title from action_items", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "fixture"
    );
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        6
    );
}

#[test]
fn upgrade_backup_retains_the_previous_restorable_schema() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = previous_database(PREVIOUS_SCHEMA_VERSION);
    let backup = backup::backup_database_before_upgrade(&conn, dir.path())
        .unwrap()
        .unwrap();
    run_migrations(&mut conn).unwrap();
    conn.execute(
        "update action_items set title='after upgrade',detected_day='2026-09-30'",
        [],
    )
    .unwrap();
    let restored = Connection::open(backup).unwrap();
    assert_eq!(
        restored
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        PREVIOUS_SCHEMA_VERSION
    );
    assert_eq!(
        restored
            .query_row("select title from action_items", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "fixture"
    );
    assert!(restored
        .prepare("select detected_day from action_items")
        .is_err());
    assert_eq!(
        restored
            .pragma_query_value(None, "integrity_check", |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn failed_backup_leaves_the_previous_database_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Backups"), "owned fixture blocks directory").unwrap();
    let conn = previous_database(PREVIOUS_SCHEMA_VERSION);
    let before = schema(&conn);
    assert!(backup::backup_database_before_upgrade(&conn, dir.path()).is_err());
    assert_eq!(schema(&conn), before);
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        PREVIOUS_SCHEMA_VERSION
    );
    assert_eq!(
        conn.query_row("select title from action_items", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "fixture"
    );
    assert!(conn
        .prepare("select detected_day from action_items")
        .is_err());
    assert!(conn.is_autocommit());
}

#[test]
fn schema_four_follows_the_edition_upgrade_policy() {
    if PREVIOUS_SCHEMA_VERSION == 4 {
        let mut conn = previous_database(4);
        run_migrations(&mut conn).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            5
        );
        return;
    }
    let mut conn = previous_database(4);
    let before = schema(&conn);
    assert!(run_migrations(&mut conn)
        .unwrap_err()
        .to_string()
        .contains("显式导入"));
    assert_eq!(schema(&conn), before);
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
}
