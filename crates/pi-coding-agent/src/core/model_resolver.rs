//! Port of `packages/coding-agent/src/core/model-resolver.ts`.
//!
//! Model resolution, scoping, and initial selection.

use crate::compat::AuthOperationOptions;
use crate::core::defaults::DEFAULT_THINKING_LEVEL;
use crate::core::model_runtime::ModelRuntime;
use indexmap::{IndexMap, IndexSet};
use pi_agent::types::ThinkingLevel;
use pi_ai::types::Model;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::Arc;
use std::sync::LazyLock;

/// Default chat model IDs for providers with built-in chat models.
pub static DEFAULT_MODEL_PER_PROVIDER: LazyLock<IndexMap<String, String>> = LazyLock::new(|| {
    [
        ("amazon-bedrock", "us.anthropic.claude-opus-4-6-v1"),
        ("ant-ling", "Ring-2.6-1T"),
        ("anthropic", "claude-opus-4-8"),
        ("openai", "gpt-5.5"),
        ("azure", "gpt-5.4"),
        ("openai-codex", "gpt-6.1-sol"),
        ("radius", "balanced"),
        ("nvidia", "nvidia/nemotron-3.5-super-vl-120b-a12b"),
        ("deepseek", "deepseek-v4-pro"),
        ("google", "gemini-3.1-pro-preview"),
        ("google-vertex", "gemini-3.1-pro-preview"),
        ("github-copilot", "gpt-6.1-sol"),
        ("openrouter", "moonshotai/kimi-k2.6"),
        ("vercel-ai-gateway", "zai/glm-5.1"),
        ("xai", "grok-4.7"),
        ("groq", "openai/gpt-oss-120b"),
        ("cerebras", "gpt-oss-120b"),
        ("zai", "glm-5.3"),
        ("zai-coding-cn", "glm-5.3"),
        ("mistral", "devstral-medium-latest"),
        ("minimax", "MiniMax-M2.7"),
        ("minimax-cn", "MiniMax-M2.7"),
        ("moonshotai", "kimi-k2.6"),
        ("moonshotai-cn", "kimi-k2.6"),
        ("huggingface", "moonshotai/Kimi-K2.6"),
        ("fireworks", "accounts/fireworks/models/kimi-k3"),
        ("together", "moonshotai/Kimi-K3"),
        ("baseten", "zai-org/GLM-5.2"),
        ("opencode", "kimi-k2.6"),
        ("opencode-go", "kimi-k3"),
        ("kimi-coding", "kimi-for-coding"),
        ("meta", "muse-spark-1.3"),
        ("cloudflare-workers-ai", "@cf/moonshotai/kimi-k2.6"),
        ("cloudflare-ai-gateway", "workers-ai/@cf/moonshotai/kimi-k2.6"),
        ("qwen-token-plan", "qwen3.7-max"),
        ("qwen-token-plan-cn", "qwen3.7-max"),
        ("qwen-token-plan-individual", "qwen3.8-max"),
        ("xiaomi", "mimo-v2.6-pro"),
        ("xiaomi-token-plan-cn", "mimo-v2.6-pro"),
        ("xiaomi-token-plan-ams", "mimo-v2.6-pro"),
        ("xiaomi-token-plan-sgp", "mimo-v2.6-pro"),
    ]
    .into_iter()
    .map(|(provider, model)| (provider.to_string(), model.to_string()))
    .collect()
});

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopedModel {
    pub model: Model,
    /// Thinking level if explicitly specified in pattern (e.g., "model:high"), undefined otherwise
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelReferenceRequestThinkingSuffix {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "strict")]
    Strict,
    #[serde(rename = "lenient")]
    Lenient,
}

impl ModelReferenceRequestThinkingSuffix {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Strict => "strict",
            Self::Lenient => "lenient",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReferenceRequest {
    /// Bare model id or `provider/id`, optionally followed by `:<thinking>`.
    pub text: String,
    /// Only consider models from this provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// A trailing `:<suffix>` is only considered when the full text matches no model, since ids such as
    /// OpenRouter's `qwen/qwen3-coder:exacto` contain colons.
    /// - `none`: the suffix stays part of the id
    /// - `strict`: a valid thinking level is parsed; any other suffix stays part of the id
    /// - `lenient`: like `strict`, but an invalid suffix is dropped with a warning
    pub thinking_suffix: ModelReferenceRequestThinkingSuffix,
    /// Match id and name substrings when no exact match exists.
    pub fuzzy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all_fields = "camelCase")]
pub enum ModelReferenceResult {
    #[serde(rename = "resolved")]
    Resolved {
        model: Model,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thinking_level: Option<ThinkingLevel>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        warning: Option<String>,
    },
    #[serde(rename = "ambiguous")]
    Ambiguous {
        candidates: Vec<Model>,
        usable_count: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thinking_level: Option<ThinkingLevel>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        warning: Option<String>,
    },
    #[serde(rename = "not-found")]
    NotFound {
        /// Set when a provider is explicit or named by a `provider/` prefix, for building a custom model id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<ModelReferenceTarget>,
    },
}

