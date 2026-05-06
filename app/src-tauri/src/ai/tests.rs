use std::collections::HashSet;

use jieba_rs::Jieba;
use rusqlite::params;

use super::*;
use crate::analysis::local_keywords::{
    add_context_keyword, is_selectable_local_keyword, keyword_texts_from_message_content,
    local_keyword_candidates, select_local_keyword_ranks, LocalKeywordRank, LocalKeywordScore,
};

#[test]
fn local_keyword_candidates_keep_meaningful_terms() {
    let mut jieba = Jieba::new();
    let mut context_terms = HashSet::new();
    add_context_keyword(&mut jieba, &mut context_terms, "IM-Board");
    add_context_keyword(&mut jieba, &mut context_terms, "退货衣服");
    let candidates = local_keyword_candidates(
        &jieba,
        "猪猪，等下帮我把退货的衣服拿下一楼。OpenAI 和 IM-Board 都更新了。",
        &context_terms,
    );

    assert!(candidates.iter().any(|value| value.text == "退货衣服"));
    assert!(candidates.iter().any(|value| value.text == "一楼"));
    assert!(candidates.iter().any(|value| value.text == "openai"));
    assert!(candidates.iter().any(|value| value.text == "im-board"));
}

#[test]
fn persist_local_keyword_stats_uses_jieba_and_local_context() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
            "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'wechat', '微信工作号', '{\"remark\":\"工作号\"}', datetime('now'), datetime('now'))",
            [],
        )
        .expect("profile");
    conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board 研发群', 1, ?2, ?3, ?4, '09:00', 'text', ?5, ?6)",
            params![
                "msg-1",
                "u-1",
                "同事甲",
                1_i64,
                "IM-Board 词云今天接入 jieba-rs 分词，先不用 AI。",
                "hash-1"
            ],
        )
        .expect("message 1");
    conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board 研发群', 1, ?2, ?3, ?4, '09:01', 'text', ?5, ?6)",
            params![
                "msg-2",
                "u-2",
                "同事乙",
                2_i64,
                "词云继续用 jieba-rs 本地分词，降低无效关键词。",
                "hash-2"
            ],
        )
        .expect("message 2");
    conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board 研发群', 1, ?2, ?3, ?4, '09:02', 'text', ?5, ?6)",
            params![
                "msg-3",
                "u-3",
                "同事丙",
                3_i64,
                "词云继续验证 jieba-rs 关键词频次阈值。",
                "hash-3"
            ],
        )
        .expect("message 3");

    let count =
        persist_local_keyword_stats(&conn, "2026-05-01", "profile-1").expect("local keyword stats");
    assert!(count > 0);
    let raw: String = conn
            .query_row(
                "select value_json from daily_stats where day = '2026-05-01' and profile_id = 'profile-1' and metric = 'keywords'",
                [],
                |row| row.get(0),
            )
            .expect("keywords");
    let values: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("keyword json");
    let texts = values
        .iter()
        .filter_map(|value| value.get("text").and_then(|value| value.as_str()))
        .collect::<Vec<_>>();
    assert!(texts.contains(&"词云"), "got {texts:?}");
    assert!(texts.contains(&"jieba-rs"), "got {texts:?}");
}

#[test]
fn selectable_local_keyword_requires_three_occurrences() {
    let mut low_frequency = LocalKeywordScore {
        score: 10.0,
        occurrences: MIN_KEYWORD_CLOUD_COUNT - 1,
        chat_ids: HashSet::new(),
        context_hits: MIN_KEYWORD_CLOUD_COUNT - 1,
        best_source: crate::analysis::local_keywords::LocalKeywordSource::Context,
        latest_timestamp: 0,
    };
    assert!(!is_selectable_local_keyword("词云", &low_frequency));

    low_frequency.occurrences = MIN_KEYWORD_CLOUD_COUNT;
    assert!(is_selectable_local_keyword("词云", &low_frequency));
}

