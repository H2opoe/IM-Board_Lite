use std::collections::{HashMap, HashSet};

use jieba_rs::Jieba;

use super::dictionary::LOCAL_KEYWORD_SEED_TERMS;
use super::types::{LocalKeywordCandidate, LocalKeywordSource};

const LOCAL_CONTEXT_WORD_FREQ: usize = 500_000;

pub(crate) fn build_local_keyword_segmenter() -> (Jieba, HashSet<String>) {
    let mut jieba = Jieba::new();
    let mut context_terms = HashSet::new();

    for term in LOCAL_KEYWORD_SEED_TERMS {
        add_context_keyword(&mut jieba, &mut context_terms, term);
    }

    (jieba, context_terms)
}
pub(crate) fn add_context_keyword(
    jieba: &mut Jieba,
    context_terms: &mut HashSet<String>,
    value: &str,
) {
    for term in context_term_candidates(value) {
        if context_terms.insert(term.clone()) {
            jieba.add_word(&term, Some(LOCAL_CONTEXT_WORD_FREQ), Some("n"));
        }
    }
}

fn context_term_candidates(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    push_context_term(value, &mut terms);

    let mut buffer = String::new();
    for ch in value.chars() {
        if is_context_separator(ch) {
            push_context_term(&buffer, &mut terms);
            buffer.clear();
        } else {
            buffer.push(ch);
        }
    }
    push_context_term(&buffer, &mut terms);
    dedupe_strings(terms)
}

fn push_context_term(value: &str, terms: &mut Vec<String>) {
    let Some(token) = normalize_keyword_candidate(value) else {
        return;
    };
    let chars = keyword_char_count(&token);
    if !(2..=24).contains(&chars) || looks_like_noise_keyword(&token) || is_local_stopword(&token) {
        return;
    }
    terms.push(token);
}

fn is_context_separator(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '/' | '\\'
                | '|'
                | ','
                | '，'
                | '、'
                | ';'
                | '；'
                | ':'
                | '：'
                | '·'
                | '-'
                | '_'
                | '('
                | ')'
                | '（'
                | '）'
                | '['
                | ']'
                | '【'
                | '】'
                | '{'
                | '}'
                | '《'
                | '》'
                | '<'
                | '>'
        )
}
pub(crate) fn local_keyword_candidates(
    jieba: &Jieba,
    content: &str,
    context_terms: &HashSet<String>,
) -> Vec<LocalKeywordCandidate> {
    let mut candidates = Vec::new();
    let mut phrase_tokens = Vec::<String>::new();

    let compact_content = content
        .chars()
        .filter(|ch| is_keyword_body_char(*ch) && !is_ignored_cjk_particle(*ch))
        .flat_map(char::to_lowercase)
        .collect::<String>();
    for term in context_terms {
        if compact_content.contains(term) {
            candidates.push(LocalKeywordCandidate {
                text: term.clone(),
                source: LocalKeywordSource::Context,
            });
        }
    }

    for tag in jieba.tag(content, true) {
        if let Some(token) = normalize_keyword_candidate(tag.word) {
            if should_keep_segment_token(&token, tag.tag, context_terms) {
                candidates.push(LocalKeywordCandidate {
                    text: token.clone(),
                    source: if context_terms.contains(&token) {
                        LocalKeywordSource::Context
                    } else {
                        LocalKeywordSource::Segment
                    },
                });
                phrase_tokens.push(token);
                continue;
            }
        }
        if tag.word.chars().all(is_ignored_cjk_particle) {
            continue;
        }
        phrase_tokens.push(String::new());
    }

    candidates.extend(phrase_keyword_candidates(&phrase_tokens));

    for word in jieba.cut_for_search(content, true) {
        let Some(token) = normalize_keyword_candidate(word) else {
            continue;
        };
        if should_keep_search_token(&token, context_terms) {
            candidates.push(LocalKeywordCandidate {
                text: token,
                source: LocalKeywordSource::Search,
            });
        }
    }

    let has_primary_candidates = candidates
        .iter()
        .any(|candidate| candidate.source.rank() >= LocalKeywordSource::Segment.rank());
    if !has_primary_candidates {
        candidates.extend(
            fallback_keyword_candidates(content)
                .into_iter()
                .map(|text| LocalKeywordCandidate {
                    text,
                    source: LocalKeywordSource::Fallback,
                }),
        );
    }

    dedupe_candidates(candidates)
}

