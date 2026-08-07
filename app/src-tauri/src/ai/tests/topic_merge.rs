#[test]
fn merge_incremental_summary_topics_extends_existing_count() {
    let existing_topics = vec![SummaryTopicContext {
        id: "topic-send-dfw".to_owned(),
        title: "5月14日早上5:40到DFW一人送机需求".to_owned(),
        summary: "用户发布送机需求".to_owned(),
        count: 2,
        source_chats: vec![SummarySourceChat {
            chat_name: "Baylor 生活群".to_owned(),
            is_group: true,
        }],
        source_message_ids: vec!["old-1".to_owned(), "old-2".to_owned()],
    }];
    let candidates = vec![SummaryCandidate {
        id: "candidate-1".to_owned(),
        title_hint: "送机".to_owned(),
        keywords: vec!["送机".to_owned()],
        count: 2,
        source_chats: vec![SummarySourceChat {
            chat_name: "Baylor 生活群".to_owned(),
            is_group: true,
        }],
        source_message_ids: vec!["new-1".to_owned(), "new-2".to_owned()],
        snippets: vec!["找送机 5月14号早上5:40出发到dfw一人".to_owned()],
        risk: false,
    }];
    let mut summary = AiSummary {
        topics: vec![serde_json::json!({
            "id": "topic-send-dfw",
            "title": "5月14日早上5:40到DFW一人送机需求",
            "summary": "用户发布送机需求",
            "count": 16,
            "sourceMessageIds": ["new-1", "new-1", "new-2", "fake-msg"],
            "sourceChats": [{ "chatName": "Baylor 生活群", "isGroup": true }]
        })],
        keywords: Vec::new(),
    };

    merge_incremental_summary_topics(&mut summary, &existing_topics, &candidates);

    assert_eq!(summary.topics[0]["count"], serde_json::json!(4));
    assert_eq!(
        summary.topics[0]["sourceMessageIds"],
        serde_json::json!(["new-1", "new-2", "old-1", "old-2"])
    );
}

#[test]
fn merge_incremental_summary_topics_combines_duplicate_ai_topics() {
    let candidates = vec![
        SummaryCandidate {
            id: "candidate-1".to_owned(),
            title_hint: "应用宝Mac公测体验群新成员邀请".to_owned(),
            keywords: vec!["应用宝Mac".to_owned()],
            count: 2,
            source_chats: vec![SummarySourceChat {
                chat_name: "应用宝Mac公测体验群".to_owned(),
                is_group: true,
            }],
            source_message_ids: vec!["msg-1".to_owned(), "msg-2".to_owned()],
            snippets: vec!["欢迎新成员加入应用宝Mac公测体验群".to_owned()],
            risk: false,
        },
        SummaryCandidate {
            id: "candidate-2".to_owned(),
            title_hint: "应用宝Mac公测体验群新成员邀请".to_owned(),
            keywords: vec!["应用宝Mac".to_owned()],
            count: 2,
            source_chats: vec![SummarySourceChat {
                chat_name: "应用宝Mac公测体验群".to_owned(),
                is_group: true,
            }],
            source_message_ids: vec!["msg-3".to_owned(), "msg-4".to_owned()],
            snippets: vec!["邀请M芯片Mac用户成为首批原生体验用户".to_owned()],
            risk: false,
        },
    ];
    let mut summary = AiSummary {
        topics: vec![
            serde_json::json!({
                "id": "topic-mac-a",
                "title": "应用宝Mac公测体验群新成员邀请及参与方式",
                "summary": "群内欢迎新成员加入应用宝Mac公测体验群",
                "count": 35,
                "sourceMessageIds": ["msg-1", "msg-2"],
                "sourceChats": [{ "chatName": "应用宝Mac公测体验群", "isGroup": true }]
            }),
            serde_json::json!({
                "id": "topic-mac-b",
                "title": "应用宝Mac公测体验群新成员邀请及参与方式",
                "summary": "群内欢迎新成员加入应用宝Mac公测体验群",
                "count": 17,
                "sourceMessageIds": ["msg-3", "msg-4"],
                "sourceChats": [{ "chatName": "应用宝Mac公测体验群", "isGroup": true }]
            }),
        ],
        keywords: Vec::new(),
    };

    merge_incremental_summary_topics(&mut summary, &[], &candidates);

    assert_eq!(summary.topics.len(), 1);
    assert_eq!(summary.topics[0]["count"], serde_json::json!(4));
    assert_eq!(
        summary.topics[0]["sourceMessageIds"],
        serde_json::json!(["msg-1", "msg-2", "msg-3", "msg-4"])
    );
}