#[test]
fn persist_local_keyword_stats_ignores_message_metadata() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
            "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'wechat', '微信工作号', '{\"remark\":\"工作号\"}', datetime('now'), datetime('now'))",
            [],
        )
        .expect("profile");

    let structured_content = serde_json::json!({
        "chatName": "IM-Board研发群",
        "senderName": "群主小吴",
        "msgType": "text",
        "content": "词云分词只看消息正文，忽略返回结构。"
    })
    .to_string();
    conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board研发群', 1, 'u-1', '群主小吴', 1, '09:00', 'text', ?2, 'hash-1')",
            params!["msg-1", structured_content],
        )
        .expect("message 1");
    conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board研发群', 1, 'u-2', '群员小李', 2, '09:01', 'text', ?2, 'hash-2')",
            params![
                "msg-2",
                "chatName: IM-Board研发群 senderName: 群员小李 content: 继续优化词云分词，不要混入群名和用户名。"
            ],
        )
        .expect("message 2");
    conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', 'IM-Board研发群', 1, 'u-3', '群员小周', 3, '09:02', 'text', ?2, 'hash-3')",
            params!["msg-3", "继续验证词云分词，只统计消息正文里的关键词。"],
        )
        .expect("message 3");

    persist_local_keyword_stats(&conn, "2026-05-01", "profile-1").expect("local keyword stats");
    let raw: String = conn
            .query_row(
                "select value_json from daily_stats where day = '2026-05-01' and profile_id = 'profile-1' and metric = 'keywords'",
                [],
                |row| row.get(0),
            )
            .expect("keywords");
    let values: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("keyword json");
    let texts = values
        .iter()
        .filter_map(|value| value.get("text").and_then(|value| value.as_str()))
        .collect::<Vec<_>>();

    assert!(texts.contains(&"词云"), "got {texts:?}");
    assert!(texts.contains(&"分词"), "got {texts:?}");
    for ignored in [
        "im-board",
        "研发群",
        "群主小吴",
        "小吴",
        "群员小李",
        "小李",
        "chatname",
        "sendername",
        "msgtype",
        "content",
        "微信工作号",
        "工作号",
    ] {
        assert!(
            !texts.contains(&ignored),
            "metadata keyword should be ignored: {ignored}; got {texts:?}"
        );
    }
}

#[test]
fn keyword_texts_from_message_content_filters_xml_urls_and_bot_templates() {
    assert!(keyword_texts_from_message_content(
            r#"[系统] <?xml version="1.0"?><sysmsg type="revokemsg"><revokemsg><content>"Joyce" 撤回了一条消息</content><revoketime>0</revoketime></revokemsg></sysmsg>"#
        )
        .is_empty());
    assert!(keyword_texts_from_message_content(
            "@Joyce\n🕹今日已签到！\n连续签到解锁更多精彩好礼\n点击进入积分商城 https://u.isaveu.cn/ixh1o"
        )
        .is_empty());

    let texts = keyword_texts_from_message_content(
        "词云过滤 XML 元数据和链接 https://u.isaveu.cn/ixh1o，保留真正消息正文。",
    );
    assert_eq!(texts.len(), 1);
    assert!(texts[0].contains("词云过滤"));
    assert!(!texts[0].contains("u.isaveu.cn"));
}

#[test]
fn keyword_texts_from_message_content_filters_dingtalk_custom_scheme_urls() {
    assert!(keyword_texts_from_message_content(
            "佛山市戴胜文化传媒有限公司\n让我们一起欢迎新人~\n群小钉\n[dingtalk://dingtalkclient/action/openapp?slide_panel_option=%7B%22width%22%3A480%2C%22hidesTitle%22%3Atrue%7D&containerType=board&dd_darkmode=false&selfIntroduceText=&openedByMiniApp=true&needRedirect=true&corpId=ding298a3a7e22a45692f2c783f7214b6d69]",
        )
        .is_empty());

    let texts = keyword_texts_from_message_content(
        "客户新人培训资料已发出\n[dingtalk://dingtalkclient/action/openapp?slide_panel_option=%7B%22width%22%3A480%2C%22hidesTitle%22%3Atrue%7D&containerType=board&dd_darkmode=false&selfIntroduceText=&openedByMiniApp=true&needRedirect=true&corpId=ding298a3a7e22a45692f2c783f7214b6d69]",
    );
    let joined = texts.join(" ");

    assert!(joined.contains("新人培训"), "got {joined}");
    for ignored in [
        "dingtalk",
        "containerType",
        "board",
        "dd_darkmode",
        "selfIntroduceText",
        "openedByMiniApp",
        "needRedirect",
        "corpId",
    ] {
        assert!(
            !joined
                .to_ascii_lowercase()
                .contains(&ignored.to_ascii_lowercase()),
            "DingTalk URL noise should be stripped: {ignored}; got {joined}"
        );
    }
}

