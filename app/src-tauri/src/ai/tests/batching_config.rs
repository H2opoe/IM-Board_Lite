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
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
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
           null, 'profile-1', 'feishu', 'chat-a', '客户群',
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
        normalize_config(local.clone()).analysis_batch_size,
        LOCAL_DEEPSEEK_MAX_ANALYSIS_BATCH_MESSAGES
    );
    let mut constrained_local = normalize_config(local);
    apply_hardware_batch_limit(&mut constrained_local, 10);
    assert_eq!(constrained_local.analysis_batch_size, 10);

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
fn non_custom_prompts_follow_backend_builtin_updates() {
    let frontend_prompt_config = AiConfig {
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

    let normalized = normalize_config(frontend_prompt_config);
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
