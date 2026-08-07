#[test]
fn filter_analysis_messages_skips_low_signal_chats() {
    let mut message = test_message("msg-1".to_owned(), "chat-a");
    message.content = "哈哈哈哈，天气也太热了".to_owned();

    assert!(filter_analysis_messages(vec![message]).is_empty());
}

#[test]
fn filter_analysis_messages_keeps_self_completion_evidence() {
    let mut message = test_message("msg-1".to_owned(), "chat-a");
    message.sender_id = "me".to_owned();
    message.sender_name = "我".to_owned();
    message.is_me = true;
    message.content = "好的，已经处理了".to_owned();

    assert_eq!(filter_analysis_messages(vec![message]).len(), 1);
}

#[test]
fn filter_analysis_messages_skips_call_records() {
    let mut message = test_message("msg-1".to_owned(), "chat-a");
    message.content = "[语音通话] 通话时长 00:12".to_owned();

    assert!(filter_analysis_messages(vec![message]).is_empty());
}

#[test]
fn summary_candidates_skip_call_records() {
    let mut call_record = test_message("msg-1".to_owned(), "chat-a");
    call_record.content = "[视频通话] 通话时长 10:03".to_owned();

    let mut business_message = test_message("msg-2".to_owned(), "chat-a");
    business_message.content = "客户投诉交付延迟，需要今天跟进处理".to_owned();

    let candidates = summary_candidates_from_messages(vec![call_record, business_message]);

    assert!(!candidates.iter().any(|candidate| {
        candidate.title_hint.contains("通话")
            || candidate
                .snippets
                .iter()
                .any(|snippet| snippet.contains("通话"))
    }));
    assert!(!candidates.is_empty());
}

#[test]
fn summary_candidates_skip_group_membership_notices() {
    let mut join_notice = test_message("msg-1".to_owned(), "chat-a");
    join_notice.content = "Github开发者企微交流群成员通过王潇咏分享的二维码加入群聊".to_owned();

    let mut business_message = test_message("msg-2".to_owned(), "chat-a");
    business_message.content = "客户投诉交付延迟，需要今天跟进处理".to_owned();

    let candidates = summary_candidates_from_messages(vec![join_notice, business_message]);

    assert!(!candidates.iter().any(|candidate| {
        candidate.title_hint.contains("加入群聊")
            || candidate
                .snippets
                .iter()
                .any(|snippet| snippet.contains("加入群聊") || snippet.contains("二维码加入"))
    }));
    assert!(!candidates.is_empty());
}

#[test]
fn dashboard_topic_filter_rejects_group_membership_notices() {
    assert!(is_disallowed_dashboard_topic(
        "Github开发者企微交流群成员通过王潇咏分享的二维码加入",
        "多名成员通过扫描王潇咏分享的二维码加入群聊"
    ));
    assert!(is_disallowed_dashboard_topic(
        "群成员变动",
        "[系统] 张三被邀请加入群聊，李四退出群聊"
    ));
}

#[test]
fn summary_candidates_do_not_premerge_generic_buying_across_chats() {
    let mut partner_message = test_message("msg-1".to_owned(), "partner-chat");
    partner_message.chat_name = "女朋友".to_owned();
    partner_message.content = "你昨天答应买东西，今天到底有没有买？".to_owned();

    let mut procurement_message = test_message("msg-2".to_owned(), "company-chat");
    procurement_message.chat_name = "公司采购群".to_owned();
    procurement_message.is_group = true;
    procurement_message.content = "办公用品采购的东西有没有买，供应商那边等确认。".to_owned();

    let candidates = summary_candidates_from_messages(vec![partner_message, procurement_message]);
    let buy_candidates = candidates
        .iter()
        .filter(|candidate| {
            candidate.title_hint.contains("买")
                || candidate
                    .keywords
                    .iter()
                    .any(|keyword| keyword.contains("买"))
                || candidate
                    .snippets
                    .iter()
                    .any(|snippet| snippet.contains("有没有买"))
        })
        .collect::<Vec<_>>();

    assert!(
        buy_candidates.len() >= 2,
        "generic buying topic should stay split by chat before AI summary; got {candidates:?}"
    );
    assert!(
            buy_candidates
                .iter()
                .all(|candidate| candidate.source_chats.len() == 1),
            "generic buying candidates should not contain multiple source chats; got {buy_candidates:?}"
        );
}

#[test]
fn load_summary_candidates_only_reads_unsummarized_messages() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    for (id, content, summarized_at) in [
        (
            "msg-old",
            "客户投诉交付延迟，需要今天跟进处理",
            "datetime('now')",
        ),
        ("msg-new", "客户投诉交付质量，需要今天跟进处理", "null"),
    ] {
        conn.execute(
            &format!(
                "insert into daily_messages(
                       id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id,
                       sender_name, timestamp, time_text, msg_type, content, content_hash,
                       topic_summarized_at
                     )
                     values(?1, '2026-05-01', 'profile-1', 'feishu', 'chat-1', '测试群', 1,
                            'u-1', '客户', 1, '09:00', 'text', ?2, ?3, {summarized_at})"
            ),
            params![id, content, format!("hash-{id}")],
        )
        .expect("message");
    }

    let candidates = load_summary_candidates(&conn, "2026-05-01", "profile-1").expect("candidates");
    let source_ids = candidates
        .iter()
        .flat_map(|candidate| candidate.source_message_ids.iter().cloned())
        .collect::<HashSet<_>>();

    assert!(source_ids.contains("msg-new"));
    assert!(!source_ids.contains("msg-old"));
}

#[test]
fn default_prompt_uses_explicit_existing_action_item_id() {
    assert!(DEFAULT_ANALYSIS_PROMPT.contains("existingActionItemId"));
    assert!(BATCH_DEDUP_PROMPT.contains("existingActionItemId"));
}

#[test]
fn default_prompts_require_primary_chat_language() {
    assert!(DEFAULT_ANALYSIS_PROMPT.contains("主要语言"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("主要语言"));
}

#[test]
fn summary_prompt_prevents_generic_cross_scene_merges() {
    assert!(DEFAULT_SUMMARY_PROMPT.contains("不能只因为共享"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("女朋友让我买东西"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("公司群讨论采购是否已买"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("existingTopics"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("sourceMessageIds"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("旧话题去重合并规则"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("旧关键词处理规则"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("旧 sourceMessageIds 由系统自动保留并合并"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("系统会自动合并 existingTopics.sourceMessageIds"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("keywordRefine"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("keywordRefine.messages"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("至少对应 3 条有效消息"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("scoreMultiplier"));
    assert!(!DEFAULT_SUMMARY_PROMPT.contains("keywordRefine.candidates"));
}
