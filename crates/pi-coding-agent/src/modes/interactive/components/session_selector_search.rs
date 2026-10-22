//! Port of `packages/coding-agent/src/modes/interactive/components/session-selector-search.ts`.

use crate::core::session_manager::SessionInfo;
use pi_tui::fuzzy::fuzzy_match;
use pi_tui::utils::{js_trim, utf16_len};
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

fn normalize_whitespace_lower(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut in_ws = false;
    for c in lower.chars() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    js_trim(&out).to_string()
}

fn get_session_search_text(session: &SessionInfo) -> String {
    format!(
        "{} {} {} {}",
        session.id,
        session.name.as_deref().unwrap_or(""),
        session.all_messages_text,
        session.cwd
    )
}

pub fn has_session_name(session: &SessionInfo) -> bool {
    session.name.as_deref().is_some_and(|name| !js_trim(name).is_empty())
}

fn matches_name_filter(session: &SessionInfo, filter: NameFilter) -> bool {
    match filter {
        NameFilter::All => true,
        NameFilter::Named => has_session_name(session),
    }
}

pub fn parse_search_query(query: &str) -> ParsedSearchQuery {
    let trimmed = js_trim(query);
    if trimmed.is_empty() {
        return ParsedSearchQuery { mode: ParsedSearchQueryMode::Tokens, tokens: Vec::new(), regex: None, error: None };
    }

    // Regex mode: re:<pattern>
    if let Some(rest) = trimmed.strip_prefix("re:") {
        let pattern = js_trim(rest);
        if pattern.is_empty() {
            return ParsedSearchQuery {
                mode: ParsedSearchQueryMode::Regex,
                tokens: Vec::new(),
                regex: None,
                error: Some("Empty regex".to_string()),
            };
        }
        return match regex::RegexBuilder::new(pattern).case_insensitive(true).build() {
            Ok(re) => ParsedSearchQuery { mode: ParsedSearchQueryMode::Regex, tokens: Vec::new(), regex: Some(re), error: None },
            Err(err) => ParsedSearchQuery {
                mode: ParsedSearchQueryMode::Regex,
                tokens: Vec::new(),
                regex: None,
                error: Some(err.to_string()),
            },
        };
    }

    // Token mode with quote support.
    // Example: foo "node cve" bar
    let mut tokens: Vec<ParsedSearchQueryTokens> = Vec::new();
    let mut buf = String::new();
    let mut in_quote = false;

    let flush = |buf: &mut String, tokens: &mut Vec<ParsedSearchQueryTokens>, kind: ParsedSearchQueryTokensKind| {
        let v = js_trim(buf).to_string();
        buf.clear();
        if v.is_empty() {
            return;
        }
        tokens.push(ParsedSearchQueryTokens { kind, value: v });
    };

    for ch in trimmed.chars() {
        if ch == '"' {
            if in_quote {
                flush(&mut buf, &mut tokens, ParsedSearchQueryTokensKind::Phrase);
                in_quote = false;
            } else {
                flush(&mut buf, &mut tokens, ParsedSearchQueryTokensKind::Fuzzy);
                in_quote = true;
            }
            continue;
        }
        if !in_quote && ch.is_whitespace() {
            flush(&mut buf, &mut tokens, ParsedSearchQueryTokensKind::Fuzzy);
            continue;
        }
        buf.push(ch);
    }

    // If quotes were unbalanced, fall back to plain whitespace tokenization.
    if in_quote {
        return ParsedSearchQuery {
            mode: ParsedSearchQueryMode::Tokens,
            tokens: trimmed
                .split(char::is_whitespace)
                .map(js_trim)
                .filter(|t| !t.is_empty())
                .map(|t| ParsedSearchQueryTokens { kind: ParsedSearchQueryTokensKind::Fuzzy, value: t.to_string() })
                .collect(),
            regex: None,
            error: None,
        };
    }

    flush(&mut buf, &mut tokens, ParsedSearchQueryTokensKind::Fuzzy);
    ParsedSearchQuery { mode: ParsedSearchQueryMode::Tokens, tokens, regex: None, error: None }
}

pub fn match_session(session: &SessionInfo, parsed: &ParsedSearchQuery) -> MatchResult {
    let text = get_session_search_text(session);

    if parsed.mode == ParsedSearchQueryMode::Regex {
        let Some(re) = &parsed.regex else {
            return MatchResult { matches: false, score: 0.0 };
        };
        return match re.find(&text) {
            None => MatchResult { matches: false, score: 0.0 },
            // JS string indices are UTF-16 code units.
            Some(m) => MatchResult { matches: true, score: utf16_len(&text[..m.start()]) as f64 * 0.1 },
        };
    }

    if parsed.tokens.is_empty() {
        return MatchResult { matches: true, score: 0.0 };
    }

    let mut total_score = 0.0;
    let mut normalized_text: Option<String> = None;

    for token in &parsed.tokens {
        if token.kind == ParsedSearchQueryTokensKind::Phrase {
            let normalized = normalized_text.get_or_insert_with(|| normalize_whitespace_lower(&text));
            let phrase = normalize_whitespace_lower(&token.value);
            if phrase.is_empty() {
                continue;
            }
            let Some(idx) = normalized.find(&phrase) else {
                return MatchResult { matches: false, score: 0.0 };
            };
            total_score += utf16_len(&normalized[..idx]) as f64 * 0.1;
            continue;
        }

        let m = fuzzy_match(&token.value, &text);
        if !m.matches {
            return MatchResult { matches: false, score: 0.0 };
        }
        total_score += m.score;
    }

    MatchResult { matches: true, score: total_score }
}

pub fn filter_and_sort_sessions(
    sessions: &[SessionInfo],
    query: &str,
    sort_mode: &SortMode,
    name_filter: Option<&NameFilter>,
) -> Vec<SessionInfo> {
    let name_filter = name_filter.copied().unwrap_or(NameFilter::All);
    let name_filtered: Vec<SessionInfo> = if name_filter == NameFilter::All {
        sessions.to_vec()
    } else {
        sessions.iter().filter(|s| matches_name_filter(s, name_filter)).cloned().collect()
    };
    if js_trim(query).is_empty() {
        return name_filtered;
    }

    let parsed = parse_search_query(query);
    if parsed.error.is_some() {
        return Vec::new();
    }

    // Recent mode: filter only, keep incoming order.
    if *sort_mode == SortMode::Recent {
        return name_filtered.into_iter().filter(|s| match_session(s, &parsed).matches).collect();
    }

    // Relevance mode: sort by score, tie-break by modified desc.
    let mut scored: Vec<(SessionInfo, f64)> = name_filtered
        .into_iter()
        .filter_map(|s| {
            let res = match_session(&s, &parsed);
            res.matches.then_some((s, res.score))
        })
        .collect();
    scored.sort_by(|a, b| {
        if a.1 != b.1 {
            return a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal);
        }
        b.0.modified.cmp(&a.0.modified)
    });
    scored.into_iter().map(|(s, _)| s).collect()
}
