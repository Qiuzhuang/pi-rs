//! Port of `packages/coding-agent/src/core/model-config.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

pub static MODELS_CONFIG_SCHEMA: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/model-config.ts:ModelsConfigSchema"));

/// Entry schemas rendered into docs/models-configuration.md.
pub static MODELS_JSON_SCHEMAS: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/model-config.ts:ModelsJsonSchemas"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelValueInputLimitsImagesResize {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<i64>,
    /// Maximum base64-encoded payload size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jpeg_quality: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelValueInputLimitsImages {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize: Option<ModelsJsonModelValueInputLimitsImagesResize>,
    /// Maximum images accepted in one provider message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_message: Option<i64>,
    /// Maximum images accepted across one provider request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_request: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelValueInputLimits {
    /// Maximum serialized provider request size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_request_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<ModelsJsonModelValueInputLimitsImages>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelValueCostTiers {
    /// Use this tier when total request input exceeds this token count.
    pub input_tokens_above: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelValueCost {
    /// Request-wide pricing tiers. The highest matching input threshold applies to the full request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<ModelsJsonModelValueCostTiers>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelValueSamplingParamsByThinkingLevel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimal: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medium: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xhigh: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<IndexMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelValueCompat {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelValue {
    /// Model ID sent to the API.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Overrides the provider api.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// Overrides the provider baseUrl.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    /// Provider value sent for each thinking level. null marks a level unsupported. Levels the model does not map use the provider's default value through high; xhigh and max are unsupported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level_map: Option<IndexMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limits: Option<ModelsJsonModelValueInputLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<ModelsJsonModelValueCost>,
    /// Best-effort prompt cache lifetime in seconds for the short and long retention tiers. Pi warms only tiers with a known lifetime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<IndexMap<String, f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Fields merged into every request body. Applies only to OpenAI-compatible APIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling_params: Option<IndexMap<String, Value>>,
    /// samplingParams per thinking level, merged over samplingParams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling_params_by_thinking_level:
        Option<ModelsJsonModelValueSamplingParamsByThinkingLevel>,
    /// Request headers. Values resolve like apiKey.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    /// Provider and model compatibility overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<ModelsJsonModelValueCompat>,
}

pub type ModelsJsonModel = ModelsJsonModelValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideValueInputLimitsImagesResize {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<i64>,
    /// Maximum base64-encoded payload size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jpeg_quality: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideValueInputLimitsImages {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize: Option<ModelsJsonModelOverrideValueInputLimitsImagesResize>,
    /// Maximum images accepted in one provider message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_message: Option<i64>,
    /// Maximum images accepted across one provider request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_request: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideValueInputLimits {
    /// Maximum serialized provider request size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_request_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<ModelsJsonModelOverrideValueInputLimitsImages>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideValueCostTiers {
    /// Use this tier when total request input exceeds this token count.
    pub input_tokens_above: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideValueCost {
    /// Request-wide pricing tiers. The highest matching input threshold applies to the full request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<ModelsJsonModelOverrideValueCostTiers>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideValueSamplingParamsByThinkingLevel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimal: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medium: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xhigh: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<IndexMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideValueCompat {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideValue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    /// Provider value sent for each thinking level. null marks a level unsupported. Levels the model does not map use the provider's default value through high; xhigh and max are unsupported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level_map: Option<IndexMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limits: Option<ModelsJsonModelOverrideValueInputLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<ModelsJsonModelOverrideValueCost>,
    /// Best-effort prompt cache lifetime in seconds for the short and long retention tiers. Pi warms only tiers with a known lifetime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<IndexMap<String, f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Fields merged into every request body. Applies only to OpenAI-compatible APIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling_params: Option<IndexMap<String, Value>>,
    /// samplingParams per thinking level, merged over samplingParams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling_params_by_thinking_level:
        Option<ModelsJsonModelOverrideValueSamplingParamsByThinkingLevel>,
    /// Request headers. Values resolve like apiKey.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    /// Provider and model compatibility overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<ModelsJsonModelOverrideValueCompat>,
}

pub type ModelsJsonModelOverride = ModelsJsonModelOverrideValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonProviderValueCompat {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonProviderValue {
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// API base URL. Also applies to the provider's built-in models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Literal key, "$NAME" environment interpolation, or "!command" output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// API for the provider's models, such as "openai-completions", "openai-responses", "anthropic-messages", or "google-generative-ai".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// OAuth login flow. Requires baseUrl.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<String>,
    /// Request headers. Values resolve like apiKey.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    /// Provider and model compatibility overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<ModelsJsonProviderValueCompat>,
    /// Whether to send the resolved API key as Authorization: Bearer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_header: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<ModelsJsonModel>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_overrides: Option<IndexMap<String, ModelsJsonModelOverride>>,
}

pub type ModelsJsonProvider = ModelsJsonProviderValue;

/// One immutable load of models.json.
pub struct ModelConfig {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ModelConfig {
    pub fn new(providers: IndexMap<String, ModelsJsonProvider>, error: Option<&str>) -> Self {
        todo!("port core/model-config.ts:ModelConfig.constructor")
    }

    pub async fn load(models_json_path: Option<&str>) -> anyhow::Result<Arc<ModelConfig>> {
        todo!("port core/model-config.ts:ModelConfig.load")
    }

    pub fn get_provider(&self, provider_id: &str) -> Option<ModelsJsonProvider> {
        todo!("port core/model-config.ts:ModelConfig.getProvider")
    }

    pub fn get_provider_ids(&self) -> Vec<String> {
        todo!("port core/model-config.ts:ModelConfig.getProviderIds")
    }

    pub fn get_error(&self) -> Option<String> {
        todo!("port core/model-config.ts:ModelConfig.getError")
    }
}