impl ModelReferenceResult {
    fn not_found() -> Self {
        Self::NotFound { target: None }
    }

    fn resolved(model: Model) -> Self {
        Self::Resolved { model, thinking_level: None, warning: None }
    }

    fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }

    fn warning(&self) -> Option<&str> {
        match self {
            Self::Resolved { warning, .. } | Self::Ambiguous { warning, .. } => warning.as_deref(),
            Self::NotFound { .. } => None,
        }
    }

    fn with_thinking(mut self, level: Option<ThinkingLevel>, new_warning: Option<Option<String>>) -> Self {
        match &mut self {
            Self::Resolved { thinking_level, warning, .. } | Self::Ambiguous { thinking_level, warning, .. } => {
                *thinking_level = level;
                if let Some(new_warning) = new_warning {
                    *warning = new_warning;
                }
            }
            Self::NotFound { .. } => {}
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReferenceTarget {
    pub provider: String,
    /// The provider's models, never empty; the first one anchors the provider.
    pub provider_models: Vec<Model>,
    /// Text after the provider prefix as typed, including any `:<suffix>`.
    pub model_text: String,
    /// `modelText` without a valid thinking suffix (unless `thinkingSuffix` is `none`).
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

pub type ResolvedOrAmbiguous = ModelReferenceResult;

/// JS `localeCompare` approximation (ICU root collation for ASCII: punctuation < digits <
/// letters, case-insensitive first, then lowercase before uppercase).
fn locale_compare(a: &str, b: &str) -> Ordering {
    const ORDER: &str = "\t\n\u{b}\u{c}\r _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    fn weight(c: char) -> u32 {
        if let Some(index) = ORDER.find(c) {
            return index as u32;
        }
        if c.is_ascii_digit() {
            return 100 + (c as u32 - '0' as u32);
        }
        if c.is_ascii_alphabetic() {
            return 200 + (c.to_ascii_lowercase() as u32 - 'a' as u32);
        }
        1000 + c.to_lowercase().next().unwrap_or(c) as u32
    }
    let primary = |s: &str| s.chars().map(weight).collect::<Vec<_>>();
    primary(a)
        .cmp(&primary(b))
        .then_with(|| {
            let case = |s: &str| s.chars().map(|c| u8::from(c.is_uppercase())).collect::<Vec<_>>();
            case(a).cmp(&case(b))
        })
        .then_with(|| a.cmp(b))
}

fn parse_thinking_level(value: &str) -> Option<ThinkingLevel> {
    ThinkingLevel::parse(value).filter(|level| level.as_str() == value)
}

struct ThinkingSplit<'a> {
    base: &'a str,
    thinking_level: ThinkingLevel,
}

fn split_thinking_suffix(text: &str) -> Option<ThinkingSplit<'_>> {
    let colon_index = text.rfind(':')?;
    let thinking_level = parse_thinking_level(&text[colon_index + 1..])?;
    Some(ThinkingSplit { base: &text[..colon_index], thinking_level })
}

struct ProviderSplit {
    provider: String,
    model_text: String,
}

fn split_provider_prefix(text: &str, models: &[&Model]) -> Option<ProviderSplit> {
    let slash_index = text.find('/')?;
    let prefix = text[..slash_index].trim().to_lowercase();
    let model_text = text[slash_index + 1..].trim();
    if prefix.is_empty() || model_text.is_empty() {
        return None;
    }
    let provider = models
        .iter()
        .find(|model| model.provider.to_lowercase() == prefix)
        .map(|model| model.provider.clone())?;
    Some(ProviderSplit { provider, model_text: model_text.to_string() })
}

/// `usable` only breaks ties between providers, so a unique match resolves even without credentials
/// (`--api-key` first-time setup).
pub fn resolve_model_reference(
    request: &ModelReferenceRequest,
    models: &[Model],
    usable: &dyn Fn(&str) -> bool,
) -> ModelReferenceResult {
    let text = request.text.trim();
    if text.is_empty() {
        return ModelReferenceResult::not_found();
    }
    let constraint = request.provider.as_ref().map(|p| p.to_lowercase());
    let pool: Vec<&Model> = match &constraint {
        Some(constraint) => models.iter().filter(|m| m.provider.to_lowercase() == *constraint).collect(),
        None => models.iter().collect(),
    };
    let result = resolve_text(text, request, &pool, usable);
    if !result.is_not_found() {
        return result;
    }

    let split = split_provider_prefix(text, &pool);
    let anchor = if constraint.is_some() {
        pool.first().copied()
    } else {
        split
            .as_ref()
            .and_then(|split| pool.iter().find(|m| m.provider == split.provider).copied())
    };
    let Some(anchor) = anchor else { return result };
    let model_text = split.map(|s| s.model_text).unwrap_or_else(|| text.to_string());
    let suffix = if request.thinking_suffix == ModelReferenceRequestThinkingSuffix::None {
        None
    } else {
        split_thinking_suffix(&model_text)
    };
    let mut provider_models = vec![anchor.clone()];
    provider_models.extend(
        pool.iter()
            .filter(|m| m.provider == anchor.provider && !std::ptr::eq(**m, anchor))
            .map(|m| (*m).clone()),
    );
    ModelReferenceResult::NotFound {
        target: Some(ModelReferenceTarget {
            provider: anchor.provider.clone(),
            provider_models,
            model_id: suffix.as_ref().map(|s| s.base.to_string()).unwrap_or_else(|| model_text.clone()),
            thinking_level: suffix.map(|s| s.thinking_level),
            model_text,
        }),
    }
}

