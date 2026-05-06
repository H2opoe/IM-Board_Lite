mod candidates;
mod dictionary;
mod repository;
mod scoring;
mod text_cleaning;
mod types;

pub(crate) use candidates::{
    build_local_keyword_segmenter, is_cjk, is_generic_single_term, is_local_stopword,
    is_low_semantic_keyword, is_self_sender, keyword_char_count, local_keyword_candidates,
    looks_like_noise_keyword,
};
pub(crate) use dictionary::{
    contains_any, ANALYSIS_COMPLETION_TERMS, ANALYSIS_REPLY_TERMS, ANALYSIS_RISK_TERMS,
    ANALYSIS_TASK_TERMS, ANALYSIS_URGENCY_TERMS,
};
pub(crate) use scoring::local_keywords;
pub(crate) use text_cleaning::{
    clean_link_or_app_message_text, clean_plain_message_text, contains_disallowed_media_content,
    contains_group_membership_notice, contains_unsupported_client_notice,
    keyword_texts_from_message_content, should_skip_keyword_message,
};

#[cfg(test)]
pub(crate) use candidates::add_context_keyword;
#[cfg(test)]
pub(crate) use scoring::{is_selectable_local_keyword, select_local_keyword_ranks};
#[cfg(test)]
pub(crate) use types::{LocalKeywordRank, LocalKeywordScore, LocalKeywordSource};
