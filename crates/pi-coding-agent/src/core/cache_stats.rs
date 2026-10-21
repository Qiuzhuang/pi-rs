//! Port of `packages/coding-agent/src/core/cache-stats.ts`.

use crate::core::session_manager::SessionEntry;
use indexmap::IndexMap;
use pi_ai::types::AssistantMessage;
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

/// Cumulative cache waste across a session: prompt tokens that should have been
/// cache reads (they were in the previous turn's prompt) but were re-billed.
pub fn compute_cache_waste(
    entries: &[SessionEntry],
    models: Arc<dyn ModelPriceSource>,
) -> CacheWasteTotals {
    todo!("port core/cache-stats.ts:computeCacheWaste")
}

/// All counted cache misses across a session, keyed by the assistant message
/// (by reference) that paid for them. Used to re-derive transcript notices when
/// rebuilding the chat from entries (resume, post-compaction rebuild).
pub fn collect_cache_misses(
    entries: &[SessionEntry],
    models: Arc<dyn ModelPriceSource>,
) -> IndexMap<AssistantMessage, CacheMiss> {
    todo!("port core/cache-stats.ts:collectCacheMisses")
}

/// Detect a cache miss on a just-completed assistant message.
/// `entries` must not yet contain `message` (message_end fires before persistence).
pub fn detect_cache_miss(
    entries: &[SessionEntry],
    message: AssistantMessage,
    models: Arc<dyn ModelPriceSource>,
) -> Option<CacheMiss> {
    todo!("port core/cache-stats.ts:detectCacheMiss")
}