/// Exact `provider/id` or bare id lookup without thinking suffixes or fuzzy matching.
pub fn find_exact_model_reference_match(model_reference: &str, models: &[Model]) -> Option<Model> {
    let result = resolve_model_reference(
        &ModelReferenceRequest {
            text: model_reference.to_string(),
            provider: None,
            thinking_suffix: ModelReferenceRequestThinkingSuffix::None,
            fuzzy: false,
        },
        models,
        &|_| true,
    );
    match result {
        ModelReferenceResult::Resolved { model, .. } => Some(model),
        _ => None,
    }
}

fn resolve_text(
    text: &str,
    request: &ModelReferenceRequest,
    pool: &[&Model],
    usable: &dyn Fn(&str) -> bool,
) -> ModelReferenceResult {
    if let Some(reference) = select_reference(text, pool, request.fuzzy, usable) {
        return reference;
    }

    if request.fuzzy
        && let Some(fuzzy) = select_fuzzy(fuzzy_matches(text, pool), usable)
    {
        return fuzzy;
    }

    if request.thinking_suffix == ModelReferenceRequestThinkingSuffix::None {
        return ModelReferenceResult::not_found();
    }
    if let Some(thinking) = split_thinking_suffix(text) {
        let inner = resolve_text(thinking.base, request, pool, usable);
        if inner.is_not_found() {
            return inner;
        }
        // Only use this thinking level if no warning from inner recursion
        let level = if inner.warning().is_some() { None } else { Some(thinking.thinking_level) };
        return inner.with_thinking(level, None);
    }
    // Keep an invalid suffix as part of the id and fail, rather than resolve a different model
    if request.thinking_suffix == ModelReferenceRequestThinkingSuffix::Strict {
        return ModelReferenceResult::not_found();
    }
    let Some(colon_index) = text.rfind(':') else {
        return ModelReferenceResult::not_found();
    };

    let suffix = &text[colon_index + 1..];
    let inner = resolve_text(&text[..colon_index], request, pool, usable);
    if inner.is_not_found() {
        return inner;
    }
    inner.with_thinking(
        None,
        Some(Some(format!(
            "Invalid thinking level \"{suffix}\" in pattern \"{text}\". Using default instead."
        ))),
    )
}

fn distinct_providers<'a>(models: impl IntoIterator<Item = &'a Model>) -> usize {
    models.into_iter().map(|m| m.provider.as_str()).collect::<IndexSet<_>>().len()
}

/// `provider/rest` is only a hint: the whole text may also be a model id on another provider, such as
/// gateway id `zai/glm-5` or commandcode id `xiaomi/mimo-v2.6-pro`. The named provider wins when usable,
/// since that is what the syntax usually means; otherwise a sole usable raw match wins, since the named
/// provider could not serve the request. An unusable named match is still preferred over unusable raw
/// matches, so `--api-key` can set up the named provider.
fn select_reference(
    text: &str,
    pool: &[&Model],
    fuzzy: bool,
    usable: &dyn Fn(&str) -> bool,
) -> Option<ResolvedOrAmbiguous> {
    let lower_text = text.to_lowercase();
    let raw: Vec<&Model> = pool.iter().copied().filter(|m| m.id.to_lowercase() == lower_text).collect();
    let split = split_provider_prefix(text, pool);
    let mut named: Option<&Model> = None;
    if let Some(split) = &split {
        let provider_models: Vec<&Model> = pool.iter().copied().filter(|m| m.provider == split.provider).collect();
        let lower_model_text = split.model_text.to_lowercase();
        // The provider's own literal id (OpenRouter's `openrouter/free`) beats its fuzzy matches
        named = provider_models
            .iter()
            .copied()
            .find(|m| m.id.to_lowercase() == lower_model_text)
            .or_else(|| raw.iter().copied().find(|m| m.provider == split.provider))
            .or_else(|| {
                if fuzzy {
                    best_fuzzy_match(&fuzzy_matches(&split.model_text, &provider_models))
                } else {
                    None
                }
            });
    }

    let others: Vec<&Model> = raw.iter().copied().filter(|m| !named.is_some_and(|n| std::ptr::eq(*m, n))).collect();
    let candidates: Vec<&Model> = named.into_iter().chain(others.iter().copied()).collect();
    if candidates.is_empty() {
        return None;
    }
    if distinct_providers(candidates.iter().copied()) == 1 {
        return Some(ModelReferenceResult::resolved(candidates[0].clone()));
    }
    if let Some(named) = named
        && usable(&named.provider)
    {
        return Some(ModelReferenceResult::resolved(named.clone()));
    }
    let usable_others: Vec<&Model> = others.iter().copied().filter(|m| usable(&m.provider)).collect();
    if usable_others.len() == 1 {
        return Some(ModelReferenceResult::resolved(usable_others[0].clone()));
    }
    if usable_others.is_empty()
        && let Some(named) = named
    {
        return Some(ModelReferenceResult::resolved(named.clone()));
    }
    Some(ModelReferenceResult::Ambiguous {
        candidates: candidates.into_iter().cloned().collect(),
        usable_count: usable_others.len() as i64,
        thinking_level: None,
        warning: None,
    })
}