fn phrase_keyword_candidates(tokens: &[String]) -> Vec<LocalKeywordCandidate> {
    let mut candidates = Vec::new();
    for size in 2..=3 {
        for window in tokens.windows(size) {
            if window.iter().any(|token| token.is_empty()) {
                continue;
            }
            let phrase = window.join("");
            let Some(token) = normalize_keyword_candidate(&phrase) else {
                continue;
            };
            let chars = keyword_char_count(&token);
            if !(3..=12).contains(&chars)
                || looks_like_noise_keyword(&token)
                || is_local_stopword(&token)
                || is_generic_single_term(&token)
            {
                continue;
            }
            candidates.push(LocalKeywordCandidate {
                text: token,
                source: LocalKeywordSource::Phrase,
            });
        }
    }
    candidates
}

fn fallback_keyword_candidates(content: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut latin = String::new();
    let mut cjk = String::new();

    for ch in content.chars() {
        if is_keyword_latin(ch) {
            flush_cjk_candidates(&mut cjk, &mut candidates);
            latin.push(ch.to_ascii_lowercase());
        } else if is_cjk(ch) {
            flush_latin_candidate(&mut latin, &mut candidates);
            if !is_ignored_cjk_particle(ch) {
                cjk.push(ch);
            }
        } else {
            flush_latin_candidate(&mut latin, &mut candidates);
            flush_cjk_candidates(&mut cjk, &mut candidates);
        }
    }
    flush_latin_candidate(&mut latin, &mut candidates);
    flush_cjk_candidates(&mut cjk, &mut candidates);
    dedupe_strings(candidates)
}

fn flush_latin_candidate(buffer: &mut String, candidates: &mut Vec<String>) {
    let token = normalize_keyword_candidate(buffer);
    buffer.clear();
    let Some(token) = token else {
        return;
    };
    if keyword_char_count(&token) < 2
        || looks_like_noise_keyword(&token)
        || is_local_stopword(&token)
    {
        return;
    }
    candidates.push(token);
}

fn flush_cjk_candidates(buffer: &mut String, candidates: &mut Vec<String>) {
    let chars = buffer.chars().collect::<Vec<_>>();
    buffer.clear();
    if chars.len() < 2 {
        return;
    }

    // fallback 只保留完整短中文片段，禁止对整段中文做滑窗，避免制造伪关键词。
    if chars.len() <= 6 {
        push_cjk_candidate(chars.iter().collect::<String>(), candidates);
    }
}

fn push_cjk_candidate(value: String, candidates: &mut Vec<String>) {
    let Some(token) = normalize_cjk_candidate(&value) else {
        return;
    };
    if is_local_stopword(&token) || is_generic_single_term(&token) {
        return;
    }
    candidates.push(token);
}

fn normalize_cjk_candidate(value: &str) -> Option<String> {
    let token = normalize_keyword_candidate(value)?;
    if keyword_char_count(&token) < 2 || is_local_stopword(&token) || is_generic_single_term(&token)
    {
        return None;
    }
    if token.chars().all(is_weak_cjk_char) {
        return None;
    }
    Some(token)
}

fn normalize_keyword_candidate(value: &str) -> Option<String> {
    let mut token = value
        .trim_matches(|ch: char| !is_keyword_body_char(ch))
        .chars()
        .filter(|ch| is_keyword_body_char(*ch))
        .flat_map(char::to_lowercase)
        .collect::<String>();
    token = token.trim_matches(is_weak_keyword_edge).to_owned();

    if token.is_empty()
        || keyword_char_count(&token) < 2
        || looks_like_noise_keyword(&token)
        || is_local_stopword(&token)
        || is_generic_single_term(&token)
        || token.chars().all(is_weak_cjk_char)
    {
        return None;
    }
    Some(token)
}

fn should_keep_segment_token(token: &str, tag: &str, context_terms: &HashSet<String>) -> bool {
    if context_terms.contains(token) {
        return true;
    }
    if looks_like_noise_keyword(token) || is_local_stopword(token) || is_generic_single_term(token)
    {
        return false;
    }
    if is_ascii_keyword(token) {
        return is_strong_ascii_keyword(token);
    }
    if token.chars().any(|ch| ch.is_ascii_digit()) && token.chars().any(is_cjk) {
        return true;
    }
    let chars = keyword_char_count(token);
    if chars < 2 || token.chars().all(is_weak_cjk_char) {
        return false;
    }
    is_informative_jieba_tag(tag) || is_meaningful_compound_term(token)
}

