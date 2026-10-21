//! Model metadata schema types (port of `providers/model-schema.ts`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::serde_util::{js_f64, opt_js_f64};

/// Pi thinking levels (excluding `off`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

pub const THINKING_LEVELS: [ThinkingLevel; 6] = [
    ThinkingLevel::Minimal,
    ThinkingLevel::Low,
    ThinkingLevel::Medium,
    ThinkingLevel::High,
    ThinkingLevel::Xhigh,
    ThinkingLevel::Max,
];

impl ThinkingLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            ThinkingLevel::Minimal => "minimal",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::Xhigh => "xhigh",
            ThinkingLevel::Max => "max",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "minimal" => ThinkingLevel::Minimal,
            "low" => ThinkingLevel::Low,
            "medium" => ThinkingLevel::Medium,
            "high" => ThinkingLevel::High,
            "xhigh" => ThinkingLevel::Xhigh,
            "max" => ThinkingLevel::Max,
            _ => return None,
        })
    }
}

impl std::fmt::Display for ThinkingLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Thinking levels including `off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

pub const MODEL_THINKING_LEVELS: [ModelThinkingLevel; 7] = [
    ModelThinkingLevel::Off,
    ModelThinkingLevel::Minimal,
    ModelThinkingLevel::Low,
    ModelThinkingLevel::Medium,
    ModelThinkingLevel::High,
    ModelThinkingLevel::Xhigh,
    ModelThinkingLevel::Max,
];

impl ModelThinkingLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelThinkingLevel::Off => "off",
            ModelThinkingLevel::Minimal => "minimal",
            ModelThinkingLevel::Low => "low",
            ModelThinkingLevel::Medium => "medium",
            ModelThinkingLevel::High => "high",
            ModelThinkingLevel::Xhigh => "xhigh",
            ModelThinkingLevel::Max => "max",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value == "off" {
            return Some(ModelThinkingLevel::Off);
        }
        ThinkingLevel::parse(value).map(Self::from)
    }

    /// The thinking level, or `None` for `off`.
    pub fn thinking(self) -> Option<ThinkingLevel> {
        Some(match self {
            ModelThinkingLevel::Off => return None,
            ModelThinkingLevel::Minimal => ThinkingLevel::Minimal,
            ModelThinkingLevel::Low => ThinkingLevel::Low,
            ModelThinkingLevel::Medium => ThinkingLevel::Medium,
            ModelThinkingLevel::High => ThinkingLevel::High,
            ModelThinkingLevel::Xhigh => ThinkingLevel::Xhigh,
            ModelThinkingLevel::Max => ThinkingLevel::Max,
        })
    }
}

impl From<ThinkingLevel> for ModelThinkingLevel {
    fn from(level: ThinkingLevel) -> Self {
        match level {
            ThinkingLevel::Minimal => ModelThinkingLevel::Minimal,
            ThinkingLevel::Low => ModelThinkingLevel::Low,
            ThinkingLevel::Medium => ModelThinkingLevel::Medium,
            ThinkingLevel::High => ModelThinkingLevel::High,
            ThinkingLevel::Xhigh => ModelThinkingLevel::Xhigh,
            ThinkingLevel::Max => ModelThinkingLevel::Max,
        }
    }
}

impl std::fmt::Display for ModelThinkingLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelInputModality {
    Text,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheRetention {
    None,
    Short,
    Long,
}

/// Provider value sent for each thinking level. `None` (JSON `null`) marks a level unsupported.
pub type ThinkingLevelMap = BTreeMap<ModelThinkingLevel, Option<String>>;

/// Best-effort prompt cache lifetime in seconds for the short and long retention tiers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelPromptCache {
    #[serde(default, skip_serializing_if = "Option::is_none", with = "opt_js_f64")]
    pub short: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "opt_js_f64")]
    pub long: Option<f64>,
}

/// Cost rates in USD per million tokens.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCostRates {
    #[serde(with = "js_f64")]
    pub input: f64,
    #[serde(with = "js_f64")]
    pub output: f64,
    #[serde(with = "js_f64")]
    pub cache_read: f64,
    #[serde(with = "js_f64")]
    pub cache_write: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCostTier {
    #[serde(with = "js_f64")]
    pub input_tokens_above: f64,
    #[serde(with = "js_f64")]
    pub input: f64,
    #[serde(with = "js_f64")]
    pub output: f64,
    #[serde(with = "js_f64")]
    pub cache_read: f64,
    #[serde(with = "js_f64")]
    pub cache_write: f64,
}

impl ModelCostTier {
    pub fn rates(&self) -> ModelCostRates {
        ModelCostRates {
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    #[serde(with = "js_f64")]
    pub input: f64,
    #[serde(with = "js_f64")]
    pub output: f64,
    #[serde(with = "js_f64")]
    pub cache_read: f64,
    #[serde(with = "js_f64")]
    pub cache_write: f64,
    /// Request-wide pricing tiers. The highest matching input threshold applies to the full request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<ModelCostTier>>,
}

impl ModelCost {
    pub fn rates(&self) -> ModelCostRates {
        ModelCostRates {
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelImageResizeOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<u64>,
    /// Maximum base64-encoded payload size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jpeg_quality: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelImageInputLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize: Option<ModelImageResizeOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_message: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_request: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInputLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_request_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<ModelImageInputLimits>,
}