fn fuzzy_matches<'a>(text: &str, models: &[&'a Model]) -> Vec<&'a Model> {
    let lower_text = text.to_lowercase();
    models
        .iter()
        .copied()
        .filter(|m| m.id.to_lowercase().contains(&lower_text) || m.name.to_lowercase().contains(&lower_text))
        .collect()
}

fn select_fuzzy(matches: Vec<&Model>, usable: &dyn Fn(&str) -> bool) -> Option<ResolvedOrAmbiguous> {
    if matches.is_empty() {
        return None;
    }
    let multiple_providers = distinct_providers(matches.iter().copied()) > 1;
    let usable_matches: Vec<&Model> = if multiple_providers {
        matches.iter().copied().filter(|m| usable(&m.provider)).collect()
    } else {
        Vec::new()
    };
    let pool = if usable_matches.is_empty() { &matches } else { &usable_matches };
    let best = best_fuzzy_match(pool)?;
    // The same id on several providers would otherwise be decided by catalog order
    let tied: Vec<&Model> = pool.iter().copied().filter(|m| m.id == best.id).collect();
    if distinct_providers(tied.iter().copied()) == 1 {
        return Some(ModelReferenceResult::resolved(best.clone()));
    }
    Some(ModelReferenceResult::Ambiguous {
        usable_count: if usable_matches.is_empty() { 0 } else { tied.len() as i64 },
        candidates: tied.into_iter().cloned().collect(),
        thinking_level: None,
        warning: None,
    })
}

/// Prefer aliases (`claude-sonnet-4-5`) over dated versions (`claude-sonnet-4-5-20250929`), then the latest.
fn best_fuzzy_match<'a>(matches: &[&'a Model]) -> Option<&'a Model> {
    let aliases: Vec<&Model> = matches.iter().copied().filter(|m| is_alias(&m.id)).collect();
    let mut sorted: Vec<&Model> = if aliases.is_empty() { matches.to_vec() } else { aliases };
    // Stable sort, descending by id.
    sorted.sort_by(|a, b| locale_compare(&b.id, &a.id));
    sorted.first().copied()
}

fn is_alias(id: &str) -> bool {
    if id.ends_with("-latest") {
        return true;
    }
    // `/-\d{8}$/`
    let bytes = id.as_bytes();
    let dated = bytes.len() >= 9
        && bytes[bytes.len() - 9] == b'-'
        && bytes[bytes.len() - 8..].iter().all(u8::is_ascii_digit);
    !dated
}

