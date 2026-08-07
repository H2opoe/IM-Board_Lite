use std::collections::HashSet;
use std::sync::OnceLock;

fn data_set(
    data: &'static str,
    cache: &'static OnceLock<HashSet<&'static str>>,
) -> &'static HashSet<&'static str> {
    cache.get_or_init(|| {
        data.lines()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect()
    })
}

pub(crate) fn is_local_stopword(value: &str) -> bool {
    static STOPWORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    data_set(include_str!("stopwords.txt"), &STOPWORDS).contains(value)
        || value.starts_with("msg")
        || value.contains("哈哈")
}

pub(crate) fn is_generic_single_term(value: &str) -> bool {
    static GENERIC_TERMS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    data_set(include_str!("generic_terms.txt"), &GENERIC_TERMS).contains(value.trim())
}

pub(crate) fn is_low_semantic_keyword(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.is_empty()
        || matches!(value.as_str(), "rmb" | "cny" | "usd")
        || looks_like_amount_shorthand(&value)
}

fn looks_like_amount_shorthand(value: &str) -> bool {
    let chars = value.chars().collect::<Vec<_>>();
    if !(2..=5).contains(&chars.len()) {
        return false;
    }
    let digit_count = chars.iter().filter(|ch| ch.is_ascii_digit()).count();
    let measure_count = chars
        .iter()
        .filter(|ch| {
            matches!(
                ch,
                '个' | '份' | '件' | '只' | '支' | '包' | '箱' | '元' | '块'
            )
        })
        .count();
    digit_count > 0
        && measure_count > 0
        && chars.iter().all(|ch| {
            ch.is_ascii_digit()
                || matches!(
                    ch,
                    '个' | '份' | '件' | '只' | '支' | '包' | '箱' | '元' | '块'
                )
        })
}

pub(crate) fn is_meaningful_compound_term(value: &str) -> bool {
    let value = value.trim();
    if is_generic_single_term(value) || value.chars().count() < 3 {
        return false;
    }
    ["公司", "系统", "市场", "问题"]
        .iter()
        .any(|root| value.contains(root) && value != *root)
}