fn should_keep_search_token(token: &str, context_terms: &HashSet<String>) -> bool {
    if context_terms.contains(token) {
        return true;
    }
    if looks_like_noise_keyword(token) || is_local_stopword(token) || is_generic_single_term(token)
    {
        return false;
    }
    if is_ascii_keyword(token) {
        return is_strong_ascii_keyword(token);
    }
    keyword_char_count(token) >= 3
}

fn is_informative_jieba_tag(tag: &str) -> bool {
    matches!(tag, "n" | "nz" | "nt" | "eng" | "vn" | "j")
}
pub(crate) fn keyword_char_count(value: &str) -> usize {
    value.chars().count()
}

pub(super) fn is_ascii_keyword(value: &str) -> bool {
    value.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn is_strong_ascii_keyword(value: &str) -> bool {
    let chars = keyword_char_count(value);
    chars >= 3 && !looks_like_noise_keyword(value) && !is_local_stopword(value)
}

fn is_keyword_latin(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '#' | '+')
}

fn is_keyword_body_char(ch: char) -> bool {
    is_keyword_latin(ch) || is_cjk(ch)
}

pub(crate) fn looks_like_noise_keyword(value: &str) -> bool {
    value.len() > 64
        || value.starts_with("http")
        || looks_like_domain_keyword(value)
        || looks_like_technical_payload_keyword(value)
        || value.starts_with("msg")
        || value.starts_with("local_")
        || value.starts_with("wxid")
        || value.starts_with("gh_")
        || value.starts_with("gh-")
        || value
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '_' | '.' | '#' | '+'))
}

fn looks_like_technical_payload_keyword(value: &str) -> bool {
    const TECHNICAL_FRAGMENTS: &[&str] = &[
        "darkmode",
        "selfintroducetext",
        "openedbyminiapp",
        "needredirect",
        "containertype",
        "slidepaneloption",
        "redirecturl",
        "hrmregister",
        "empprofile",
        "groupwelcome",
        "dingtalkclient",
        "openapp",
    ];
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return false;
    }
    if TECHNICAL_FRAGMENTS
        .iter()
        .any(|fragment| value.contains(fragment))
    {
        return true;
    }

    let chars = value.chars().count();
    let digit_count = value.chars().filter(|ch| ch.is_ascii_digit()).count();
    if chars >= 12 && digit_count * 2 >= chars {
        return true;
    }
    if chars >= 8
        && ["22", "26", "3a", "3f", "7b"]
            .iter()
            .any(|prefix| value.starts_with(prefix))
        && digit_count > 0
        && value.chars().any(|ch| ch.is_ascii_alphabetic())
    {
        return true;
    }
    if chars >= 10
        && value.contains("26")
        && ["cid", "false", "true", "profile", "group"]
            .iter()
            .any(|fragment| value.contains(fragment))
    {
        return true;
    }
    value.starts_with("dding") && chars >= 12 && digit_count >= 4
}

fn looks_like_domain_keyword(value: &str) -> bool {
    value.contains('.')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '/'))
        && value.split('.').filter(|part| !part.is_empty()).count() >= 2
}

fn is_ignored_cjk_particle(ch: char) -> bool {
    matches!(
        ch,
        '的' | '了' | '吗' | '呢' | '吧' | '啊' | '哦' | '呀' | '哟'
    )
}

fn is_weak_keyword_edge(ch: char) -> bool {
    matches!(
        ch,
        '我' | '你'
            | '他'
            | '她'
            | '它'
            | '们'
            | '把'
            | '被'
            | '给'
            | '在'
            | '是'
            | '有'
            | '就'
            | '都'
            | '也'
            | '还'
            | '要'
            | '能'
            | '会'
            | '去'
            | '来'
            | '请'
            | '将'
            | '和'
            | '与'
            | '及'
            | '再'
            | '又'
            | '先'
            | '后'
            | '让'
            | '用'
            | '跟'
            | '到'
            | '等'
            | '个'
            | '很'
            | '太'
            | '更'
            | '真'
    )
}

