use super::*;

pub(super) fn fixture() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("fixture database");
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("fixture schema");
    conn
}

pub(super) fn insert_message(
    conn: &rusqlite::Connection,
    id: &str,
    platform: &str,
    chat_id: &str,
    sender_id: &str,
) {
    conn.execute(
        "insert into daily_messages(id, day, profile_id, platform, chat_id, chat_name,
          is_group, sender_id, sender_name, timestamp, time_text, msg_type, content, content_hash)
         values (?1, '2026-05-01', ?2, ?2, ?3, '测试群', 1, ?4, '测试成员',
          1, '09:00', 'text', '仓库交付排期请确认。', ?1)",
        params![id, platform, chat_id, sender_id],
    )
    .expect("fixture message");
}

#[test]
fn keyword_refine_scope_payload_carries_each_message_platform() {
    let conn = fixture();
    let platforms = ["wecom", "feishu", "dingtalk"];
    for platform in platforms {
        insert_message(&conn, platform, platform, "shared-id", "normal-sender");
        upsert_keyword_meta(&conn, "2026-05-01", platform, "fixture-v1", "local_final")
            .expect("fixture keyword version");
    }
    let profile_ids = platforms.map(str::to_owned);
    let plan = build_keyword_refine_plan(&conn, "2026-05-01", &profile_ids, 3)
        .expect("fixture plan")
        .expect("messages exist");
    let messages = plan.payload["messages"]
        .as_array()
        .expect("payload messages");
    assert_eq!(messages.len(), 3);
    for platform in platforms {
        let message = messages
            .iter()
            .find(|message| message["id"] == platform)
            .expect("source message retained");
        assert_eq!(message["platform"], platform);
        assert_eq!(message["profileId"], platform);
        assert_eq!(message["chatId"], "shared-id");
    }
}

#[test]
fn keyword_refine_scope_keeps_unknown_source_platform_explicit() {
    let conn = fixture();
    insert_message(
        &conn,
        "unknown-source",
        "vendor_extension",
        "shared-id",
        "normal-sender",
    );
    let messages = load_keyword_refine_messages(&conn, "2026-05-01", "vendor_extension")
        .expect("load fixture messages");
    assert_eq!(messages.len(), 1);
    let payload = serde_json::to_value(&messages[0]).expect("serialize source");
    assert_eq!(payload["platform"], "vendor_extension");
}
