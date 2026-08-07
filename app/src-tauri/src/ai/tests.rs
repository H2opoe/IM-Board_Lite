use std::collections::HashSet;

use jieba_rs::Jieba;
use rusqlite::params;

use super::*;
use crate::analysis::local_keywords::{
    add_context_keyword, is_selectable_local_keyword, keyword_texts_from_message_content,
    local_keyword_candidates, select_local_keyword_ranks, LocalKeywordRank, LocalKeywordScore,
};

include!("tests/keywords.rs");
include!("tests/batching_config.rs");
include!("tests/message_summary.rs");
include!("tests/topic_merge.rs");
include!("tests/action_items.rs");
