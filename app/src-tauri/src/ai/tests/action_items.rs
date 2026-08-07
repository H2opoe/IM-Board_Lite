#[test]
fn persist_analysis_can_write_resolved_reply_as_done() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, ?2, ?3, 'feishu', ?4, ?5, 0, 'friend', '朋友', 1, '09:00', 'text', '你看下可以吗', 'hash-1')",
            params![
                "msg-1",
                "2026-05-01",
                "profile-1",
                "chat-1",
                "测试聊天"
            ],
        )
        .expect("message");

    let analysis = AiAnalysis {
        action_items: vec![AiActionItem {
            item_type: "reply".to_owned(),
            status: Some("done".to_owned()),
            priority: "medium".to_owned(),
            title: "回复确认问题".to_owned(),
            description: "对方询问后，用户已经回复处理。".to_owned(),
            suggested_reply: None,
            chat_id: "chat-1".to_owned(),
            profile_id: None,
            existing_action_item_id: None,
            source_message_ids: vec!["msg-1".to_owned()],
            evidence_summary: "对方询问后已回复。".to_owned(),
            context_incomplete: false,
        }],
        topics: Vec::new(),
        keywords: Vec::new(),
    };

    persist_analysis_across_profiles(&conn, "2026-05-01", &[], analysis).expect("persist analysis");
    let (status, completed_at, first_detected_at, expected_detected_at): (
        String,
        Option<String>,
        String,
        String,
    ) = conn
        .query_row(
            "select status, completed_at, first_detected_at, datetime(1, 'unixepoch', 'localtime')
                 from action_items where type = 'reply'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("action item");
    assert_eq!(status, "done");
    assert!(completed_at.is_some());
    assert_eq!(first_detected_at, expected_detected_at);
}

#[test]
fn persist_analysis_marks_inferred_reply_done_after_user_response() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute_batch(
        "insert into daily_messages(
           id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
           timestamp, time_text, msg_type, content, content_hash
         )
         values
           ('msg-question', '2026-05-06', 'profile-1', 'feishu', 'chat-1', '何玉玲',
            0, 'friend', '何玉玲', 100, '11:23', 'text', '你大概明早几点到东丽，我准备好出门时间。', 'hash-question'),
           ('msg-reply', '2026-05-06', 'profile-1', 'feishu', 'chat-1', '何玉玲',
            0, 'me', 'me', 101, '11:24', 'text', '9点', 'hash-reply'),
           ('msg-ack', '2026-05-06', 'profile-1', 'feishu', 'chat-1', '何玉玲',
            0, 'friend', '何玉玲', 102, '11:24', 'text', '好', 'hash-ack')",
    )
    .expect("messages");

    let analysis = AiAnalysis {
        action_items: vec![AiActionItem {
            item_type: "reply".to_owned(),
            status: Some("open".to_owned()),
            priority: "medium".to_owned(),
            title: "回复母亲关于明天到达时间".to_owned(),
            description: "母亲询问明天早上到达东丽的时间。".to_owned(),
            suggested_reply: None,
            chat_id: "chat-1".to_owned(),
            profile_id: Some("profile-1".to_owned()),
            existing_action_item_id: None,
            source_message_ids: Vec::new(),
            evidence_summary: "何玉玲：你大概明早几点到东丽，我准备好出门时间。".to_owned(),
            context_incomplete: false,
        }],
        topics: Vec::new(),
        keywords: Vec::new(),
    };

    persist_analysis_across_profiles(&conn, "2026-05-06", &[], analysis).expect("persist analysis");
    let (status, source_message_ids, completed_at): (String, String, Option<String>) = conn
        .query_row(
            "select status, source_message_ids, completed_at from action_items where type = 'reply'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("action item");
    assert_eq!(status, "done");
    assert_eq!(source_message_ids, "[\"msg-question\"]");
    assert!(completed_at.is_some());
}