#[test]
fn dashboard_keyword_filter_rejects_dingtalk_payload_fragments() {
    for ignored in [
        "board",
        "containertype",
        "3fdd_darkmode",
        "26selfintroducetext",
        "26openedbyminiapp",
        "26needredirect",
        "dfalse26cid3",
        "7b22width22",
        "3a4802c22",
        "dding298a3a7e22a45692f2c783f7214b6d6926",
        "d0129131342192618510926",
        "d500000000485825726",
        "dgroupwelcome26",
        "d7480156731726",
        "dempprofile26",
        "fhrmregister2",
    ] {
        assert!(
            is_disallowed_dashboard_keyword(ignored),
            "DingTalk payload keyword should be ignored: {ignored}"
        );
    }

    for kept in ["im-board", "openai", "2500rmb", "确保数据安全"] {
        assert!(
            !is_disallowed_dashboard_keyword(kept),
            "meaningful keyword should stay selectable: {kept}"
        );
    }
}

#[test]
fn keyword_texts_from_message_content_filters_media_and_client_notices() {
    for content in [
        "图片分享",
        "[图片]",
        "分享图片",
        "微信版本不支持展示内容，请升级至最新版本查看",
        "当前版本不支持显示内容",
    ] {
        assert!(
            keyword_texts_from_message_content(content).is_empty(),
            "placeholder or client notice should be ignored: {content}"
        );
    }

    let texts = keyword_texts_from_message_content("客户发来的装修图片需要确认预算方案");
    assert_eq!(texts, vec!["客户发来的装修图片需要确认预算方案".to_owned()]);
}

#[test]
fn clean_message_content_for_ai_keeps_only_allowed_message_types() {
    assert_eq!(
        clean_message_content_for_ai("text", "客户今天需要确认报价"),
        Some("客户今天需要确认报价".to_owned())
    );
    assert_eq!(
        clean_message_content_for_ai("location", "上海市浦东新区世纪大道"),
        Some("上海市浦东新区世纪大道".to_owned())
    );
    for (msg_type, content) in [
        ("system", "[系统] 张三加入了群聊"),
        ("image", "客户发来的装修图片需要确认预算方案"),
        ("voice", "[语音]"),
        ("emoji", "[表情]"),
        ("file", "报价单.pdf"),
        ("video", "[视频]"),
        ("voip", "[语音通话] 通话时长 00:12"),
    ] {
        assert_eq!(
            clean_message_content_for_ai(msg_type, content),
            None,
            "{msg_type} should not be sent to AI"
        );
    }
}

#[test]
fn clean_link_or_app_message_text_keeps_title_and_description_without_urls_or_ips() {
    let content = r#"{
          "title": "客户续费方案",
          "description": "需要确认 5 月报价和审批节奏",
          "url": "https://example.com/order?id=1",
          "host": "192.168.1.9"
        }"#;

    let text = clean_message_content_for_ai("appmsg", content).expect("cleaned app text");

    assert!(text.contains("客户续费方案"));
    assert!(text.contains("审批节奏"));
    assert!(!text.contains("example.com"));
    assert!(!text.contains("192.168.1.9"));
}

