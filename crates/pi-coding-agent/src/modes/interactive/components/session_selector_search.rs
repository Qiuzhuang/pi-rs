//! Port of `packages/coding-agent/src/modes/interactive/components/session-selector-search.ts`.

use crate::core::session_manager::SessionInfo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SortMode {
    #[serde(rename = "threaded")]
    Threaded,
    #[serde(rename = "recent")]
    Recent,
    #[serde(rename = "relevance")]
    Relevance,
}

impl SortMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Threaded => "threaded",
            Self::Recent => "recent",
            Self::Relevance => "relevance",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NameFilter {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "named")]
    Named,
}

impl NameFilter {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Named => "named",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParsedSearchQueryMode {
    #[serde(rename = "tokens")]
    Tokens,
    #[serde(rename = "regex")]
    Regex,
}

impl ParsedSearchQueryMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tokens => "tokens",
            Self::Regex => "regex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParsedSearchQueryTokensKind {
    #[serde(rename = "fuzzy")]
    Fuzzy,
    #[serde(rename = "phrase")]
    Phrase,
}

impl ParsedSearchQueryTokensKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fuzzy => "fuzzy",
            Self::Phrase => "phrase",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedSearchQueryTokens {
    pub kind: ParsedSearchQueryTokensKind,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct ParsedSearchQuery {
    pub mode: ParsedSearchQueryMode,
    pub tokens: Vec<ParsedSearchQueryTokens>,
    pub regex: Option<regex::Regex>,
    /// If set, parsing failed and we should treat query as non-matching.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchResult {
    pub matches: bool,
    /// Lower is better; only meaningful when matches === true
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub score: f64,
}

pub fn has_session_name(session: &SessionInfo) -> bool {
    todo!("port modes/interactive/components/session-selector-search.ts:hasSessionName")
}

pub fn parse_search_query(query: &str) -> ParsedSearchQuery {
    todo!("port modes/interactive/components/session-selector-search.ts:parseSearchQuery")
}

pub fn match_session(session: &SessionInfo, parsed: &ParsedSearchQuery) -> MatchResult {
    todo!("port modes/interactive/components/session-selector-search.ts:matchSession")
}

pub fn filter_and_sort_sessions(
    sessions: &[SessionInfo],
    query: &str,
    sort_mode: &SortMode,
    name_filter: Option<&NameFilter>,
) -> Vec<SessionInfo> {
    todo!("port modes/interactive/components/session-selector-search.ts:filterAndSortSessions")
}
