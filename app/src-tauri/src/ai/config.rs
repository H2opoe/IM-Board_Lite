use chrono::Local;
use rusqlite::{params, OptionalExtension};

use crate::storage::models::AiConfig;

use super::{
    ANALYSIS_REQUEST_RESERVED_TOKENS, DEFAULT_ANALYSIS_PROMPT, DEFAULT_SUMMARY_PROMPT,
    LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS, LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE,
    LOCAL_DEEPSEEK_ANALYSIS_OUTPUT_TOKENS, LOCAL_DEEPSEEK_MAX_ANALYSIS_BATCH_MESSAGES,
    LOCAL_DEEPSEEK_MODEL, LOCAL_DEEPSEEK_PROVIDER, MIN_ANALYSIS_BATCH_MESSAGES,
    OPENROUTER_PROVIDER, OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS,
    OTHER_MODEL_ANALYSIS_OUTPUT_TOKENS, OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE,
    OTHER_MODEL_MAX_ANALYSIS_BATCH_MESSAGES,
};

pub fn get_config(conn: &rusqlite::Connection) -> anyhow::Result<AiConfig> {
    let row = conn
        .query_row(
            "select provider, api_key, base_url, model, user_prompt, analysis_prompt, summary_prompt, analysis_prompt_custom, summary_prompt_custom, analysis_batch_size, enabled, test_status from ai_config where id = 1",
            [],
            |row| {
                let analysis_prompt: String = row.get(5)?;
                let summary_prompt: String = row.get(6)?;
                let analysis_prompt_custom: Option<i64> = row.get(7)?;
                let summary_prompt_custom: Option<i64> = row.get(8)?;
                Ok(normalize_config(AiConfig {
                    provider: row.get(0)?,
                    api_key: row.get(1)?,
                    base_url: row.get(2)?,
                    model: row.get(3)?,
                    user_prompt: row.get(4)?,
                    analysis_prompt_custom: infer_prompt_custom(
                        analysis_prompt_custom,
                        &analysis_prompt,
                        is_known_default_analysis_prompt,
                    ),
                    summary_prompt_custom: infer_prompt_custom(
                        summary_prompt_custom,
                        &summary_prompt,
                        is_known_default_summary_prompt,
                    ),
                    analysis_prompt,
                    summary_prompt,
                    analysis_batch_size: row.get(9)?,
                    enabled: row.get::<_, i64>(10)? == 1,
                    test_status: row.get(11)?,
                }))
            },
        )
        .optional()?;

    Ok(row.unwrap_or_else(|| {
        normalize_config(AiConfig {
            provider: LOCAL_DEEPSEEK_PROVIDER.to_owned(),
            api_key: String::new(),
            base_url: "http://127.0.0.1:11434/v1".to_owned(),
            model: LOCAL_DEEPSEEK_MODEL.to_owned(),
            user_prompt: String::new(),
            analysis_prompt: DEFAULT_ANALYSIS_PROMPT.to_owned(),
            summary_prompt: DEFAULT_SUMMARY_PROMPT.to_owned(),
            analysis_prompt_custom: false,
            summary_prompt_custom: false,
            analysis_batch_size: LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE,
            enabled: true,
            test_status: "untested".to_owned(),
        })
    }))
}

