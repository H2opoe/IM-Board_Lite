use super::*;

const EDITED_ANALYSIS_PROMPT: &str = "你是一个本地即时通讯工作助理。只处理今天聊天消息中的采购事项。请返回严格 JSON；按采购金额排序，这是用户的自定义要求。";
const EDITED_SUMMARY_PROMPT: &str = "你是一个本地即时通讯工作助理。按采购部门汇总 candidateTopics。请返回严格 JSON；保留供应商名称，这是用户的自定义要求。";
const LEGACY_ANALYSIS_DEFAULT: &str = include_str!("fixtures/legacy_analysis_default.txt");

fn prompt_config_fixture(analysis: &str, summary: &str, flag: Option<i64>) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("fixture database");
    conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
        .expect("fixture schema");
    conn.execute(
        "insert into ai_config(id, analysis_prompt, summary_prompt,
          analysis_prompt_custom, summary_prompt_custom, updated_at)
         values (1, ?1, ?2, ?3, ?3, 'fixture')",
        params![analysis, summary, flag],
    )
    .expect("fixture config");
    conn
}

#[test]
fn prompt_config_normalize_preserves_explicit_edits_to_old_style_prompts() {
    let mut config = default_config();
    config.analysis_prompt = EDITED_ANALYSIS_PROMPT.to_owned();
    config.summary_prompt = EDITED_SUMMARY_PROMPT.to_owned();
    config.analysis_prompt_custom = true;
    config.summary_prompt_custom = true;

    let normalized = normalize_config(config);
    assert_eq!(normalized.analysis_prompt, EDITED_ANALYSIS_PROMPT);
    assert_eq!(normalized.summary_prompt, EDITED_SUMMARY_PROMPT);
    assert!(normalized.analysis_prompt_custom);
    assert!(normalized.summary_prompt_custom);
}