#[test]
fn persist_local_keyword_stats_filters_structural_noise_terms() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'wechat', '微信工作号', '{}', datetime('now'), datetime('now'))",
        [],
    )
    .expect("profile");

    for (id, content, timestamp) in [
            (
                "msg-1",
                r#"[系统] <?xml version="1.0"?><sysmsg type="revokemsg"><revokemsg><content>"Joyce" 撤回了一条消息</content><revoketime>0</revoketime></revokemsg></sysmsg>"#,
                1_i64,
            ),
            (
                "msg-2",
                "@Joyce\n🕹今日已签到！\n连续签到解锁更多精彩好礼\n点击进入🛒积分商城 https://u.isaveu.cn/ixh1o",
                2_i64,
            ),
            (
                "msg-3",
                "词云过滤 XML 元数据和链接，保留真正消息正文。",
                3_i64,
            ),
            (
                "msg-4",
                "继续优化词云过滤，不要显示 version type sysmsg revoketime。",
                4_i64,
            ),
            (
                "msg-5",
                "词云过滤继续保留真正消息正文，避免结构字段混入。",
                5_i64,
            ),
            (
                "msg-6",
                "佛山市戴胜文化传媒有限公司\n让我们一起欢迎新人~\n群小钉\n[dingtalk://dingtalkclient/action/openapp?slide_panel_option=%7B%22width%22%3A480%2C%22hidesTitle%22%3Atrue%7D&containerType=board&dd_darkmode=false&selfIntroduceText=&openedByMiniApp=true&needRedirect=true&corpId=ding298a3a7e22a45692f2c783f7214b6d69]",
                6_i64,
            ),
        ] {
            conn.execute(
                "insert into daily_messages(
                   id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
                   timestamp, time_text, msg_type, content, content_hash
                 )
                 values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', '测试群', 1, 'u-1', '测试用户', ?2, '09:00', 'text', ?3, ?4)",
                params![id, timestamp, content, format!("hash-{id}")],
            )
            .expect("message");
        }

    persist_local_keyword_stats(&conn, "2026-05-01", "profile-1").expect("local keyword stats");
    let raw: String = conn
            .query_row(
                "select value_json from daily_stats where day = '2026-05-01' and profile_id = 'profile-1' and metric = 'keywords'",
                [],
                |row| row.get(0),
            )
            .expect("keywords");
    let values: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("keyword json");
    let texts = values
        .iter()
        .filter_map(|value| value.get("text").and_then(|value| value.as_str()))
        .collect::<Vec<_>>();

    assert!(texts.contains(&"词云"), "got {texts:?}");
    for ignored in [
        "xml",
        "version",
        "type",
        "sysmsg",
        "revoketime",
        "u.isaveu.cn",
        "joyce",
        "连续",
        "解锁",
        "领取",
        "链接",
        "board",
        "containertype",
        "3fdd_darkmode",
        "26selfintroducetext",
        "26openedbyminiapp",
        "26needredirect",
        "dingtalkclient",
        "hrmregister",
    ] {
        assert!(
            !texts.contains(&ignored),
            "structural keyword should be ignored: {ignored}; got {texts:?}"
        );
    }
}

#[test]
fn select_local_keyword_ranks_keeps_longest_four_char_overlap() {
    let selected = select_local_keyword_ranks(vec![
        test_keyword_rank("潦草的一生", 100.0),
        test_keyword_rank("我们这潦草的一生", 1.0),
        test_keyword_rank("退货衣服", 10.0),
    ]);
    let texts = selected
        .iter()
        .map(|item| item.text.as_str())
        .collect::<Vec<_>>();

    assert!(texts.contains(&"我们这潦草的一生"));
    assert!(!texts.contains(&"潦草的一生"));
    assert!(texts.contains(&"退货衣服"));
}

#[test]
fn select_local_keyword_ranks_collapses_contained_fragments() {
    let selected = select_local_keyword_ranks(vec![
        test_keyword_rank("飞书", 100.0),
        test_keyword_rank("李俊彦", 90.0),
        test_keyword_rank("俊彦飞", 80.0),
        test_keyword_rank("李俊彦飞书", 10.0),
        test_keyword_rank("确保数据安全", 8.0),
    ]);
    let texts = selected
        .iter()
        .map(|item| item.text.as_str())
        .collect::<Vec<_>>();

    assert!(texts.contains(&"李俊彦飞书"));
    assert!(!texts.contains(&"飞书"));
    assert!(!texts.contains(&"李俊彦"));
    assert!(!texts.contains(&"俊彦飞"));
    assert!(texts.contains(&"确保数据安全"));
}

#[test]
fn persist_summary_stats_does_not_overwrite_local_keywords() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    upsert_stat(
        &conn,
        "2026-05-01",
        "profile-1",
        "keywords",
        &[serde_json::json!({ "text": "本地词云", "weight": 3 })],
    )
    .expect("seed keywords");

    persist_summary_stats(
        &conn,
        "2026-05-01",
        "profile-1",
        AiSummary {
            topics: vec![serde_json::json!({ "title": "话题", "summary": "摘要", "count": 2 })],
            keywords: Vec::new(),
        },
        &[],
    )
    .expect("summary stats");

    let raw: String = conn
            .query_row(
                "select value_json from daily_stats where day = '2026-05-01' and profile_id = 'profile-1' and metric = 'keywords'",
                [],
                |row| row.get(0),
            )
            .expect("keywords");
    assert!(raw.contains("本地词云"));
    assert!(!raw.contains("ai词云"));
}