pub fn save_config(conn: &rusqlite::Connection, config: AiConfig) -> anyhow::Result<AiConfig> {
    let normalized = normalize_config(config);
    conn.execute(
        "insert into ai_config(id, provider, api_key, base_url, model, user_prompt, analysis_prompt, summary_prompt, analysis_prompt_custom, summary_prompt_custom, analysis_batch_size, enabled, test_status, updated_at)
         values(1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         on conflict(id) do update set
           provider = excluded.provider,
           api_key = excluded.api_key,
           base_url = excluded.base_url,
           model = excluded.model,
           user_prompt = excluded.user_prompt,
           analysis_prompt = excluded.analysis_prompt,
           summary_prompt = excluded.summary_prompt,
           analysis_prompt_custom = excluded.analysis_prompt_custom,
           summary_prompt_custom = excluded.summary_prompt_custom,
           analysis_batch_size = excluded.analysis_batch_size,
           enabled = excluded.enabled,
           test_status = excluded.test_status,
           updated_at = excluded.updated_at",
        params![
            &normalized.provider,
            &normalized.api_key,
            &normalized.base_url,
            &normalized.model,
            &normalized.user_prompt,
            &normalized.analysis_prompt,
            &normalized.summary_prompt,
            if normalized.analysis_prompt_custom { 1 } else { 0 },
            if normalized.summary_prompt_custom { 1 } else { 0 },
            normalized.analysis_batch_size,
            if normalized.enabled { 1 } else { 0 },
            &normalized.test_status,
            Local::now().to_rfc3339()
        ],
    )?;
    Ok(normalized)
}

pub(crate) fn normalize_config(config: AiConfig) -> AiConfig {
    // 提示词编辑框由前端维护，后端只归一化“是否自定义”的标记，不覆盖前端传入的提示词内容。
    let analysis_prompt_custom =
        config.analysis_prompt_custom && !is_known_default_analysis_prompt(&config.analysis_prompt);
    let summary_prompt_custom =
        config.summary_prompt_custom && !is_known_default_summary_prompt(&config.summary_prompt);
    let analysis_prompt = if config.analysis_prompt.trim().is_empty() {
        DEFAULT_ANALYSIS_PROMPT.to_owned()
    } else {
        config.analysis_prompt.clone()
    };
    let summary_prompt = if config.summary_prompt.trim().is_empty() {
        DEFAULT_SUMMARY_PROMPT.to_owned()
    } else {
        config.summary_prompt.clone()
    };
    let analysis_batch_size = normalize_analysis_batch_size(&config);
    AiConfig {
        user_prompt: config.user_prompt.trim().to_owned(),
        analysis_prompt,
        summary_prompt,
        analysis_prompt_custom,
        summary_prompt_custom,
        analysis_batch_size,
        ..config
    }
}

fn normalize_analysis_batch_size(config: &AiConfig) -> i64 {
    let requested = if config.analysis_batch_size <= 0 {
        default_analysis_batch_size(config)
    } else {
        config.analysis_batch_size
    };
    requested.clamp(
        MIN_ANALYSIS_BATCH_MESSAGES,
        max_analysis_batch_messages(config),
    )
}

fn default_analysis_batch_size(config: &AiConfig) -> i64 {
    if is_managed_local_deepseek(config) {
        LOCAL_DEEPSEEK_ANALYSIS_BATCH_SIZE
    } else {
        OTHER_MODEL_DEFAULT_ANALYSIS_BATCH_SIZE
    }
}

fn max_analysis_batch_messages(config: &AiConfig) -> i64 {
    if is_managed_local_deepseek(config) {
        LOCAL_DEEPSEEK_MAX_ANALYSIS_BATCH_MESSAGES
    } else {
        OTHER_MODEL_MAX_ANALYSIS_BATCH_MESSAGES
    }
}

fn is_managed_local_deepseek(config: &AiConfig) -> bool {
    config.provider == LOCAL_DEEPSEEK_PROVIDER
}

pub(crate) fn analysis_batch_token_budget(config: &AiConfig) -> usize {
    let full_context_budget = if is_managed_local_deepseek(config) {
        LOCAL_DEEPSEEK_ANALYSIS_BATCH_ESTIMATED_TOKENS
    } else {
        OTHER_MODEL_ANALYSIS_BATCH_ESTIMATED_TOKENS
    };
    let output_budget = if is_managed_local_deepseek(config) {
        LOCAL_DEEPSEEK_ANALYSIS_OUTPUT_TOKENS
    } else {
        OTHER_MODEL_ANALYSIS_OUTPUT_TOKENS
    };
    // 切批预算按完整请求反推：总上下文先扣掉输出空间和固定提示词/JSON结构余量，剩余才给 messages。
    // 这样小模型不会因为提示词、已有事项或历史上下文挤占而把消息批次塞满到上下文边界。
    full_context_budget
        .saturating_sub(output_budget)
        .saturating_sub(ANALYSIS_REQUEST_RESERVED_TOKENS)
        .max(1_200)
}

pub fn is_local_provider(config: &AiConfig) -> bool {
    config.provider.contains("本地")
        || config.base_url.contains("127.0.0.1")
        || config.base_url.contains("localhost")
}

pub fn with_provider_headers(
    request: reqwest::RequestBuilder,
    config: &AiConfig,
    base_url: &str,
) -> reqwest::RequestBuilder {
    if is_openrouter_config(config, base_url) {
        return request.header("X-OpenRouter-Title", "IM-Board");
    }
    request
}

fn is_openrouter_config(config: &AiConfig, base_url: &str) -> bool {
    config.provider == OPENROUTER_PROVIDER || base_url.contains("openrouter.ai")
}

pub fn is_configured(config: &AiConfig) -> bool {
    if !config.enabled || config.model.trim().is_empty() {
        return false;
    }
    is_local_provider(config) || !config.api_key.trim().is_empty()
}

fn infer_prompt_custom(
    stored_flag: Option<i64>,
    value: &str,
    is_known_default: fn(&str) -> bool,
) -> bool {
    match stored_flag {
        Some(flag) => flag == 1 && !is_known_default(value),
        None => !value.trim().is_empty() && !is_known_default(value),
    }
}

fn is_known_default_analysis_prompt(value: &str) -> bool {
    let trimmed = value.trim();
    let is_old_default = value.contains("话题识别和计数规则")
        && value.contains("关键词词云识别和计数规则")
        && value.contains("openActionItems");
    let missing_language_constraint = trimmed.starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && !value.contains("所有输出字段必须使用简体中文")
        && !value.contains("主要语言");
    let fixed_chinese_only_language_constraint = trimmed
        .starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && value.contains("所有输出字段必须使用简体中文")
        && !value.contains("主要语言");
    let date_limited_default = trimmed.starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && (value.contains("今天聊天消息")
            || value.contains("尚未分析的今天消息")
            || value.contains("今天已有的待回复/待办"));
    let missing_wechat_system_account_filter = trimmed
        .starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && value.contains("管理介入/情绪风险规则")
        && value.contains("批内去重与历史参考规则")
        && !value.contains("filehelper")
        && !value.contains("notification_messages");
    let missing_single_chat_file_reply_rule = trimmed.starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && value.contains("待我回复")
        && value.contains("批内去重与历史参考规则")
        && !value.contains("单聊里对方发送文件");
    trimmed.is_empty()
        || trimmed == DEFAULT_ANALYSIS_PROMPT.trim()
        || is_old_default
        || missing_language_constraint
        || fixed_chinese_only_language_constraint
        || date_limited_default
        || missing_wechat_system_account_filter
        || missing_single_chat_file_reply_rule
}

fn is_known_default_summary_prompt(value: &str) -> bool {
    let trimmed = value.trim();
    let old_frontend_default = trimmed
        .starts_with("你是一个本地即时通讯工作助理。请根据输入的今天聊天消息生成看板话题。")
        && value.contains("existingTopics是今天已经汇总好的热门话题")
        && value.contains("sourceChats只能使用existingTopics或candidateTopics中的chatName");
    let date_limited_default = trimmed.starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && value.contains("candidateTopics")
        && (value.contains("今天聊天消息")
            || value.contains("今天已经汇总好的热门话题")
            || value.contains("群聊当天总消息数"));
    let missing_primary_language_constraint = trimmed.starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && value.contains("candidateTopics")
        && !value.contains("主要语言");
    let missing_keyword_refine_section = trimmed.starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && value.contains("candidateTopics")
        && !value.contains("keywordRefine");
    let old_keyword_refine_candidates_prompt = trimmed
        .starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && value.contains("keywordRefine.candidates");
    let old_mixed_dedup_prompt = trimmed.starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && (value.contains("不要参考本地识别词，不要合并旧关键词")
            || !value.contains("旧话题去重合并规则"));
    let missing_wechat_system_account_filter = trimmed
        .starts_with("你是一个本地即时通讯工作助理。")
        && value.contains("请返回严格 JSON")
        && value.contains("candidateTopics")
        && value.contains("旧话题去重合并规则")
        && value.contains("关键词词云识别规则")
        && (!value.contains("filehelper") || !value.contains("notification_messages"));
    trimmed.is_empty()
        || trimmed == DEFAULT_SUMMARY_PROMPT.trim()
        || old_frontend_default
        || date_limited_default
        || missing_primary_language_constraint
        || missing_keyword_refine_section
        || old_keyword_refine_candidates_prompt
        || old_mixed_dedup_prompt
        || missing_wechat_system_account_filter
}