#[test]
fn prompt_config_database_preserves_explicit_edits_to_old_style_prompts() {
    let conn = prompt_config_fixture(EDITED_ANALYSIS_PROMPT, EDITED_SUMMARY_PROMPT, Some(1));
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, EDITED_ANALYSIS_PROMPT);
    assert_eq!(loaded.summary_prompt, EDITED_SUMMARY_PROMPT);
    assert!(loaded.analysis_prompt_custom);
    assert!(loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_explicit_builtin_copy_remains_custom() {
    let conn = prompt_config_fixture(DEFAULT_ANALYSIS_PROMPT, DEFAULT_SUMMARY_PROMPT, Some(1));
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, DEFAULT_ANALYSIS_PROMPT);
    assert_eq!(loaded.summary_prompt, DEFAULT_SUMMARY_PROMPT);
    assert!(loaded.analysis_prompt_custom);
    assert!(loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_empty_custom_text_uses_defaults() {
    for text in ["", " \n\t "] {
        let conn = prompt_config_fixture(text, text, Some(1));
        let loaded = get_config(&conn).expect("load fixture config");
        assert_eq!(loaded.analysis_prompt, DEFAULT_ANALYSIS_PROMPT);
        assert_eq!(loaded.summary_prompt, DEFAULT_SUMMARY_PROMPT);
        assert!(!loaded.analysis_prompt_custom);
        assert!(!loaded.summary_prompt_custom);
    }
}

#[test]
fn prompt_config_legacy_null_flag_preserves_unverified_old_style_text() {
    let analysis = "你是一个本地即时通讯工作助理。请分析今天聊天消息。请返回严格 JSON。";
    let summary =
        "你是一个本地即时通讯工作助理。请汇总今天聊天消息中的 candidateTopics。请返回严格 JSON。";
    let conn = prompt_config_fixture(analysis, summary, None);
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, analysis);
    assert_eq!(loaded.summary_prompt, summary);
    assert!(loaded.analysis_prompt_custom);
    assert!(loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_legacy_null_flag_preserves_distinct_custom_text() {
    let conn = prompt_config_fixture("采购分析专用规则", "供应商汇总专用规则", None);
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, "采购分析专用规则");
    assert_eq!(loaded.summary_prompt, "供应商汇总专用规则");
    assert!(loaded.analysis_prompt_custom);
    assert!(loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_explicit_default_flag_follows_builtin_updates() {
    let conn = prompt_config_fixture("采购分析专用规则", "供应商汇总专用规则", Some(0));
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, DEFAULT_ANALYSIS_PROMPT);
    assert_eq!(loaded.summary_prompt, DEFAULT_SUMMARY_PROMPT);
    assert!(!loaded.analysis_prompt_custom);
    assert!(!loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_legacy_null_flag_preserves_edits_to_old_style_prompts() {
    let conn = prompt_config_fixture(EDITED_ANALYSIS_PROMPT, EDITED_SUMMARY_PROMPT, None);
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, EDITED_ANALYSIS_PROMPT);
    assert_eq!(loaded.summary_prompt, EDITED_SUMMARY_PROMPT);
    assert!(loaded.analysis_prompt_custom);
    assert!(loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_legacy_null_flag_upgrades_verified_complete_default() {
    let conn = prompt_config_fixture(LEGACY_ANALYSIS_DEFAULT, DEFAULT_SUMMARY_PROMPT, None);
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, DEFAULT_ANALYSIS_PROMPT);
    assert_eq!(loaded.summary_prompt, DEFAULT_SUMMARY_PROMPT);
    assert!(!loaded.analysis_prompt_custom);
    assert!(!loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_explicit_historical_default_copy_remains_custom() {
    let conn = prompt_config_fixture(LEGACY_ANALYSIS_DEFAULT, DEFAULT_SUMMARY_PROMPT, Some(1));
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, LEGACY_ANALYSIS_DEFAULT);
    assert_eq!(loaded.summary_prompt, DEFAULT_SUMMARY_PROMPT);
    assert!(loaded.analysis_prompt_custom);
    assert!(loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_null_current_defaults_follow_builtin_updates() {
    let conn = prompt_config_fixture(DEFAULT_ANALYSIS_PROMPT, DEFAULT_SUMMARY_PROMPT, None);
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, DEFAULT_ANALYSIS_PROMPT);
    assert_eq!(loaded.summary_prompt, DEFAULT_SUMMARY_PROMPT);
    assert!(!loaded.analysis_prompt_custom);
    assert!(!loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_null_default_recognition_uses_rust_unicode_trim() {
    let analysis = format!("\u{2003}\n{LEGACY_ANALYSIS_DEFAULT}\r\n\u{3000}");
    let summary = format!("\u{2003}{DEFAULT_SUMMARY_PROMPT}\u{3000}");
    let conn = prompt_config_fixture(&analysis, &summary, None);
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, DEFAULT_ANALYSIS_PROMPT);
    assert_eq!(loaded.summary_prompt, DEFAULT_SUMMARY_PROMPT);
    assert!(!loaded.analysis_prompt_custom);
    assert!(!loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_null_preserves_substrings_and_appended_user_changes() {
    for (analysis, summary) in [
        (
            format!("{LEGACY_ANALYSIS_DEFAULT}\n只处理采购金额超过一万元的任务。"),
            format!("{DEFAULT_SUMMARY_PROMPT}\n供应商名称必须保留。"),
        ),
        (
            format!("个人规则\n{LEGACY_ANALYSIS_DEFAULT}"),
            format!("个人规则\n{DEFAULT_SUMMARY_PROMPT}"),
        ),
        (
            LEGACY_ANALYSIS_DEFAULT.replace("真正需要用户处理", "需要采购部门处理"),
            DEFAULT_SUMMARY_PROMPT.replace("生成看板话题", "生成采购部门看板话题"),
        ),
    ] {
        let conn = prompt_config_fixture(&analysis, &summary, None);
        let loaded = get_config(&conn).expect("load fixture config");
        assert_eq!(loaded.analysis_prompt, analysis);
        assert_eq!(loaded.summary_prompt, summary);
        assert!(loaded.analysis_prompt_custom);
        assert!(loaded.summary_prompt_custom);
    }
}

#[test]
fn prompt_config_null_does_not_guess_unverified_line_ending_variants() {
    let analysis = LEGACY_ANALYSIS_DEFAULT.replace('\n', "\r\n");
    let summary = DEFAULT_SUMMARY_PROMPT.replace('\n', "\r\n");
    let conn = prompt_config_fixture(&analysis, &summary, None);
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, analysis);
    assert_eq!(loaded.summary_prompt, summary);
    assert!(loaded.analysis_prompt_custom);
    assert!(loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_null_does_not_strip_non_whitespace_characters() {
    let analysis = format!("{LEGACY_ANALYSIS_DEFAULT}\u{200b}");
    let summary = format!("{DEFAULT_SUMMARY_PROMPT}\u{200b}");
    let conn = prompt_config_fixture(&analysis, &summary, None);
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, analysis);
    assert_eq!(loaded.summary_prompt, summary);
    assert!(loaded.analysis_prompt_custom);
    assert!(loaded.summary_prompt_custom);
}

#[test]
fn prompt_config_null_and_explicit_default_empty_values_use_defaults() {
    for flag in [None, Some(0)] {
        let conn = prompt_config_fixture("\u{2003}\n\t", "\u{3000}\r\n", flag);
        let loaded = get_config(&conn).expect("load fixture config");
        assert_eq!(loaded.analysis_prompt, DEFAULT_ANALYSIS_PROMPT);
        assert_eq!(loaded.summary_prompt, DEFAULT_SUMMARY_PROMPT);
        assert!(!loaded.analysis_prompt_custom);
        assert!(!loaded.summary_prompt_custom);
    }
}

#[test]
fn prompt_config_null_read_keeps_stored_text_and_flags_unchanged() {
    for (analysis, summary) in [
        (EDITED_ANALYSIS_PROMPT, EDITED_SUMMARY_PROMPT),
        (LEGACY_ANALYSIS_DEFAULT, DEFAULT_SUMMARY_PROMPT),
    ] {
        let conn = prompt_config_fixture(analysis, summary, None);
        let _ = get_config(&conn).expect("load fixture config");
        let stored: (String, String, Option<i64>, Option<i64>) = conn
            .query_row(
                "select analysis_prompt, summary_prompt, analysis_prompt_custom, summary_prompt_custom from ai_config where id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("read original database row");
        assert_eq!(
            stored,
            (analysis.to_owned(), summary.to_owned(), None, None)
        );
    }
}

#[test]
fn prompt_config_mixed_flags_are_resolved_independently() {
    let conn = prompt_config_fixture(EDITED_ANALYSIS_PROMPT, EDITED_SUMMARY_PROMPT, None);
    conn.execute(
        "update ai_config set summary_prompt_custom = 0 where id = 1",
        [],
    )
    .expect("set independent summary flag");
    let loaded = get_config(&conn).expect("load fixture config");
    assert_eq!(loaded.analysis_prompt, EDITED_ANALYSIS_PROMPT);
    assert!(loaded.analysis_prompt_custom);
    assert_eq!(loaded.summary_prompt, DEFAULT_SUMMARY_PROMPT);
    assert!(!loaded.summary_prompt_custom);
}