fn build_fallback_model(provider_models: &[Model], model_id: &str) -> Model {
    let first = &provider_models[0];
    let base_model = match DEFAULT_MODEL_PER_PROVIDER.get(&first.provider) {
        Some(default_id) => provider_models.iter().find(|m| m.id == *default_id).unwrap_or(first),
        None => first,
    };
    Model { id: model_id.to_string(), name: model_id.to_string(), ..base_model.clone() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelScopeDiagnosticCode {
    #[serde(rename = "no-match")]
    NoMatch,
    #[serde(rename = "invalid-thinking-level")]
    InvalidThinkingLevel,
    #[serde(rename = "ambiguous")]
    Ambiguous,
}

impl ModelScopeDiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoMatch => "no-match",
            Self::InvalidThinkingLevel => "invalid-thinking-level",
            Self::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelScopeDiagnostic {
    /// Always "warning".
    pub r#type: String,
    pub code: ModelScopeDiagnosticCode,
    pub message: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveModelScopeResult {
    pub scoped_models: Vec<ScopedModel>,
    pub diagnostics: Vec<ModelScopeDiagnostic>,
}

// ---------------------------------------------------------------------------
// minimatch (subset used for scope patterns: `*`, `?`, `[...]`, `**`, `{a,b}`, nocase)
// ---------------------------------------------------------------------------

fn expand_braces(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut depth = 0;
    let mut start = None;
    for (i, c) in chars.iter().enumerate() {
        match c {
            '\\' => continue,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let open = start.expect("brace start");
                    let inner: String = chars[open + 1..i].iter().collect();
                    let mut parts = Vec::new();
                    let mut level = 0;
                    let mut current = String::new();
                    for ch in inner.chars() {
                        match ch {
                            '{' => {
                                level += 1;
                                current.push(ch);
                            }
                            '}' => {
                                level -= 1;
                                current.push(ch);
                            }
                            ',' if level == 0 => parts.push(std::mem::take(&mut current)),
                            _ => current.push(ch),
                        }
                    }
                    parts.push(current);
                    if parts.len() < 2 {
                        // `{a}` is literal in brace expansion.
                        continue;
                    }
                    let prefix: String = chars[..open].iter().collect();
                    let suffix: String = chars[i + 1..].iter().collect();
                    return parts
                        .into_iter()
                        .flat_map(|part| expand_braces(&format!("{prefix}{part}{suffix}")))
                        .collect();
                }
            }
            _ => {}
        }
    }
    vec![pattern.to_string()]
}

fn segment_regex(segment: &str) -> Option<regex::Regex> {
    let chars: Vec<char> = segment.chars().collect();
    let mut out = String::from("(?i)^");
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' => {
                while i + 1 < chars.len() && chars[i + 1] == '*' {
                    i += 1;
                }
                out.push_str("[^/]*");
            }
            '?' => out.push_str("[^/]"),
            '[' => {
                // Find the closing bracket; an unclosed bracket is literal.
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '!' || chars[j] == '^') {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ']' {
                    j += 1;
                }
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                if j >= chars.len() {
                    out.push_str("\\[");
                } else {
                    let mut class = String::from("[");
                    let mut k = i + 1;
                    if chars[k] == '!' || chars[k] == '^' {
                        class.push('^');
                        k += 1;
                    }
                    while k < j {
                        let ch = chars[k];
                        if ch == '\\' || ch == '[' || (ch == ']' && k != i + 1) {
                            class.push('\\');
                        }
                        class.push(ch);
                        k += 1;
                    }
                    class.push(']');
                    out.push_str(&class);
                    i = j;
                }
            }
            '\\' if i + 1 < chars.len() => {
                i += 1;
                out.push_str(&regex::escape(&chars[i].to_string()));
            }
            _ => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    out.push('$');
    regex::Regex::new(&out).ok()
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    let Some((first, rest)) = pattern.split_first() else {
        return path.is_empty();
    };
    if *first == "**" {
        // Globstar: zero or more segments, none of them dot segments.
        for skip in 0..=path.len() {
            if path[..skip].iter().any(|s| s.starts_with('.')) {
                break;
            }
            if match_segments(rest, &path[skip..]) {
                return true;
            }
        }
        return false;
    }
    let Some((segment, path_rest)) = path.split_first() else {
        return false;
    };
    // Dot segments only match patterns that start with a dot.
    if segment.starts_with('.') && !first.starts_with('.') && first.chars().any(|c| matches!(c, '*' | '?' | '[')) {
        return false;
    }
    match segment_regex(first) {
        Some(re) if re.is_match(segment) => match_segments(rest, path_rest),
        _ => false,
    }
}

/// `minimatch(path, pattern, { nocase: true })`.
fn minimatch(path: &str, pattern: &str) -> bool {
    expand_braces(pattern).iter().any(|pattern| {
        let pattern_segments: Vec<&str> = pattern.split('/').collect();
        let path_segments: Vec<&str> = path.split('/').collect();
        match_segments(&pattern_segments, &path_segments)
    })
}

fn models_are_equal(a: &Model, b: &Model) -> bool {
    a.id == b.id && a.provider == b.provider
}