#[test]
fn keyword_refine_uses_messages_and_overwrites_without_local_candidates() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'wechat', '微信工作号', '{}', datetime('now'), datetime('now'))",
        [],
    )
    .expect("profile");
    for (id, content, timestamp) in [
        ("msg-1", "下午货架自取的那批货到了吗", 1_i64),
        ("msg-2", "货架自取那批货已经放好了", 2_i64),
        ("msg-3", "今天货架自取的订单别漏掉", 3_i64),
        ("msg-4", "张三通过扫描二维码加入群聊", 4_i64),
    ] {
        conn.execute(
            "insert into daily_messages(
                   id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
                   timestamp, time_text, msg_type, content, content_hash
                 )
                 values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', '测试群', 1, 'u-1', '测试用户', ?2, '09:00', 'text', ?3, ?4)",
            params![id, timestamp, content, format!("hash-{id}")],
        )
        .expect("message");
    }
    upsert_stat(
        &conn,
        "2026-05-01",
        "profile-1",
        "keywords",
        &[serde_json::json!({ "text": "本地旧词", "weight": 3, "version": "kw-test" })],
    )
    .expect("seed local keywords");
    upsert_keyword_meta(&conn, "2026-05-01", "profile-1", "kw-test", "local_final")
        .expect("keyword meta");

    let plan = build_keyword_refine_plan(&conn, "2026-05-01", &["profile-1".to_owned()], 4)
        .expect("keyword refine plan")
        .expect("plan");
    assert!(plan.payload.get("candidates").is_none());
    let messages = plan
        .payload
        .get("messages")
        .and_then(|value| value.as_array())
        .expect("messages payload");
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["content"], "下午货架自取的那批货到了吗");

    let replaced = persist_refined_keywords_from_analysis(
        &conn,
        "2026-05-01",
        &plan,
        &[serde_json::json!({
            "profileId": "profile-1",
            "display": "货架自取",
            "aliases": ["自取货架"],
            "category": "business_topic",
            "valid": true,
            "confidence": 0.9,
            "scoreMultiplier": 1.2,
            "sourceMessageIds": ["msg-1", "msg-2", "msg-3"]
        })],
    )
    .expect("persist refined keywords");
    assert_eq!(replaced, 1);

    let raw: String = conn
        .query_row(
            "select value_json from daily_stats where day = '2026-05-01' and profile_id = 'profile-1' and metric = 'keywords'",
            [],
            |row| row.get(0),
        )
        .expect("keywords");
    let values: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("keyword json");
    assert_eq!(values[0]["text"], "货架自取");
    assert_eq!(values[0]["source"], "ai_refined");
    assert_eq!(values[0]["localScore"], 0.0);
    assert_eq!(values[0]["messageCount"], MIN_KEYWORD_CLOUD_COUNT);
    assert!(!raw.contains("本地旧词"));
}

#[test]
fn keyword_refine_requires_three_ai_source_messages() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'wechat', '微信工作号', '{}', datetime('now'), datetime('now'))",
        [],
    )
    .expect("profile");
    for (id, content, timestamp) in [
        ("msg-1", "货架自取那批货到了", 1_i64),
        ("msg-2", "货架自取订单已经放好", 2_i64),
    ] {
        conn.execute(
            "insert into daily_messages(
                   id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
                   timestamp, time_text, msg_type, content, content_hash
                 )
                 values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', '测试群', 1, 'u-1', '测试用户', ?2, '09:00', 'text', ?3, ?4)",
            params![id, timestamp, content, format!("hash-{id}")],
        )
        .expect("message");
    }
    upsert_stat(
        &conn,
        "2026-05-01",
        "profile-1",
        "keywords",
        &[serde_json::json!({ "text": "本地旧词", "weight": 3, "version": "kw-test" })],
    )
    .expect("seed local keywords");
    upsert_stat(
        &conn,
        "2026-05-01",
        "aggregate",
        "keywords",
        &[serde_json::json!({ "text": "聚合旧词", "weight": 9, "version": "kw-aggregate-old" })],
    )
    .expect("seed aggregate keywords");
    upsert_keyword_meta(&conn, "2026-05-01", "profile-1", "kw-test", "local_final")
        .expect("keyword meta");

    let plan = build_keyword_refine_plan(&conn, "2026-05-01", &["profile-1".to_owned()], 2)
        .expect("keyword refine plan")
        .expect("plan");
    let replaced = persist_refined_keywords_from_analysis(
        &conn,
        "2026-05-01",
        &plan,
        &[serde_json::json!({
            "profileId": "profile-1",
            "display": "货架自取",
            "aliases": [],
            "category": "business_topic",
            "valid": true,
            "confidence": 0.9,
            "scoreMultiplier": 1.2,
            "sourceMessageIds": ["msg-1", "msg-2"]
        })],
    )
    .expect("persist refined keywords");
    assert_eq!(replaced, 1);

    let raw: String = conn
        .query_row(
            "select value_json from daily_stats where day = '2026-05-01' and profile_id = 'profile-1' and metric = 'keywords'",
            [],
            |row| row.get(0),
        )
        .expect("keywords");
    let values: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("keyword json");
    assert!(
        values.is_empty(),
        "old local keywords must be replaced by empty AI result: {values:?}"
    );
    let aggregate_count: i64 = conn
        .query_row(
            "select count(*) from daily_stats where day = '2026-05-01' and profile_id = 'aggregate' and metric = 'keywords'",
            [],
            |row| row.get(0),
        )
        .expect("aggregate keyword count");
    assert_eq!(aggregate_count, 0);
}

