use std::collections::HashSet;

#[derive(Debug, Default)]
pub(crate) struct LocalKeywordScore {
    pub(crate) score: f64,
    pub(crate) occurrences: i64,
    pub(crate) chat_ids: HashSet<String>,
    pub(crate) context_hits: i64,
    pub(crate) best_source: LocalKeywordSource,
    pub(crate) latest_timestamp: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalKeywordMessage {
    pub(crate) chat_id: String,
    pub(crate) content: String,
    pub(crate) timestamp: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalKeywordCandidate {
    pub(crate) text: String,
    pub(crate) source: LocalKeywordSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalKeywordSource {
    Context,
    Segment,
    Search,
    Phrase,
    Fallback,
}

impl Default for LocalKeywordSource {
    fn default() -> Self {
        Self::Fallback
    }
}

impl LocalKeywordSource {
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Context => 4,
            Self::Fallback => 0,
            Self::Search => 1,
            Self::Segment => 2,
            Self::Phrase => 3,
        }
    }

    pub(crate) fn multiplier(self) -> f64 {
        match self {
            Self::Context => 1.2,
            Self::Segment => 1.0,
            Self::Phrase => 0.9,
            Self::Search => 0.45,
            Self::Fallback => 0.1,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::Segment => "segment",
            Self::Search => "search",
            Self::Phrase => "phrase",
            Self::Fallback => "fallback",
        }
    }
}

#[derive(Debug)]
pub(crate) struct LocalKeywordRank {
    pub(crate) text: String,
    pub(crate) value: LocalKeywordScore,
    pub(crate) final_score: f64,
}