#[test]
fn persist_analysis_does_not_merge_open_items_without_existing_action_item_id() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values('msg-1', '2026-05-01', 'profile-1', 'feishu', 'chat-1', '测试聊天',
                    0, 'friend', '朋友', 1, '09:00', 'text', '请确认合同', 'hash-1')",
        [],
    )
    .expect("message");
    conn.execute(
        "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id,
               chat_name, source_message_ids, evidence_summary, first_detected_at, last_updated_at
             )
             values('act-existing', 'task', 'open', 'medium', '确认合同', '旧事项',
                    'profile-1', 'feishu', 'chat-1', '测试聊天', '[\"old-msg\"]',
                    '旧证据', datetime('now'), datetime('now'))",
        [],
    )
    .expect("existing action item");

    let analysis = AiAnalysis {
        action_items: vec![AiActionItem {
            item_type: "task".to_owned(),
            status: Some("open".to_owned()),
            priority: "medium".to_owned(),
            title: "确认合同".to_owned(),
            description: "新消息要求确认合同。".to_owned(),
            suggested_reply: None,
            chat_id: "chat-1".to_owned(),
            profile_id: None,
            existing_action_item_id: None,
            source_message_ids: vec!["msg-1".to_owned()],
            evidence_summary: "对方要求确认合同。".to_owned(),
            context_incomplete: false,
        }],
        topics: Vec::new(),
        keywords: Vec::new(),
    };

    persist_analysis_across_profiles(&conn, "2026-05-01", &[], analysis).expect("persist analysis");
    let count: i64 = conn
            .query_row(
                "select count(*) from action_items where profile_id = 'profile-1' and chat_id = 'chat-1'",
                [],
                |row| row.get(0),
            )
            .expect("action item count");
    let old_sources: String = conn
        .query_row(
            "select source_message_ids from action_items where id = 'act-existing'",
            [],
            |row| row.get(0),
        )
        .expect("old sources");
    assert_eq!(count, 2);
    assert_eq!(old_sources, "[\"old-msg\"]");
}

#[test]
fn persist_analysis_uses_existing_action_item_without_action_item_group_column() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values('msg-1', '2026-05-01', 'profile-1', 'feishu', 'chat-1', '测试群',
                    1, 'member-1', '成员', 1, '09:00', 'text', '继续确认合同', 'hash-1')",
        [],
    )
    .expect("message");
    conn.execute(
        "insert into action_items(
               id, type, status, priority, title, description, profile_id, platform, chat_id,
               chat_name, source_message_ids, evidence_summary, first_detected_at, last_updated_at
             )
             values('act-existing', 'task', 'open', 'medium', '确认合同', '旧事项',
                    'profile-1', 'feishu', 'chat-1', '测试群', '[\"old-msg\"]',
                    '旧证据', datetime('now'), datetime('now'))",
        [],
    )
    .expect("existing action item");

    let analysis = AiAnalysis {
        action_items: vec![AiActionItem {
            item_type: "task".to_owned(),
            status: Some("open".to_owned()),
            priority: "high".to_owned(),
            title: "确认合同".to_owned(),
            description: "群里继续催促确认合同。".to_owned(),
            suggested_reply: None,
            chat_id: "chat-1".to_owned(),
            profile_id: None,
            existing_action_item_id: Some("act-existing".to_owned()),
            source_message_ids: vec!["msg-1".to_owned()],
            evidence_summary: "群里继续催促。".to_owned(),
            context_incomplete: true,
        }],
        topics: Vec::new(),
        keywords: Vec::new(),
    };

    let persisted = persist_analysis_across_profiles(&conn, "2026-05-01", &[], analysis)
        .expect("persist analysis");
    let count: i64 = conn
        .query_row("select count(*) from action_items", [], |row| row.get(0))
        .expect("action item count");
    let priority: String = conn
        .query_row(
            "select priority from action_items where id = 'act-existing'",
            [],
            |row| row.get(0),
        )
        .expect("updated priority");

    assert_eq!(count, 1);
    assert_eq!(priority, "high");
    assert_eq!(persisted.context_requests.len(), 1);
    assert!(persisted.context_requests[0].is_group);
}

fn test_message(id: String, chat_id: &str) -> AnalysisMessage {
    AnalysisMessage {
        id,
        profile_id: "profile-1".to_owned(),
        platform: "feishu".to_owned(),
        chat_id: chat_id.to_owned(),
        chat_name: chat_id.to_owned(),
        is_group: false,
        timestamp: 1,
        sender_id: "friend".to_owned(),
        sender_name: "朋友".to_owned(),
        is_me: false,
        time_text: "09:00".to_owned(),
        msg_type: "text".to_owned(),
        content: "测试消息".to_owned(),
        partial: false,
    }
}

fn test_keyword_rank(text: &str, score: f64) -> LocalKeywordRank {
    LocalKeywordRank {
        text: text.to_owned(),
        value: LocalKeywordScore {
            score,
            occurrences: 1,
            chat_ids: HashSet::new(),
            context_hits: 0,
            best_source: crate::analysis::local_keywords::LocalKeywordSource::Segment,
            latest_timestamp: 0,
        },
        final_score: score,
    }
}