#[test]
fn split_analysis_batches_keeps_each_chat_together() {
    let mut messages = Vec::new();
    for index in 0..30 {
        messages.push(test_message(format!("a-{index}"), "chat-a"));
        messages.push(test_message(format!("b-{index}"), "chat-b"));
    }

    let batches = split_analysis_batches(
        messages,
        OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE,
        OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS,
    );
    assert!(batches
        .iter()
        .all(|batch| estimate_analysis_messages_tokens(batch)
            <= OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS));
    assert!(batches.iter().any(|batch| batch
        .iter()
        .filter(|message| message.chat_id == "chat-a")
        .count()
        == 30));
    assert!(batches.iter().any(|batch| batch
        .iter()
        .filter(|message| message.chat_id == "chat-b")
        .count()
        == 30));
}

#[test]
fn split_analysis_batches_allows_large_single_chat_batch() {
    let messages = (0..195)
        .map(|index| test_message(format!("a-{index}"), "chat-a"))
        .collect::<Vec<_>>();

    let batches = split_analysis_batches(
        messages,
        OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE,
        OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS,
    );
    assert!(batches.len() > 1);
    assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 195);
    assert!(batches
        .iter()
        .all(|batch| estimate_analysis_messages_tokens(batch)
            <= OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS));
}

#[test]
fn split_analysis_batches_splits_long_single_chat_by_token_budget() {
    let mut messages = (0..12)
        .map(|index| test_message(format!("a-{index}"), "chat-a"))
        .collect::<Vec<_>>();
    for message in &mut messages {
        message.content = "需要确认这个客户投诉和交付风险。".repeat(35);
    }

    let batches = split_analysis_batches(
        messages,
        OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE,
        LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS,
    );

    assert!(batches.len() > 1);
    assert!(batches
        .iter()
        .all(|batch| estimate_analysis_messages_tokens(batch)
            <= LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS));
}

#[test]
fn split_analysis_batches_packs_multiple_chats_until_near_limit() {
    let messages = (0..170)
        .map(|index| test_message(format!("a-{index}"), &format!("chat-{index}")))
        .collect::<Vec<_>>();

    let batches = split_analysis_batches(
        messages,
        OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE,
        OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS,
    );

    assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 170);
    assert!(batches.len() > 1);
    assert!(batches
        .iter()
        .all(|batch| estimate_analysis_messages_tokens(batch)
            <= OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS));
}

#[test]
fn split_analysis_batches_starts_next_batch_before_crossing_limit() {
    let messages = (0..90)
        .map(|index| test_message(format!("a-{index}"), &format!("chat-{index}")))
        .collect::<Vec<_>>();

    let batches = split_analysis_batches(
        messages,
        LOCAL_DEEPSEEK_MAX_ANALYSIS_BATCH_MESSAGES,
        LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS,
    );

    assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 90);
    assert!(batches.len() > 2);
    assert!(batches
        .iter()
        .all(|batch| estimate_analysis_messages_tokens(batch)
            <= LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS));
}

#[test]
fn split_analysis_batch_plans_use_full_request_budget() {
    let mut messages = (0..24)
        .map(|index| test_message(format!("msg-{index}"), &format!("chat-{index}")))
        .collect::<Vec<_>>();
    for message in &mut messages {
        message.content = "客户催促交付，需要今天确认排期。".repeat(15);
    }

    let light_batches = split_analysis_batch_plans(messages.clone(), 30, 4_096, 0);
    let heavy_batches = split_analysis_batch_plans(messages, 30, 4_096, 2_200);

    assert!(
        heavy_batches.len() > light_batches.len(),
        "fixed prompt and context budget should reduce messages per batch"
    );
    assert!(heavy_batches
        .iter()
        .all(|batch| batch.estimated_input_tokens <= 4_096));
}

