//! Port of `packages/coding-agent/src/core/usage-totals.ts`.

use crate::core::session_manager::SessionEntry;
use indexmap::IndexMap;
use pi_agent::types::AgentMessage;
use pi_ai::types::{Usage, UsageCost};
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
    UsageTotals {
        input: 0.0,
        output: 0.0,
        cache_read: 0.0,
        cache_write: 0.0,
        cost: 0.0,
    }
}

pub fn add_usage_to_totals(totals: &mut UsageTotals, usage: &Usage) {
    totals.input += usage.input as f64;
    totals.output += usage.output as f64;
    totals.cache_read += usage.cache_read as f64;
    totals.cache_write += usage.cache_write as f64;
    totals.cost += usage.cost.total;
}

/// Sum of two usages, keeping the optional token splits when either side reports them.
pub fn combine_usage(first: &Usage, second: &Usage) -> Usage {
    Usage {
        input: first.input + second.input,
        output: first.output + second.output,
        cache_read: first.cache_read + second.cache_read,
        cache_write: first.cache_write + second.cache_write,
        cache_write_1h: if first.cache_write_1h.is_some() || second.cache_write_1h.is_some() {
            Some(first.cache_write_1h.unwrap_or(0) + second.cache_write_1h.unwrap_or(0))
        } else {
            None
        },
        reasoning: if first.reasoning.is_some() || second.reasoning.is_some() {
            Some(first.reasoning.unwrap_or(0) + second.reasoning.unwrap_or(0))
        } else {
            None
        },
        total_tokens: first.total_tokens + second.total_tokens,
        cost: UsageCost {
            input: first.cost.input + second.cost.input,
            output: first.cost.output + second.cost.output,
            cache_read: first.cost.cache_read + second.cost.cache_read,
            cache_write: first.cost.cache_write + second.cost.cache_write,
            total: first.cost.total + second.cost.total,
        },
    }
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
    let mut totals_by_key: IndexMap<String, UsageTotals> = IndexMap::new();

    for entry in entries {
        let (key, usage): (String, &Usage) = match entry {
            SessionEntry::Message(e) => match &e.message {
                AgentMessage::Assistant(m) => (
                    format!(
                        "{}/{}",
                        m.provider,
                        m.response_model.as_deref().unwrap_or(&m.model)
                    ),
                    &m.usage,
                ),
                AgentMessage::ToolResult(m) => match &m.usage {
                    Some(usage) => ("Tools/summaries".to_string(), usage),
                    None => continue,
                },
                _ => continue,
            },
            SessionEntry::Usage(e) => (format!("{}/{}", e.provider, e.model), &e.usage),
            SessionEntry::BranchSummary(e) => match &e.usage {
                Some(usage) => ("Tools/summaries".to_string(), usage),
                None => continue,
            },
            SessionEntry::Compaction(e) => match &e.usage {
                Some(usage) => ("Tools/summaries".to_string(), usage),
                None => continue,
            },
            _ => continue,
        };
        let totals = totals_by_key.entry(key).or_insert_with(create_usage_totals);
        add_usage_to_totals(totals, usage);
    }

    let mut result: Vec<UsageCostBreakdownEntry> = totals_by_key
        .into_iter()
        .map(|(key, totals)| UsageCostBreakdownEntry {
            key,
            cost: totals.cost,
            tokens: (totals.input + totals.output + totals.cache_read + totals.cache_write) as u64,
        })
        .filter(|entry| entry.cost > 0.0 || entry.tokens > 0)
        .collect();
    // Stable sort, descending by cost (Array.prototype.sort is stable).
    result.sort_by(|a, b| b.cost.partial_cmp(&a.cost).unwrap_or(std::cmp::Ordering::Equal));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, cost: f64) -> Usage {
        Usage {
            input,
            total_tokens: input,
            cost: UsageCost {
                input: cost,
                total: cost,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn combines_optional_splits() {
        let mut a = usage(1, 1.0);
        a.reasoning = Some(2);
        let b = usage(2, 0.5);
        let c = combine_usage(&a, &b);
        assert_eq!(c.input, 3);
        assert_eq!(c.reasoning, Some(2));
        assert_eq!(c.cache_write_1h, None);
        assert_eq!(c.cost.total, 1.5);
    }

    #[test]
    fn totals_accumulate() {
        let mut totals = create_usage_totals();
        add_usage_to_totals(&mut totals, &usage(10, 0.25));
        add_usage_to_totals(&mut totals, &usage(5, 0.25));
        assert_eq!(totals.input, 15.0);
        assert_eq!(totals.cost, 0.5);
    }
}