#[test]
fn merge_incremental_summary_topics_keeps_existing_when_ids_omitted() {
    let existing_topics = vec![SummaryTopicContext {
        id: "topic-send-dfw".to_owned(),
        title: "5月14日早上5:40到DFW一人送机需求".to_owned(),
        summary: "用户发布送机需求".to_owned(),
        count: 2,
        source_chats: vec![SummarySourceChat {
            chat_name: "Baylor 生活群".to_owned(),
            is_group: true,
        }],
        source_message_ids: vec!["old-1".to_owned(), "old-2".to_owned()],
    }];
    let candidates = vec![SummaryCandidate {
        id: "candidate-1".to_owned(),
        title_hint: "送机".to_owned(),
        keywords: vec!["送机".to_owned()],
        count: 3,
        source_chats: vec![SummarySourceChat {
            chat_name: "Baylor 生活群".to_owned(),
            is_group: true,
        }],
        source_message_ids: vec!["msg-1".to_owned(), "msg-2".to_owned(), "msg-3".to_owned()],
        snippets: vec!["找送机 5月14号早上5:40出发到dfw一人".to_owned()],
        risk: false,
    }];
    let mut summary = AiSummary {
        topics: vec![serde_json::json!({
            "id": "topic-send-dfw",
            "title": "5月14日早上5:40到DFW一人送机需求",
            "summary": "用户发布送机需求",
            "count": 16,
            "sourceChats": [{ "chatName": "Baylor 生活群", "isGroup": true }]
        })],
        keywords: Vec::new(),
    };

    merge_incremental_summary_topics(&mut summary, &existing_topics, &candidates);

    assert_eq!(summary.topics[0]["count"], serde_json::json!(2));
    assert_eq!(
        summary.topics[0]["sourceMessageIds"],
        serde_json::json!(["old-1", "old-2"])
    );
}

#[test]
fn merge_incremental_summary_topics_keeps_existing_topics_omitted_by_ai() {
    let existing_topics = vec![SummaryTopicContext {
        id: "topic-existing".to_owned(),
        title: "旧话题".to_owned(),
        summary: "旧摘要".to_owned(),
        count: 2,
        source_chats: vec![SummarySourceChat {
            chat_name: "旧群".to_owned(),
            is_group: true,
        }],
        source_message_ids: vec!["old-1".to_owned(), "old-2".to_owned()],
    }];
    let candidates = vec![SummaryCandidate {
        id: "candidate-1".to_owned(),
        title_hint: "新话题".to_owned(),
        keywords: vec!["新话题".to_owned()],
        count: 1,
        source_chats: vec![SummarySourceChat {
            chat_name: "新群".to_owned(),
            is_group: true,
        }],
        source_message_ids: vec!["new-1".to_owned()],
        snippets: vec!["新话题讨论".to_owned()],
        risk: false,
    }];
    let mut summary = AiSummary {
        topics: vec![serde_json::json!({
            "id": "topic-new",
            "title": "新话题",
            "summary": "新摘要",
            "count": 1,
            "sourceMessageIds": ["new-1"],
            "sourceChats": [{ "chatName": "新群", "isGroup": true }]
        })],
        keywords: Vec::new(),
    };

    merge_incremental_summary_topics(&mut summary, &existing_topics, &candidates);

    let ids = summary
        .topics
        .iter()
        .filter_map(|topic| topic.get("id").and_then(|value| value.as_str()))
        .collect::<HashSet<_>>();
    assert!(ids.contains("topic-existing"));
    assert!(ids.contains("topic-new"));
}