#[test]
fn split_large_chat_group_adds_overlap_without_losing_primary_messages() {
    let mut messages = (0..18)
        .map(|index| test_message(format!("msg-{index}"), "chat-a"))
        .collect::<Vec<_>>();
    for message in &mut messages {
        message.content = "客户投诉交付风险，需要今天处理。".repeat(22);
    }

    let batches = split_analysis_batch_plans(messages, 30, 1_900, 0);
    assert!(batches.len() > 1);
    assert!(batches
        .iter()
        .skip(1)
        .any(|batch| batch.overlap_message_count > 0));

    let primary_ids = batches
        .iter()
        .flat_map(|batch| {
            batch
                .primary_messages()
                .iter()
                .map(|message| message.id.clone())
        })
        .collect::<HashSet<_>>();
    assert_eq!(primary_ids.len(), 18);
}

#[test]
fn estimate_analysis_request_tokens_includes_existing_action_context() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into action_items(
           id, type, status, priority, title, description, suggested_reply, profile_id, platform,
           chat_id, chat_name, source_message_ids, evidence_summary, context_incomplete, carry_over,
           first_detected_at, last_updated_at
         )
         values(
           'act-1', 'task', 'open', 'high', '跟进客户投诉',
           '客户持续反馈交付延期，需要协调排期并回复处理方案。',
           null, 'profile-1', 'wechat', 'chat-a', '客户群',
           '[\"old-1\"]', '客户已经连续两次催促交付时间。', 0, 1,
           datetime('now'), datetime('now')
         )",
        [],
    )
    .expect("action item");
    let mut message = test_message("msg-1".to_owned(), "chat-a");
    message.chat_name = "客户群".to_owned();
    let context = load_analysis_context_for_messages(&conn, &[message.clone()]).expect("context");

    let without_context = estimate_analysis_messages_tokens(&[message.clone()]);
    let with_context =
        estimate_analysis_request_tokens(DEFAULT_ANALYSIS_PROMPT, "", &[message], &context);

    assert!(
        with_context > without_context + 200,
        "request estimate should include prompt and existingActionItems"
    );
}

#[test]
fn normalize_analysis_batch_size_uses_provider_limits() {
    let local = AiConfig {
        provider: LOCAL_DEEPSEEK_PROVIDER.to_owned(),
        api_key: String::new(),
        base_url: "http://127.0.0.1:11434/v1".to_owned(),
        model: LOCAL_DEEPSEEK_MODEL.to_owned(),
        user_prompt: String::new(),
        analysis_prompt: String::new(),
        summary_prompt: String::new(),
        analysis_prompt_custom: false,
        summary_prompt_custom: false,
        analysis_batch_size: 50,
        enabled: false,
        test_status: "untested".to_owned(),
    };
    assert_eq!(
        normalize_config(local).analysis_batch_size,
        LOCAL_DEEPSEEK_MAX_ANALYSIS_BATCH_MESSAGES
    );

    let other = AiConfig {
        provider: "火山方舟".to_owned(),
        model: "doubao-seed".to_owned(),
        analysis_batch_size: 0,
        ..normalize_config(AiConfig {
            provider: LOCAL_DEEPSEEK_PROVIDER.to_owned(),
            api_key: String::new(),
            base_url: String::new(),
            model: LOCAL_DEEPSEEK_MODEL.to_owned(),
            user_prompt: String::new(),
            analysis_prompt: String::new(),
            summary_prompt: String::new(),
            analysis_prompt_custom: false,
            summary_prompt_custom: false,
            analysis_batch_size: LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE,
            enabled: false,
            test_status: String::new(),
        })
    };
    assert_eq!(
        normalize_config(other.clone()).analysis_batch_size,
        OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE
    );

    let other_over_limit = AiConfig {
        provider: "火山方舟".to_owned(),
        model: "doubao-seed".to_owned(),
        analysis_batch_size: 999,
        ..other
    };
    assert_eq!(
        normalize_config(other_over_limit).analysis_batch_size,
        OTHER_MODEL_MAX_ANALYSIS_BATCH_MESSAGES
    );
}

