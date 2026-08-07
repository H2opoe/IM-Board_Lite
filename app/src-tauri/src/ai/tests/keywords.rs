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
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
            "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'feishu', '飞书工作号', '{\"remark\":\"工作号\"}', datetime('now'), datetime('now'))",
            [],
        )
        .expect("profile");
    conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'feishu', 'chat-1', 'IM-Board 研发群', 1, ?2, ?3, ?4, '09:00', 'text', ?5, ?6)",
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
             values(?1, '2026-05-01', 'profile-1', 'feishu', 'chat-1', 'IM-Board 研发群', 1, ?2, ?3, ?4, '09:01', 'text', ?5, ?6)",
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
             values(?1, '2026-05-01', 'profile-1', 'feishu', 'chat-1', 'IM-Board 研发群', 1, ?2, ?3, ?4, '09:02', 'text', ?5, ?6)",
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
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
            "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'feishu', '飞书工作号', '{\"remark\":\"工作号\"}', datetime('now'), datetime('now'))",
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
             values(?1, '2026-05-01', 'profile-1', 'feishu', 'chat-1', 'IM-Board研发群', 1, 'u-1', '群主小吴', 1, '09:00', 'text', ?2, 'hash-1')",
            params!["msg-1", structured_content],
        )
        .expect("message 1");
    conn.execute(
            "insert into daily_messages(
               id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id, sender_name,
               timestamp, time_text, msg_type, content, content_hash
             )
             values(?1, '2026-05-01', 'profile-1', 'feishu', 'chat-1', 'IM-Board研发群', 1, 'u-2', '群员小李', 2, '09:01', 'text', ?2, 'hash-2')",
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
             values(?1, '2026-05-01', 'profile-1', 'feishu', 'chat-1', 'IM-Board研发群', 1, 'u-3', '群员小周', 3, '09:02', 'text', ?2, 'hash-3')",
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
        "飞书工作号",
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
        "飞书版本不支持展示内容，请升级至最新版本查看",
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
    assert_eq!(
        clean_message_content_for_ai("file", "[文件] 26）03.13—29凯乐石报销清单.xlsx"),
        Some("文件：26）03.13—29凯乐石报销清单.xlsx".to_owned())
    );
    for (msg_type, content) in [
        ("system", "[系统] 张三加入了群聊"),
        ("image", "客户发来的装修图片需要确认预算方案"),
        ("voice", "[语音]"),
        ("emoji", "[表情]"),
        ("file", "[文件]"),
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
fn clean_message_content_for_ai_filters_group_noise_templates() {
    for content in [
        r#"[系统] "Mr.成"通过扫描"管家芽芽"分享的二维码加入群聊"#,
        "🖥 欢迎 孙文康、皮蛋瘦肉周 加入应用宝Mac公测体验群！\n🔹 如何参与公测？",
        "飞书版本不支持展示内容，请升级至最新版本查看",
        "当前版本不支持显示内容",
        "请升级飞书查看",
        "@荣少\n今日第 58 个签到，加 5.0 积分 当前会员总积分：470.0 已连续签到5天",
        "[系统] 群公告 已更新",
        "[链接/文件] #接龙 🔥老温开团【周日5.24早上送货】",
        "#接龙\n百香果团购\n1. 王霞 1箱\n2. 钟予馨2箱\n3. 昕彤1箱",
    ] {
        assert_eq!(
            clean_message_content_for_ai("text", content),
            None,
            "group noise should not be sent to AI: {content}"
        );
    }

    for content in [
        "Steam官网被墙了，直接其他渠道找一个Steam的安装包，也一样的",
        "softwareupdate --install-rosetta 在【终端】里执行下这个命令试试",
        "早！不好意思，昨天有点事搞忘了，还在不，我一会过来拿",
    ] {
        assert_eq!(
            clean_message_content_for_ai("text", content),
            Some(content.to_owned())
        );
    }
}

#[test]
fn filter_analysis_messages_filters_group_only_checkin_text() {
    let mut checkin = test_message("msg-1".to_owned(), "chat-a");
    checkin.is_group = true;
    checkin.content = "签到".to_owned();

    let mut business = test_message("msg-2".to_owned(), "chat-a");
    business.is_group = true;
    business.content = "需要今天处理Steam安装没反应的问题".to_owned();

    let kept_ids = filter_analysis_messages(vec![checkin, business])
        .into_iter()
        .map(|message| message.id)
        .collect::<Vec<_>>();

    assert_eq!(kept_ids, vec!["msg-2"]);
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
fn filter_analysis_messages_keeps_soft_questions_and_inbound_files() {
    let messages = vec![
        analysis_message(
            "msg-question",
            "humanbeing",
            "humanbeing",
            "text",
            "请问付费版的话是可以拿到源码吗！",
            false,
        ),
        analysis_message(
            "msg-file",
            "化妆·白一玲（白白）",
            "化妆·白一玲（白白）",
            "file",
            "文件：26）03.13—29凯乐石报销清单.xlsx",
            false,
        ),
    ];

    let kept_ids = filter_analysis_messages(messages)
        .into_iter()
        .map(|message| message.id)
        .collect::<Vec<_>>();

    assert_eq!(
        kept_ids,
        vec!["msg-question".to_owned(), "msg-file".to_owned()]
    );
}

fn analysis_message(
    id: &str,
    chat_name: &str,
    sender_name: &str,
    msg_type: &str,
    content: &str,
    is_group: bool,
) -> AnalysisMessage {
    AnalysisMessage {
        id: id.to_owned(),
        profile_id: "profile-1".to_owned(),
        platform: "feishu".to_owned(),
        chat_id: chat_name.to_owned(),
        chat_name: chat_name.to_owned(),
        is_group,
        timestamp: 1,
        sender_id: sender_name.to_owned(),
        sender_name: sender_name.to_owned(),
        is_me: sender_name == "me",
        time_text: "09:00".to_owned(),
        msg_type: msg_type.to_owned(),
        content: content.to_owned(),
        partial: false,
    }
}

#[test]
fn persist_local_keyword_stats_filters_structural_noise_terms() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'feishu', '飞书工作号', '{}', datetime('now'), datetime('now'))",
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
                 values(?1, '2026-05-01', 'profile-1', 'feishu', 'chat-1', '测试群', 1, 'u-1', '测试用户', ?2, '09:00', 'text', ?3, ?4)",
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
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
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
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'feishu', '飞书工作号', '{}', datetime('now'), datetime('now'))",
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
                 values(?1, '2026-05-01', 'profile-1', 'feishu', 'chat-1', '测试群', 1, 'u-1', '测试用户', ?2, '09:00', 'text', ?3, ?4)",
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
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("schema");
    conn.execute(
        "insert into profiles(id, platform, label, config_json, created_at, updated_at)
             values('profile-1', 'feishu', '飞书工作号', '{}', datetime('now'), datetime('now'))",
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
                 values(?1, '2026-05-01', 'profile-1', 'feishu', 'chat-1', '测试群', 1, 'u-1', '测试用户', ?2, '09:00', 'text', ?3, ?4)",
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