fn is_weak_cjk_char(ch: char) -> bool {
    is_weak_keyword_edge(ch)
        || matches!(
            ch,
            '可' | '以'
                | '不'
                | '没'
                | '嘛'
                | '啥'
                | '呢'
                | '啦'
                | '哈'
                | '哦'
                | '嗯'
                | '好'
                | '行'
                | '啊'
        )
}

pub(crate) fn is_local_stopword(value: &str) -> bool {
    const STOPWORDS: &[&str] = &[
        "http",
        "https",
        "www",
        "com",
        "cn",
        "local",
        "local_id",
        "true",
        "false",
        "null",
        "xml",
        "version",
        "type",
        "sysmsg",
        "revokemsg",
        "revoketime",
        "appmsg",
        "videomsg",
        "img",
        "emoji",
        "aeskey",
        "cdnthumburl",
        "cdnvideourl",
        "fromusername",
        "newmd5",
        "rawmd5",
        "md5",
        "appid",
        "sdkver",
        "ion",
        "rs",
        "im",
        "content",
        "body",
        "board",
        "darkmode",
        "containertype",
        "selfintroducetext",
        "openedbyminiapp",
        "needredirect",
        "slidepaneloption",
        "redirecturl",
        "dingtalkclient",
        "openapp",
        "hrmregister",
        "empprofile",
        "groupwelcome",
        "cid",
        "corpid",
        "message",
        "messages",
        "data",
        "items",
        "records",
        "chat",
        "chatid",
        "chat_id",
        "chatname",
        "chat_name",
        "chatroom",
        "group",
        "groupid",
        "group_id",
        "groupname",
        "group_name",
        "sender",
        "senderid",
        "sender_id",
        "sendername",
        "sender_name",
        "user",
        "userid",
        "user_id",
        "username",
        "user_name",
        "nickname",
        "displayname",
        "display_name",
        "profile",
        "profileid",
        "profile_id",
        "platform",
        "msgtype",
        "msg_type",
        "rawtype",
        "raw_type",
        "rawjson",
        "raw_json",
        "timestamp",
        "timetext",
        "time_text",
        "localid",
        "contenthash",
        "content_hash",
        "text",
        "image",
        "voice",
        "video",
        "emoji",
        "file",
        "audio",
        "msg",
        "url",
        "link",
        "ok",
        "okay",
        "yes",
        "no",
        "这个",
        "那个",
        "这些",
        "那些",
        "今天",
        "今天上午",
        "今天下午",
        "今天中午",
        "今天晚上",
        "明天",
        "昨天",
        "现在",
        "刚刚",
        "一下",
        "等下",
        "等等",
        "然后",
        "因为",
        "所以",
        "但是",
        "如果",
        "还是",
        "就是",
        "不是",
        "没有",
        "可以",
        "不能",
        "不用",
        "不要",
        "已经",
        "觉得",
        "感觉",
        "知道",
        "看到",
        "收到",
        "回复",
        "处理",
        "消息",
        "聊天",
        "内容",
        "事情",
        "问题",
        "情况",
        "时间",
        "时候",
        "公司",
        "系统",
        "市场",
        "新鲜",
        "上午",
        "下午",
        "晚上",
        "中午",
        "好的",
        "好哟",
        "哈哈",
        "哈哈哈",
        "哈哈哈哈",
        "帮我",
        "我把",
        "你把",
        "我们",
        "你们",
        "他们",
        "她们",
        "什么",
        "怎么",
        "这样",
        "那样",
        "这里",
        "那里",
        "链接",
        "领取",
        "连续",
        "解锁",
        "连续签到",
        "连续签到解锁",
        "签到",
        "积分",
        "积分商城",
        "商城",
        "好礼",
        "邀请",
        "好友",
        "入群",
        "奖励",
        "有效期",
        "兑换",
        "点击",
        "点击领取",
        "点击进入",
        "二维码",
        "扫码",
        "扫描",
        "通过扫描",
        "加入群聊",
        "群聊",
        "卡值",
        "精彩",
        "礼品",
        "恭喜",
        "完成",
        "今日",
        "答题",
        "获奖",
        "答案",
        "记录",
        "直接",
        "真的",
        "可能",
        "需要",
        "进行",
        "过去",
        "回来",
        "起来",
        "出来",
        "一个",
        "两个",
        "几个",
    ];
    STOPWORDS.contains(&value)
        || value.starts_with("msg")
        || value.starts_with("wxid")
        || value.starts_with("gh_")
        || value.contains("哈哈")
}

