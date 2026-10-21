//! Port of `packages/coding-agent/src/core/cache-stats.ts`.

use crate::core::session_manager::SessionEntry;
use indexmap::IndexMap;
use pi_agent::types::AgentMessage;
use pi_ai::types::{AssistantMessage, Usage};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Prompt-cache TTL: idle gaps longer than this are worth mentioning as the
/// likely cause of a miss. Anthropic's default cache TTL is 5 minutes.
pub const CACHE_TTL_MS: i64 = 300000;

/// A counted cache miss on a single assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheMiss {
    /// Prompt tokens that were in the previous turn's prompt but not read from cache.
    pub missed_tokens: u64,
    /// Extra dollars paid vs. a full cache hit; 0 when pricing is unknown.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub missed_cost: f64,
    /// Milliseconds since the previous request (which last refreshed the cache).
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub idle_ms: f64,
    /// True when the model changed relative to the previous request.
    pub model_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheWasteTotals {
    pub missed_tokens: u64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub missed_cost: f64,
    /// Number of counted misses (turns above the noise floor).
    pub miss_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceSourceGetModelResultCost {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub cache_read: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceSourceGetModelResult {
    pub cost: ModelPriceSourceGetModelResultCost,
}

/// Minimal pricing lookup, satisfied by ModelRuntime. Cost is $/million tokens.
pub trait ModelPriceSource: Send + Sync {
    fn get_model(&self, provider: &str, model_id: &str) -> Option<ModelPriceSourceGetModelResult>;
}

/// The last request seen by the scan; everything in its prompt should be cached.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousRequest {
    pub prompt_tokens: u64,
    pub model_key: String,
    pub timestamp: i64,
    /// Sticky: some earlier request in this scan segment reported cache activity.
    /// Distinguishes a total miss on a cache-read-only provider (OpenAI-style,
    /// writes unreported) from a provider that never reports caching at all.
    pub reported_cache: bool,
}

/// Per-turn misses at or below this are cache breakpoint granularity noise.
const NOISE_FLOOR_TOKENS: u64 = 1024;

fn prompt_tokens(usage: &Usage) -> u64 {
    usage.input + usage.cache_read + usage.cache_write
}

/// Compute the cache miss for one assistant message relative to the previous
/// request. Returns `None` when nothing is counted: first turn, after a
/// reset, no cache activity ever reported (provider without cache support), or
/// miss below the noise floor.
fn detect_miss(
    prev: Option<&PreviousRequest>,
    message: &AssistantMessage,
    models: &dyn ModelPriceSource,
) -> Option<CacheMiss> {
    let usage = &message.usage;
    let prompt = prompt_tokens(usage);
    let prev = prev?;
    if prompt == 0 || (usage.cache_read + usage.cache_write == 0 && !prev.reported_cache) {
        return None;
    }

    let missed = prev.prompt_tokens.min(prompt) as i64 - usage.cache_read as i64;
    if missed <= NOISE_FLOOR_TOKENS as i64 {
        return None;
    }
    let missed_tokens = missed as u64;

    let paid_tokens = usage.input + usage.cache_write;
    let paid_per_token = if paid_tokens > 0 {
        (usage.cost.input + usage.cost.cache_write) / paid_tokens as f64
    } else {
        0.0
    };
    let read_per_token = if usage.cache_read > 0 {
        usage.cost.cache_read / usage.cache_read as f64
    } else {
        models
            .get_model(&message.provider, &message.model)
            .map(|m| m.cost.cache_read)
            .unwrap_or(0.0)
            / 1_000_000.0
    };

    Some(CacheMiss {
        missed_tokens,
        missed_cost: missed_tokens as f64 * (paid_per_token - read_per_token).max(0.0),
        idle_ms: (message.timestamp - prev.timestamp).max(0) as f64,
        model_changed: format!("{}/{}", message.provider, message.model) != prev.model_key,
    })
}

fn as_previous_request(message: &AssistantMessage, reported_cache: bool) -> Option<PreviousRequest> {
    let usage = &message.usage;
    let prompt = prompt_tokens(usage);
    if prompt == 0 {
        return None;
    }
    Some(PreviousRequest {
        prompt_tokens: prompt,
        model_key: format!("{}/{}", message.provider, message.model),
        timestamp: message.timestamp,
        reported_cache: reported_cache || usage.cache_read + usage.cache_write > 0,
    })
}

struct ScanResult {
    prev: Option<PreviousRequest>,
    totals: CacheWasteTotals,
    misses: IndexMap<String, CacheMiss>,
}

fn scan(entries: &[SessionEntry], models: &dyn ModelPriceSource) -> ScanResult {
    let mut prev: Option<PreviousRequest> = None;
    let mut totals = CacheWasteTotals {
        missed_tokens: 0,
        missed_cost: 0.0,
        miss_count: 0,
    };
    let mut misses = IndexMap::new();

    for entry in entries {
        match entry {
            SessionEntry::Compaction(_) | SessionEntry::BranchSummary(_) => {
                // The context legitimately changed; the next turn's prompt is new content,
                // not re-billed content. Model switches are NOT exempt: they re-bill the
                // full prompt and should be counted.
                prev = None;
            }
            SessionEntry::Usage(e) if e.kind == "cache_warm" => {
                let prompt = prompt_tokens(&e.usage);
                if prompt > 0 {
                    prev = Some(PreviousRequest {
                        prompt_tokens: prompt,
                        model_key: format!("{}/{}", e.provider, e.model),
                        timestamp: chrono::DateTime::parse_from_rfc3339(&e.timestamp)
                            .map(|d| d.timestamp_millis())
                            .unwrap_or(0),
                        reported_cache: true,
                    });
                }
            }
            SessionEntry::Message(e) => {
                if let AgentMessage::Assistant(message) = &e.message {
                    if let Some(miss) = detect_miss(prev.as_ref(), message, models) {
                        totals.missed_tokens += miss.missed_tokens;
                        totals.missed_cost += miss.missed_cost;
                        totals.miss_count += 1;
                        misses.insert(e.id.clone(), miss);
                    }
                    let reported = prev.as_ref().is_some_and(|p| p.reported_cache);
                    if let Some(next) = as_previous_request(message, reported) {
                        prev = Some(next);
                    }
                }
            }
            _ => {}
        }
    }
    ScanResult {
        prev,
        totals,
        misses,
    }
}

/// Cumulative cache waste across a session: prompt tokens that should have been
/// cache reads (they were in the previous turn's prompt) but were re-billed.
pub fn compute_cache_waste(
    entries: &[SessionEntry],
    models: Arc<dyn ModelPriceSource>,
) -> CacheWasteTotals {
    scan(entries, models.as_ref()).totals
}

/// All counted cache misses across a session, keyed by the id of the session entry whose
/// assistant message paid for them (TS keys by message reference). Used to re-derive
/// transcript notices when rebuilding the chat from entries (resume, post-compaction rebuild).
pub fn collect_cache_misses(
    entries: &[SessionEntry],
    models: Arc<dyn ModelPriceSource>,
) -> IndexMap<String, CacheMiss> {
    scan(entries, models.as_ref()).misses
}

/// Detect a cache miss on a just-completed assistant message.
/// `entries` must not yet contain `message` (message_end fires before persistence).
pub fn detect_cache_miss(
    entries: &[SessionEntry],
    message: &AssistantMessage,
    models: Arc<dyn ModelPriceSource>,
) -> Option<CacheMiss> {
    let result = scan(entries, models.as_ref());
    detect_miss(result.prev.as_ref(), message, models.as_ref())
}