/// Resolve scope patterns (`--models`, `enabledModels`) to models with optional thinking levels.
///
/// Every model in `models` is usable, so credentials cannot choose between providers offering the same
/// id. All of them are added, with a warning so the user can pick one with `provider/id`.
pub fn resolve_model_scope_from_models(patterns: &[String], models: &[Model]) -> ResolveModelScopeResult {
    let mut scoped_models: Vec<ScopedModel> = Vec::new();
    let mut diagnostics: Vec<ModelScopeDiagnostic> = Vec::new();
    let add = |scoped: &mut Vec<ScopedModel>, model: &Model, thinking_level: Option<ThinkingLevel>| {
        if !scoped.iter().any(|s| models_are_equal(&s.model, model)) {
            scoped.push(ScopedModel { model: model.clone(), thinking_level });
        }
    };
    let no_match = |diagnostics: &mut Vec<ModelScopeDiagnostic>, pattern: &str| {
        diagnostics.push(ModelScopeDiagnostic {
            r#type: "warning".to_string(),
            code: ModelScopeDiagnosticCode::NoMatch,
            message: format!("No models match pattern \"{pattern}\""),
            pattern: pattern.to_string(),
        });
    };
    let ambiguous = |diagnostics: &mut Vec<ModelScopeDiagnostic>, pattern: &str, candidates: &[Model]| {
        let mut providers: Vec<String> =
            candidates.iter().map(|m| m.provider.clone()).collect::<IndexSet<_>>().into_iter().collect();
        providers.sort_by(|a, b| locale_compare(a, b));
        diagnostics.push(ModelScopeDiagnostic {
            r#type: "warning".to_string(),
            code: ModelScopeDiagnosticCode::Ambiguous,
            message: format!(
                "Pattern \"{pattern}\" matches several providers: {}. Use provider/id to choose one.",
                providers.join(", ")
            ),
            pattern: pattern.to_string(),
        });
    };

    for pattern in patterns {
        // Check if pattern contains glob characters
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            // Extract optional thinking level suffix (e.g., "provider/*:high")
            let thinking = split_thinking_suffix(pattern);
            let glob_pattern = thinking.as_ref().map(|t| t.base).unwrap_or(pattern);
            let thinking_level = thinking.as_ref().map(|t| t.thinking_level);

            // Ids such as `bracketed-model[1m]` are not globs
            let exact = resolve_model_reference(
                &ModelReferenceRequest {
                    text: glob_pattern.to_string(),
                    provider: None,
                    thinking_suffix: ModelReferenceRequestThinkingSuffix::None,
                    fuzzy: false,
                },
                models,
                &|_| true,
            );
            let matches: Vec<Model> = match exact {
                ModelReferenceResult::Resolved { model, .. } => vec![model],
                ModelReferenceResult::Ambiguous { candidates, .. } => {
                    ambiguous(&mut diagnostics, pattern, &candidates);
                    candidates
                }
                ModelReferenceResult::NotFound { .. } => {
                    // Match against "provider/modelId" format OR just model ID
                    // This allows "*sonnet*" to match without requiring "anthropic/*sonnet*"
                    models
                        .iter()
                        .filter(|m| {
                            minimatch(&format!("{}/{}", m.provider, m.id), glob_pattern) || minimatch(&m.id, glob_pattern)
                        })
                        .cloned()
                        .collect()
                }
            };
            if matches.is_empty() {
                no_match(&mut diagnostics, pattern);
            }
            for model in &matches {
                add(&mut scoped_models, model, thinking_level);
            }
            continue;
        }

        let result = resolve_model_reference(
            &ModelReferenceRequest {
                text: pattern.clone(),
                provider: None,
                thinking_suffix: ModelReferenceRequestThinkingSuffix::Lenient,
                fuzzy: true,
            },
            models,
            &|_| true,
        );
        match result {
            ModelReferenceResult::NotFound { .. } => {
                no_match(&mut diagnostics, pattern);
            }
            ModelReferenceResult::Resolved { model, thinking_level, warning } => {
                if let Some(warning) = warning {
                    diagnostics.push(ModelScopeDiagnostic {
                        r#type: "warning".to_string(),
                        code: ModelScopeDiagnosticCode::InvalidThinkingLevel,
                        message: warning,
                        pattern: pattern.clone(),
                    });
                }
                add(&mut scoped_models, &model, thinking_level);
            }
            ModelReferenceResult::Ambiguous { candidates, thinking_level, warning, .. } => {
                if let Some(warning) = warning {
                    diagnostics.push(ModelScopeDiagnostic {
                        r#type: "warning".to_string(),
                        code: ModelScopeDiagnosticCode::InvalidThinkingLevel,
                        message: warning,
                        pattern: pattern.clone(),
                    });
                }
                ambiguous(&mut diagnostics, pattern, &candidates);
                for model in &candidates {
                    add(&mut scoped_models, model, thinking_level);
                }
            }
        }
    }

    ResolveModelScopeResult { scoped_models, diagnostics }
}

pub async fn resolve_model_scope_with_diagnostics(
    patterns: &[String],
    model_runtime: Arc<ModelRuntime>,
    options: Option<&AuthOperationOptions>,
) -> anyhow::Result<ResolveModelScopeResult> {
    let available = model_runtime.get_available(None, options).await?;
    Ok(resolve_model_scope_from_models(patterns, &available))
}

/// Yellow terminal text (`chalk.yellow`).
fn yellow(text: &str) -> String {
    format!("\x1b[33m{text}\x1b[39m")
}

/// Dim terminal text (`chalk.dim`).
fn dim(text: &str) -> String {
    format!("\x1b[2m{text}\x1b[22m")
}

pub async fn resolve_model_scope(
    patterns: &[String],
    model_runtime: Arc<ModelRuntime>,
    options: Option<&AuthOperationOptions>,
) -> anyhow::Result<Vec<ScopedModel>> {
    let ResolveModelScopeResult { scoped_models, diagnostics } =
        resolve_model_scope_with_diagnostics(patterns, model_runtime, options).await?;
    for diagnostic in diagnostics {
        eprintln!("{}", yellow(&format!("Warning: {}", diagnostic.message)));
    }
    Ok(scoped_models)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCliModelResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Error message suitable for CLI display.
    /// When set, model will be undefined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ResolveCliModelResult {
    fn error(message: String) -> Self {
        Self { error: Some(message), ..Default::default() }
    }
}

#[derive(Clone)]
pub struct ResolveCliModelOptions {
    pub cli_provider: Option<String>,
    pub cli_model: Option<String>,
    pub cli_thinking: Option<ThinkingLevel>,
    /// `--api-key` was supplied for the selected provider.
    pub inline_api_key: Option<bool>,
    pub model_runtime: Arc<ModelRuntime>,
}

