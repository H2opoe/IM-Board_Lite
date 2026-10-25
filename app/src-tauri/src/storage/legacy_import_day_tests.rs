use super::*;

fn database() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    super::super::run_migrations(&mut conn).unwrap();
    conn
}

fn source_fixture() -> Connection {
    let conn = database();
    conn.execute_batch("insert into profiles(id,platform,label,config_json,created_at,updated_at) values('p','feishu','fixture','{}','first','last');
        insert into action_items(id,type,status,priority,title,description,profile_id,platform,chat_id,chat_name,first_detected_at,last_updated_at,detected_day) values('a','task','done','medium','fixture','','p','feishu','c','fixture','2026-09-30 01:15:00','last','2026-09-29');").unwrap();
    conn
}

#[test]
fn readonly_v4_import_keeps_unknown_detection_day_and_original_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.sqlite");
    let source = source_fixture();
    source
        .execute("alter table action_items drop column detected_day", [])
        .unwrap();
    source.pragma_update(None, "user_version", 4).unwrap();
    let mut file = Connection::open(&path).unwrap();
    rusqlite::backup::Backup::new(&source, &mut file)
        .unwrap()
        .run_to_completion(128, std::time::Duration::from_millis(1), None)
        .unwrap();
    drop(file);
    let before = std::fs::read(&path).unwrap();
    let mut dest = database();
    import_from_path(&mut dest, &path).unwrap();
    assert_eq!(before, std::fs::read(&path).unwrap());
    let (first, day): (String, Option<String>) = dest
        .query_row(
            "select first_detected_at,detected_day from action_items",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(first, "2026-09-30 01:15:00");
    assert_eq!(day, None);
}

#[test]
fn v5_import_preserves_detection_day_and_future_source_is_refused() {
    let mut source = source_fixture();
    let mut dest = database();
    import_connections(&mut dest, &mut source).unwrap();
    assert_eq!(
        dest.query_row("select detected_day from action_items", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "2026-09-29"
    );
    source.pragma_update(None, "user_version", 6).unwrap();
    let mut empty = database();
    assert!(import_connections(&mut empty, &mut source).is_err());
    assert_eq!(
        empty
            .query_row("select count(*) from profiles", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
