//! Port of `packages/coding-agent/src/core/model-resolver.ts`.

use crate::compat::AuthOperationOptions;
use crate::core::model_runtime::ModelRuntime;
use indexmap::IndexMap;
use pi_agent::types::ThinkingLevel;
use pi_ai::types::Model;
use pi_ai::types::ProviderId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

/// Default chat model IDs for providers with built-in chat models.
pub static DEFAULT_MODEL_PER_PROVIDER: LazyLock<IndexMap<ProviderId, String>> =
    LazyLock::new(|| todo!("port core/model-resolver.ts:defaultModelPerProvider"));

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReferenceTarget {
    pub provider: String,
    pub provider_models: (Model, Value),
    /// Text after the provider prefix as typed, including any `:<suffix>`.
    pub model_text: String,
    /// `modelText` without a valid thinking suffix (unless `thinkingSuffix` is `none`).
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

pub type ResolvedOrAmbiguous = ModelReferenceResult;

/// `usable` only breaks ties between providers, so a unique match resolves even without credentials
/// (`--api-key` first-time setup).
pub fn resolve_model_reference(
    request: &ModelReferenceRequest,
    models: &[Model],
    usable: Arc<dyn Fn(String) -> bool + Send + Sync>,
) -> ModelReferenceResult {
    todo!("port core/model-resolver.ts:resolveModelReference")
}

/// Exact `provider/id` or bare id lookup without thinking suffixes or fuzzy matching.
pub fn find_exact_model_reference_match(model_reference: &str, models: &[Model]) -> Option<Model> {
    todo!("port core/model-resolver.ts:findExactModelReferenceMatch")
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Resolve scope patterns (`--models`, `enabledModels`) to models with optional thinking levels.
///
/// Every model in `models` is usable, so credentials cannot choose between providers offering the same
/// id. All of them are added, with a warning so the user can pick one with `provider/id`.
pub fn resolve_model_scope_from_models(
    patterns: &[String],
    models: &[Model],
) -> ResolveModelScopeResult {
    todo!("port core/model-resolver.ts:resolveModelScopeFromModels")
}

pub async fn resolve_model_scope_with_diagnostics(
    patterns: &[String],
    model_runtime: Arc<ModelRuntime>,
    options: Option<&AuthOperationOptions>,
) -> anyhow::Result<ResolveModelScopeResult> {
    todo!("port core/model-resolver.ts:resolveModelScopeWithDiagnostics")
}

pub async fn resolve_model_scope(
    patterns: &[String],
    model_runtime: Arc<ModelRuntime>,
    options: Option<&AuthOperationOptions>,
) -> anyhow::Result<Vec<ScopedModel>> {
    todo!("port core/model-resolver.ts:resolveModelScope")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    todo!("port core/model-resolver.ts:resolveCliModel")
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

/// Find the initial model to use based on priority:
/// 1. First model from scoped models (if not continuing/resuming)
/// 2. Saved default from settings
/// 3. First available model with valid API key
pub async fn find_initial_model(
    options: &FindInitialModelOptions,
) -> anyhow::Result<InitialModelResult> {
    todo!("port core/model-resolver.ts:findInitialModel")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    todo!("port core/model-resolver.ts:restoreModelFromSession")
}
