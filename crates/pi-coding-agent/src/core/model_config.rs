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
pub struct ModelsJsonModelInputLimitsImagesResize {
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
pub struct ModelsJsonModelInputLimitsImages {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize: Option<ModelsJsonModelInputLimitsImagesResize>,
    /// Maximum images accepted in one provider message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_message: Option<i64>,
    /// Maximum images accepted across one provider request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_request: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelInputLimits {
    /// Maximum serialized provider request size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_request_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<ModelsJsonModelInputLimitsImages>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelCostTiers {
    /// Use this tier when total request input exceeds this token count.
    pub input_tokens_above: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelCost {
    /// Request-wide pricing tiers. The highest matching input threshold applies to the full request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<ModelsJsonModelCostTiers>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelSamplingParamsByThinkingLevel {
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
pub struct ModelsJsonModelCompat {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModel {
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
    pub input_limits: Option<ModelsJsonModelInputLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<ModelsJsonModelCost>,
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
    pub sampling_params_by_thinking_level: Option<ModelsJsonModelSamplingParamsByThinkingLevel>,
    /// Request headers. Values resolve like apiKey.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    /// Provider and model compatibility overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<ModelsJsonModelCompat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideInputLimitsImagesResize {
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
pub struct ModelsJsonModelOverrideInputLimitsImages {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize: Option<ModelsJsonModelOverrideInputLimitsImagesResize>,
    /// Maximum images accepted in one provider message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_message: Option<i64>,
    /// Maximum images accepted across one provider request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_request: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideInputLimits {
    /// Maximum serialized provider request size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_request_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<ModelsJsonModelOverrideInputLimitsImages>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideCostTiers {
    /// Use this tier when total request input exceeds this token count.
    pub input_tokens_above: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideCost {
    /// Request-wide pricing tiers. The highest matching input threshold applies to the full request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<ModelsJsonModelOverrideCostTiers>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverrideSamplingParamsByThinkingLevel {
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
pub struct ModelsJsonModelOverrideCompat {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonModelOverride {
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
    pub input_limits: Option<ModelsJsonModelOverrideInputLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<ModelsJsonModelOverrideCost>,
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
        Option<ModelsJsonModelOverrideSamplingParamsByThinkingLevel>,
    /// Request headers. Values resolve like apiKey.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    /// Provider and model compatibility overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<ModelsJsonModelOverrideCompat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonProviderCompat {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsJsonProvider {
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
    pub compat: Option<ModelsJsonProviderCompat>,
    /// Whether to send the resolved API key as Authorization: Bearer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_header: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<ModelsJsonModel>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_overrides: Option<IndexMap<String, ModelsJsonModelOverride>>,
}

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