pub(crate) fn is_generic_single_term(value: &str) -> bool {
    const GENERIC_SINGLE_TERMS: &[&str] = &[
        "二维码",
        "扫码",
        "扫描",
        "通过扫描",
        "加入群聊",
        "群聊",
        "时候",
        "公司",
        "系统",
        "市场",
        "今天下午",
        "今天上午",
        "今天中午",
        "今天晚上",
        "新鲜",
        "消息",
        "内容",
        "情况",
        "问题",
        "时间",
        "处理",
        "收到",
        "回复",
        "这个",
        "那个",
        "可以",
        "需要",
        "直接",
        "进行",
        "目前",
        "现在",
        "刚刚",
        "上午",
        "下午",
        "中午",
        "晚上",
    ];
    GENERIC_SINGLE_TERMS.contains(&value.trim())
}

pub(crate) fn is_meaningful_compound_term(value: &str) -> bool {
    let value = value.trim();
    if is_generic_single_term(value) || keyword_char_count(value) < 3 {
        return false;
    }
    const GENERIC_ROOTS: &[&str] = &["公司", "系统", "市场", "问题"];
    GENERIC_ROOTS
        .iter()
        .any(|root| value.contains(root) && value != *root)
}

fn dedupe_candidates(candidates: Vec<LocalKeywordCandidate>) -> Vec<LocalKeywordCandidate> {
    let mut seen = HashMap::<String, usize>::new();
    let mut deduped = Vec::<LocalKeywordCandidate>::new();
    for candidate in candidates {
        if let Some(index) = seen.get(&candidate.text).copied() {
            if candidate.source.rank() > deduped[index].source.rank() {
                deduped[index].source = candidate.source;
            }
            continue;
        }
        seen.insert(candidate.text.clone(), deduped.len());
        deduped.push(candidate);
    }
    deduped
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}

pub(crate) fn is_self_sender(sender_id: &str, sender_name: &str) -> bool {
    let sender_id = sender_id.trim().to_ascii_lowercase();
    let sender_name = sender_name.trim().to_ascii_lowercase();
    matches!(sender_id.as_str(), "me" | "self" | "我")
        || matches!(sender_name.as_str(), "me" | "self" | "我")
}

pub(crate) fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
        || ('\u{3400}'..='\u{4dbf}').contains(&ch)
        || ('\u{f900}'..='\u{faff}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::local_keywords::should_skip_keyword_message;

    fn candidate_texts(content: &str) -> Vec<String> {
        let (jieba, context_terms) = build_local_keyword_segmenter();
        local_keyword_candidates(&jieba, content, &context_terms)
            .into_iter()
            .map(|candidate| candidate.text)
            .collect()
    }

    #[test]
    fn skips_group_join_notice_before_segmentation() {
        assert!(should_skip_keyword_message(
            "张三通过扫描二维码加入群聊",
            None
        ));
        let texts = candidate_texts("张三通过扫描二维码加入群聊");
        assert!(
            !texts.contains(&"二维码加入群聊".to_owned()),
            "got {texts:?}"
        );
        assert!(!texts.contains(&"通过扫描".to_owned()), "got {texts:?}");
        assert!(!texts.contains(&"加入群聊".to_owned()), "got {texts:?}");
    }

    #[test]
    fn keeps_business_phrase_without_cjk_sliding_windows() {
        let texts = candidate_texts("下午货架自取的那批货到了吗");
        assert!(texts.contains(&"货架自取".to_owned()), "got {texts:?}");
        assert!(!texts.contains(&"下午货架".to_owned()), "got {texts:?}");
        assert!(!texts.contains(&"今天下午".to_owned()), "got {texts:?}");
    }

    #[test]
    fn filters_generic_terms_but_keeps_specific_compounds() {
        for term in ["公司", "今天下午", "系统", "问题"] {
            assert!(is_generic_single_term(term));
        }
        let texts = candidate_texts("订单系统今天又卡了");
        assert!(texts.contains(&"订单系统".to_owned()), "got {texts:?}");
        assert!(!texts.contains(&"系统".to_owned()), "got {texts:?}");
    }
}
