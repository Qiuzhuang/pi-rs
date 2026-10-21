//! Port of `packages/coding-agent/src/core/usage-totals.ts`.

use crate::core::session_manager::SessionEntry;
use pi_ai::types::Usage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub input: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub output: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub cache_read: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub cache_write: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub cost: f64,
}

pub fn create_usage_totals() -> UsageTotals {
    todo!("port core/usage-totals.ts:createUsageTotals")
}

pub fn add_usage_to_totals(totals: &UsageTotals, usage: Usage) {
    todo!("port core/usage-totals.ts:addUsageToTotals")
}

/// Sum of two usages, keeping the optional token splits when either side reports them.
pub fn combine_usage(first: Usage, second: Usage) -> Usage {
    todo!("port core/usage-totals.ts:combineUsage")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCostBreakdownEntry {
    pub key: String,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub cost: f64,
    pub tokens: u64,
}

/// Group model-attributed usage by model and all other usage into a separate bucket.
pub fn get_usage_cost_breakdown(entries: &[SessionEntry]) -> Vec<UsageCostBreakdownEntry> {
    todo!("port core/usage-totals.ts:getUsageCostBreakdown")
}