/// Resolve a single model from CLI flags.
///
/// Supports:
/// - --provider <provider> --model <pattern>
/// - --model <provider>/<pattern>
/// - Fuzzy matching (same rules as model scoping: exact id, then partial id/name)
///
/// Note: This does not apply the thinking level by itself, but it may *parse* and
/// return a thinking level from "<pattern>:<thinking>" so the caller can apply it.
pub fn resolve_cli_model(options: &ResolveCliModelOptions) -> ResolveCliModelResult {
    let runtime = options.model_runtime.clone();
    resolve_cli_model_from(
        options.cli_provider.as_deref(),
        options.cli_model.as_deref(),
        options.cli_thinking,
        options.inline_api_key == Some(true),
        &runtime.get_models(None),
        &|provider| runtime.has_configured_auth(provider),
    )
}

/// [`resolve_cli_model`] over an explicit model list and auth check.
pub fn resolve_cli_model_from(
    cli_provider: Option<&str>,
    cli_model: Option<&str>,
    cli_thinking: Option<ThinkingLevel>,
    inline_api_key: bool,
    models: &[Model],
    has_configured_auth: &dyn Fn(&str) -> bool,
) -> ResolveCliModelResult {
    let Some(cli_model) = cli_model.filter(|m| !m.is_empty()) else {
        return ResolveCliModelResult::default();
    };

    // Important: use *all* models here, not just models with pre-configured auth.
    // This allows "--api-key" to be used for first-time setup.
    if models.is_empty() {
        return ResolveCliModelResult::error(
            "No models available. Check your installation or add models to models.json.".to_string(),
        );
    }

    let cli_provider = cli_provider.filter(|p| !p.is_empty());
    let provider = cli_provider.and_then(|cli_provider| {
        let lower = cli_provider.to_lowercase();
        models.iter().find(|m| m.provider.to_lowercase() == lower).map(|m| m.provider.clone())
    });
    if let Some(cli_provider) = cli_provider
        && provider.is_none()
    {
        return ResolveCliModelResult::error(format!(
            "Unknown provider \"{cli_provider}\". Use --list-models to see available providers/models."
        ));
    }

    let never: &dyn Fn(&str) -> bool = &|_| false;
    let result = resolve_model_reference(
        &ModelReferenceRequest {
            text: cli_model.to_string(),
            provider,
            thinking_suffix: ModelReferenceRequestThinkingSuffix::Strict,
            fuzzy: true,
        },
        models,
        // An inline key goes to whichever provider is selected, so stored credentials must not choose it
        if inline_api_key { never } else { has_configured_auth },
    );
    let target = match result {
        ModelReferenceResult::Resolved { model, thinking_level, warning } => {
            return ResolveCliModelResult { model: Some(model), thinking_level, warning, error: None };
        }
        ModelReferenceResult::Ambiguous { candidates, usable_count, .. } => {
            let mut matches: Vec<String> = candidates.iter().map(|m| format!("{}/{}", m.provider, m.id)).collect();
            matches.sort_by(|a, b| locale_compare(a, b));
            let auth_hint = if inline_api_key {
                "--api-key does not choose a provider."
            } else if usable_count == 0 {
                "No matching provider is authenticated."
            } else {
                "More than one matching provider is authenticated."
            };
            return ResolveCliModelResult::error(format!(
                "Model \"{cli_model}\" is ambiguous across providers: {}. {auth_hint} Use --provider or provider/model.",
                matches.join(", ")
            ));
        }
        ModelReferenceResult::NotFound { target } => target,
    };

    let Some(target) = target else {
        return ResolveCliModelResult::error(format!(
            "Model \"{cli_model}\" not found. Use --list-models to see available models."
        ));
    };

    // Use the thinking level suffix only when --thinking is not explicitly provided; otherwise it
    // stays part of the custom model id.
    // e.g. "zai-org/GLM-5.1-FP8:high" → modelId="zai-org/GLM-5.1-FP8", fallbackThinking="high"
    let model_id = if cli_thinking.is_some() { target.model_text.clone() } else { target.model_id.clone() };
    let fallback_thinking = if cli_thinking.is_some() { None } else { target.thinking_level };
    let mut fallback_model = build_fallback_model(&target.provider_models, &model_id);
    let requested_thinking = cli_thinking.or(fallback_thinking);
    if requested_thinking.is_some_and(|level| level != ThinkingLevel::Off) {
        fallback_model.reasoning = true;
    }
    ResolveCliModelResult {
        model: Some(fallback_model),
        thinking_level: fallback_thinking,
        warning: Some(format!(
            "Model \"{model_id}\" not found for provider \"{}\". Using custom model id.",
            target.provider
        )),
        error: None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialModelResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    pub thinking_level: ThinkingLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

#[derive(Clone)]
pub struct FindInitialModelOptions {
    pub scoped_models: Vec<ScopedModel>,
    pub is_continuing: bool,
    pub default_provider: Option<String>,
    pub default_model_id: Option<String>,
    pub default_thinking_level: Option<ThinkingLevel>,
    pub model_thinking_levels: Option<IndexMap<String, ThinkingLevel>>,
    pub model_runtime: Arc<ModelRuntime>,
}

fn find_default_available(available: &[Model]) -> Option<Model> {
    // Try to find a default model from known providers
    DEFAULT_MODEL_PER_PROVIDER.iter().find_map(|(provider, default_id)| {
        available.iter().find(|m| m.provider == *provider && m.id == *default_id).cloned()
    })
}

/// Find the initial model to use based on priority:
/// 1. First model from scoped models (if not continuing/resuming)
/// 2. Saved default from settings
/// 3. First available model with valid API key
pub async fn find_initial_model(options: &FindInitialModelOptions) -> anyhow::Result<InitialModelResult> {
    let runtime = &options.model_runtime;

    // 1. Use first model from scoped models (skip if continuing/resuming)
    if let Some(scoped) = options.scoped_models.first()
        && !options.is_continuing
    {
        let per_model = options
            .model_thinking_levels
            .as_ref()
            .and_then(|levels| levels.get(&format!("{}/{}", scoped.model.provider, scoped.model.id)))
            .copied();
        return Ok(InitialModelResult {
            model: Some(scoped.model.clone()),
            thinking_level: scoped
                .thinking_level
                .or(per_model)
                .or(options.default_thinking_level)
                .unwrap_or(DEFAULT_THINKING_LEVEL),
            fallback_message: None,
        });
    }

    // 2. Try saved default from settings if auth is configured.
    if let (Some(default_provider), Some(default_model_id)) = (&options.default_provider, &options.default_model_id)
        && !default_provider.is_empty()
        && !default_model_id.is_empty()
        && let Some(found) = runtime.get_model(default_provider, default_model_id)
        && runtime.has_configured_auth(&found.provider)
    {
        let per_model = options
            .model_thinking_levels
            .as_ref()
            .and_then(|levels| levels.get(&format!("{default_provider}/{default_model_id}")))
            .copied();
        let thinking_level = per_model.or(options.default_thinking_level).unwrap_or(DEFAULT_THINKING_LEVEL);
        return Ok(InitialModelResult { model: Some(found), thinking_level, fallback_message: None });
    }

    // 3. Try first available model with valid API key
    let available = runtime.get_available_snapshot();
    if !available.is_empty() {
        let model = find_default_available(&available).unwrap_or_else(|| available[0].clone());
        return Ok(InitialModelResult {
            model: Some(model),
            thinking_level: DEFAULT_THINKING_LEVEL,
            fallback_message: None,
        });
    }

    // 4. No model found
    Ok(InitialModelResult { model: None, thinking_level: DEFAULT_THINKING_LEVEL, fallback_message: None })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreModelFromSessionResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

/// Restore model from session, with fallback to available models
pub async fn restore_model_from_session(
    saved_provider: &str,
    saved_model_id: &str,
    current_model: Option<Model>,
    should_print_messages: bool,
    model_runtime: Arc<ModelRuntime>,
) -> anyhow::Result<RestoreModelFromSessionResult> {
    let restored_model = model_runtime.get_model(saved_provider, saved_model_id);

    // Check if restored model exists and still has auth configured
    let has_configured_auth = restored_model
        .as_ref()
        .is_some_and(|model| model_runtime.has_configured_auth(&model.provider));

    if let Some(restored) = restored_model.clone()
        && has_configured_auth
    {
        if should_print_messages {
            println!("{}", dim(&format!("Restored model: {saved_provider}/{saved_model_id}")));
        }
        return Ok(RestoreModelFromSessionResult { model: Some(restored), fallback_message: None });
    }

    // Model not found or no API key - fall back
    let reason = if restored_model.is_none() { "model no longer exists" } else { "no auth configured" };

    if should_print_messages {
        eprintln!(
            "{}",
            yellow(&format!(
                "Warning: Could not restore model {saved_provider}/{saved_model_id} ({reason})."
            ))
        );
    }

    // If we already have a model, use it as fallback
    if let Some(current) = current_model {
        if should_print_messages {
            println!("{}", dim(&format!("Falling back to: {}/{}", current.provider, current.id)));
        }
        let fallback_message = format!(
            "Could not restore model {saved_provider}/{saved_model_id} ({reason}). Using {}/{}.",
            current.provider, current.id
        );
        return Ok(RestoreModelFromSessionResult { model: Some(current), fallback_message: Some(fallback_message) });
    }

    // Try to find any available model
    let available = model_runtime.get_available_snapshot();
    if !available.is_empty() {
        let fallback = find_default_available(&available).unwrap_or_else(|| available[0].clone());
        if should_print_messages {
            println!("{}", dim(&format!("Falling back to: {}/{}", fallback.provider, fallback.id)));
        }
        let fallback_message = format!(
            "Could not restore model {saved_provider}/{saved_model_id} ({reason}). Using {}/{}.",
            fallback.provider, fallback.id
        );
        return Ok(RestoreModelFromSessionResult { model: Some(fallback), fallback_message: Some(fallback_message) });
    }

    // No models available
    Ok(RestoreModelFromSessionResult::default())
}
