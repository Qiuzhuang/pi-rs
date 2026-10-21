//! OpenRouter catalog pricing helpers (port of `providers/openrouter-api.ts`).

use serde::{Deserialize, Serialize};

use crate::types::{ModelCost, ModelCostTier};

/// OpenRouter's per-token prices, as decimal strings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenRouterPricing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_cache_read: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_cache_write: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Vec<OpenRouterPricingOverride>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenRouterPricingOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_prompt_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utc_start: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utc_end: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utc_days: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_cache_read: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_cache_write: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenRouterTopProvider {
    #[serde(default)]
    pub context_length: Option<f64>,
    #[serde(default)]
    pub max_completion_tokens: Option<f64>,
}

/// The fields pi reads from a model in OpenRouter's `GET /models` and `GET /models/user`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenRouterModel {
    pub id: String,
    #[serde(default)]
    pub context_length: Option<f64>,
    #[serde(default)]
    pub top_provider: Option<OpenRouterTopProvider>,
    #[serde(default)]
    pub pricing: Option<OpenRouterPricing>,
}

/// Convert one $/token price to $/1M tokens; invalid/negative prices use the fallback.
fn per_million(value: Option<&str>, fallback: f64) -> f64 {
    let per_token = crate::utils::provider_retry::js_parse_float(value.unwrap_or(""));
    if per_token >= 0.0 {
        let scaled = per_token * 1_000_000.0;
        // Number(x.toFixed(6))
        (scaled * 1e6).round() / 1e6
    } else {
        fallback
    }
}

/// Convert OpenRouter's $/token prices to pi's $/1M tokens.
pub fn open_router_cost(pricing: Option<&OpenRouterPricing>) -> ModelCost {
    let input = per_million(pricing.and_then(|p| p.prompt.as_deref()), 0.0);
    let output = per_million(pricing.and_then(|p| p.completion.as_deref()), 0.0);
    let cache_read = per_million(pricing.and_then(|p| p.input_cache_read.as_deref()), 0.0);
    let cache_write = per_million(pricing.and_then(|p| p.input_cache_write.as_deref()), 0.0);
    let tiers: Vec<ModelCostTier> = pricing
        .and_then(|p| p.overrides.as_ref())
        .map(|overrides| {
            overrides
                .iter()
                .filter_map(|o| {
                    let min = o.min_prompt_tokens?;
                    if o.utc_start.is_some() || o.utc_end.is_some() || o.utc_days.is_some() {
                        return None;
                    }
                    Some(ModelCostTier {
                        input_tokens_above: min,
                        input: per_million(o.prompt.as_deref(), input),
                        output: per_million(o.completion.as_deref(), output),
                        cache_read: per_million(o.input_cache_read.as_deref(), cache_read),
                        cache_write: per_million(o.input_cache_write.as_deref(), cache_write),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    ModelCost { input, output, cache_read, cache_write, tiers: if tiers.is_empty() { None } else { Some(tiers) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_prices_and_tiers() {
        let pricing = OpenRouterPricing {
            prompt: Some("0.000003".into()),
            completion: Some("-1".into()),
            overrides: Some(vec![
                OpenRouterPricingOverride { min_prompt_tokens: Some(200000.0), prompt: Some("0.000006".into()), ..Default::default() },
                OpenRouterPricingOverride { min_prompt_tokens: Some(1.0), utc_start: Some(1.0), ..Default::default() },
            ]),
            ..Default::default()
        };
        let cost = open_router_cost(Some(&pricing));
        assert_eq!(cost.input, 3.0);
        assert_eq!(cost.output, 0.0);
        let tiers = cost.tiers.unwrap();
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].input, 6.0);
        assert_eq!(tiers[0].input_tokens_above, 200000.0);
    }
}