#[test]
fn default_prompts_follow_builtin_updates_until_customized() {
    let saved_default = AiConfig {
            provider: LOCAL_DEEPSEEK_PROVIDER.to_owned(),
            api_key: String::new(),
            base_url: "http://127.0.0.1:11434/v1".to_owned(),
            model: LOCAL_DEEPSEEK_MODEL.to_owned(),
            user_prompt: String::new(),
            analysis_prompt: "你是一个本地即时通讯工作助理。请只根据输入的今天聊天消息识别真正需要用户处理的事项。\n\n请返回严格 JSON，不要 Markdown，不要解释：".to_owned(),
            summary_prompt: "你是一个本地即时通讯工作助理。请根据输入的今天聊天消息生成看板话题。\n\n话题规则：candidateTopics只包含本次尚未汇总的新消息候选；不要使用群聊当天总消息数。\n\n请返回严格 JSON，不要 Markdown，不要解释：".to_owned(),
            analysis_prompt_custom: false,
            summary_prompt_custom: false,
            analysis_batch_size: LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE,
            enabled: true,
            test_status: "untested".to_owned(),
        };

    let normalized = normalize_config(saved_default);
    assert_eq!(normalized.analysis_prompt, DEFAULT_ANALYSIS_PROMPT);
    assert_eq!(normalized.summary_prompt, DEFAULT_SUMMARY_PROMPT);
    assert!(!normalized.analysis_prompt_custom);
    assert!(!normalized.summary_prompt_custom);
}

#[test]
fn custom_prompts_do_not_follow_builtin_updates() {
    let saved_custom = AiConfig {
        provider: LOCAL_DEEPSEEK_PROVIDER.to_owned(),
        api_key: String::new(),
        base_url: "http://127.0.0.1:11434/v1".to_owned(),
        model: LOCAL_DEEPSEEK_MODEL.to_owned(),
        user_prompt: String::new(),
        analysis_prompt: "自定义待办识别提示词".to_owned(),
        summary_prompt: "自定义话题识别提示词".to_owned(),
        analysis_prompt_custom: true,
        summary_prompt_custom: true,
        analysis_batch_size: LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE,
        enabled: true,
        test_status: "untested".to_owned(),
    };

    let normalized = normalize_config(saved_custom);
    assert_eq!(normalized.analysis_prompt, "自定义待办识别提示词");
    assert_eq!(normalized.summary_prompt, "自定义话题识别提示词");
    assert!(normalized.analysis_prompt_custom);
    assert!(normalized.summary_prompt_custom);
}

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
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
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
                     values(?1, '2026-05-01', 'profile-1', 'wechat', 'chat-1', '测试群', 1,
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
    assert!(DEFAULT_SUMMARY_PROMPT.contains("旧 sourceMessageIds 由系统自动保留并合并"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("系统会自动合并 existingTopics.sourceMessageIds"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("keywordRefine"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("keywordRefine.messages"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("至少对应 3 条有效消息"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("scoreMultiplier"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("拍一拍"));
    assert!(DEFAULT_SUMMARY_PROMPT.contains("拍了拍"));
    assert!(!DEFAULT_SUMMARY_PROMPT.contains("keywordRefine.candidates"));
}

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

#[test]
fn persist_analysis_can_write_resolved_reply_as_done() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, ?2, ?3, 'wechat', ?4, ?5, 0, 'friend', '朋友', 1, '09:00', 'text', '你看下可以吗', 'hash-1')",
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
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute_batch(
        "insert into daily_messages(
           id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
           timestamp, time_text, msg_type, content, content_hash
         )
         values
           ('msg-question', '2026-05-06', 'profile-1', 'wechat', 'chat-1', '何玉玲',
            0, 'friend', '何玉玲', 100, '11:23', 'text', '你大概明早几点到东丽，我准备好出门时间。', 'hash-question'),
           ('msg-reply', '2026-05-06', 'profile-1', 'wechat', 'chat-1', '何玉玲',
            0, 'me', 'me', 101, '11:24', 'text', '9点', 'hash-reply'),
           ('msg-ack', '2026-05-06', 'profile-1', 'wechat', 'chat-1', '何玉玲',
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
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values('msg-1', '2026-05-01', 'profile-1', 'wechat', 'chat-1', '测试聊天',
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
                    'profile-1', 'wechat', 'chat-1', '测试聊天', '[\"old-msg\"]',
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
    conn.execute_batch(include_str!("../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values('msg-1', '2026-05-01', 'profile-1', 'wechat', 'chat-1', '测试群',
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
                    'profile-1', 'wechat', 'chat-1', '测试群', '[\"old-msg\"]',
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
        platform: "wechat".to_owned(),
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
