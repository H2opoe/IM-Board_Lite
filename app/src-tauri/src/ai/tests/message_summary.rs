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
    assert!(!is_disallowed_dashboard_topic(
        "邀请客户参与项目交付",
        "明天邀请客户加入群聊讨论交付"
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

#[test]
fn message_type_boundary_keeps_business_discussions_and_excludes_typed_events() {
    let conn = rusqlite::Connection::open_in_memory().expect("fixture database");
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    for platform in ["feishu", "dingtalk", "wecom"] {
        for (suffix, msg_type, raw_type, raw_json, content) in [
            (
                "announcement",
                "text",
                Some("text"),
                "{}",
                "请确认群公告的发布时间，今天需要跟进交付排期",
            ),
            (
                "interaction",
                "text",
                Some("text"),
                "{}",
                "拍一拍提醒功能今天需要完成交付，请确认",
            ),
            (
                "meeting",
                "text",
                Some("text"),
                "{}",
                "会议安排与项目交付排期今天需要确认",
            ),
            (
                "welcome",
                "text",
                Some("text"),
                "{}",
                "请设计欢迎新人引导流程",
            ),
            (
                "invite",
                "text",
                Some("text"),
                "{}",
                "明天邀请客户加入群聊讨论交付",
            ),
            (
                "compatibility",
                "text",
                Some("text"),
                "{}",
                "当前版本不支持显示内容，我们下周修复",
            ),
            (
                "checkin-rule",
                "text",
                Some("text"),
                "{}",
                "签到奖励积分规则需要产品确认",
            ),
            (
                "raw-system",
                "text",
                Some("system"),
                "{}",
                "今天需要完成系统维护任务",
            ),
            (
                "nested-system",
                "text",
                None,
                r#"{"raw":{"msg_type":"system"}}"#,
                "今天需要完成后台配置任务",
            ),
            (
                "canonical-system",
                "system",
                None,
                "{}",
                "今天需要完成后台维护任务",
            ),
            (
                "legacy-file",
                "file",
                Some("system"),
                "{}",
                "今天需要交付系统维护记录invoice.pdf",
            ),
        ] {
            let id = format!("{platform}-{suffix}");
            conn.execute("insert into daily_messages(id,day,profile_id,platform,chat_id,chat_name,is_group,
                sender_id,sender_name,timestamp,time_text,msg_type,content,raw_type,raw_json,content_hash)
                values(?1,'2026-05-01',?2,?2,'business-chat','项目群',1,'customer','客户',1,'09:00',?3,?4,?5,?6,?1)",
                params![id,platform,msg_type,content,raw_type,raw_json]).expect("fixture row");
        }
        upsert_keyword_meta(&conn, "2026-05-01", platform, "fixture-v1", "local_final")
            .expect("keyword metadata");
        let ids = [platform.to_owned()];
        let messages =
            load_analysis_messages_for_profiles(&conn, "2026-05-01", &ids).expect("analysis");
        let actual = messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<HashSet<_>>();
        let expected = [
            "announcement",
            "interaction",
            "meeting",
            "welcome",
            "invite",
            "compatibility",
            "checkin-rule",
        ]
        .map(|suffix| format!("{platform}-{suffix}"))
        .into_iter()
        .collect::<HashSet<_>>();
        assert_eq!(actual, expected, "analysis {platform}");
        let summary_messages = super::summary_candidates::load_summary_candidate_messages(
            &conn,
            "2026-05-01",
            platform,
        )
        .expect("summary SQL reader");
        assert_eq!(
            summary_messages
                .iter()
                .map(|message| message.id.clone())
                .collect::<HashSet<_>>(),
            expected,
            "summary SQL {platform}"
        );
        let summaries = load_summary_candidates(&conn, "2026-05-01", platform).expect("summary");
        let sources = summaries
            .iter()
            .flat_map(|value| value.source_message_ids.iter().cloned())
            .collect::<HashSet<_>>();
        assert!(sources.is_subset(&expected), "summary {platform}");
        for suffix in ["announcement", "interaction", "meeting"] {
            assert!(
                sources.contains(&format!("{platform}-{suffix}")),
                "summary {platform}: {suffix}"
            );
        }
        let plan = super::keyword_refine::build_keyword_refine_plan(&conn, "2026-05-01", &ids, 3)
            .expect("refine")
            .expect("business messages");
        let refined_sources = plan.payload["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .map(|value| value["id"].as_str().unwrap().to_owned())
            .collect::<HashSet<_>>();
        assert_eq!(refined_sources, expected, "refine {platform}");
        assert_eq!(
            conn.query_row(
                "select count(*) from daily_messages where profile_id=?1",
                [platform],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            11,
            "source messages remain stored"
        );
    }
}

#[test]
fn message_type_boundary_keeps_generated_business_topics_and_keyword_text() {
    for content in [
        "群公告发布时间需要确认",
        "拍一拍提醒功能需要交付",
        "会议安排与项目交付排期需要确认",
        "请设计欢迎新人引导流程",
        "明天邀请客户加入群聊讨论交付",
        "当前版本不支持显示内容，我们下周修复",
        "签到奖励积分规则需要产品确认",
        "请设计修改群名的权限提示",
        "接龙团购功能需要产品确认",
    ] {
        assert!(
            clean_message_content_for_ai("text", content).is_some(),
            "{content}"
        );
        assert!(
            !keyword_texts_from_message_content(content).is_empty(),
            "{content}"
        );
        assert!(
            !is_disallowed_dashboard_topic(content, "今天需要跟进项目交付"),
            "{content}"
        );
        assert!(!is_disallowed_dashboard_keyword(content), "{content}");
    }
    for kind in ["system", "sys", "notice", "notification"] {
        assert!(
            clean_message_content_for_ai(kind, "今天需要跟进后台维护").is_none(),
            "{kind}"
        );
    }
}
