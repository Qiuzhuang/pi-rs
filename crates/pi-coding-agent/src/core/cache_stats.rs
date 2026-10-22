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

#[cfg(test)]
mod tests {
    //! Ported from `test/cache-stats.test.ts`.
    use super::*;
    use serde_json::{Value, json};

    struct Models;
    impl ModelPriceSource for Models {
        // $/million tokens; used as cache-read price fallback on full-miss turns
        fn get_model(&self, _provider: &str, _model_id: &str) -> Option<ModelPriceSourceGetModelResult> {
            Some(ModelPriceSourceGetModelResult {
                cost: ModelPriceSourceGetModelResultCost { cache_read: 0.3 },
            })
        }
    }

    fn models() -> Arc<dyn ModelPriceSource> {
        Arc::new(Models)
    }

    #[derive(Default)]
    struct Opts {
        input: u64,
        cache_read: u64,
        cache_write: u64,
        cost_cache_read: f64,
        cost_cache_write: f64,
        model: Option<&'static str>,
        timestamp: i64,
    }

    fn assistant(o: Opts) -> AssistantMessage {
        serde_json::from_value(json!({
            "role": "assistant",
            "content": [],
            "api": "anthropic-messages",
            "provider": "test",
            "model": o.model.unwrap_or("test-model"),
            "usage": {
                "input": o.input,
                "output": 10,
                "cacheRead": o.cache_read,
                "cacheWrite": o.cache_write,
                "totalTokens": 0,
                "cost": {
                    "input": 0, "output": 0,
                    "cacheRead": o.cost_cache_read, "cacheWrite": o.cost_cache_write,
                    "total": 0
                }
            },
            "stopReason": "stop",
            "timestamp": o.timestamp,
        }))
        .unwrap()
    }

    fn entry_value(value: Value) -> SessionEntry {
        serde_json::from_value(value).unwrap()
    }

    fn entry(id: &str, message: &AssistantMessage) -> SessionEntry {
        entry_value(json!({
            "type": "message", "id": id, "parentId": null, "timestamp": "",
            "message": serde_json::to_value(AgentMessage::Assistant(message.clone())).unwrap(),
        }))
    }

    fn usage_entry(kind: &str, timestamp: i64) -> SessionEntry {
        let ts = chrono::DateTime::from_timestamp_millis(timestamp)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        entry_value(json!({
            "type": "usage", "id": format!("usage-{kind}"), "parentId": null, "timestamp": ts,
            "kind": kind, "provider": "test", "model": "test-model",
            "usage": {
                "input": 0, "output": 0, "cacheRead": 100_000, "cacheWrite": 0, "totalTokens": 100_000,
                "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0}
            }
        }))
    }

    // Turn 1: fresh 100k cache write at $3.75/M
    fn turn1() -> AssistantMessage {
        assistant(Opts { cache_write: 100_000, cost_cache_write: 0.375, ..Default::default() })
    }
    // Turn 2: healthy, everything read back at $0.30/M
    fn turn2() -> AssistantMessage {
        assistant(Opts {
            cache_read: 100_000,
            cache_write: 5_000,
            cost_cache_read: 0.03,
            cost_cache_write: 0.019,
            timestamp: 60_000,
            ..Default::default()
        })
    }

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 5e-6, "{a} != {b}");
    }

    #[test]
    fn accumulates_missed_tokens_and_cost_across_turns() {
        // Turn 3: full miss, previous 105k prompt re-billed at $3.75/M write
        let turn3 = assistant(Opts { cache_write: 110_000, cost_cache_write: 0.4125, timestamp: 120_000, ..Default::default() });
        let totals = compute_cache_waste(&[entry("1", &turn1()), entry("2", &turn2()), entry("3", &turn3)], models());
        assert_eq!(totals.missed_tokens, 105_000);
        // 105k at ($3.75 - $0.30)/M
        close(totals.missed_cost, 0.36225);
    }

    #[test]
    fn counts_nothing_for_healthy_sessions() {
        let totals = compute_cache_waste(&[entry("1", &turn1()), entry("2", &turn2())], models());
        assert_eq!(totals.missed_tokens, 0);
        assert_eq!(totals.missed_cost, 0.0);
    }

    #[test]
    fn skips_the_turn_after_a_compaction_reset() {
        let reset = entry_value(json!({
            "type": "compaction", "id": "c", "parentId": null, "timestamp": "",
            "summary": "", "firstKeptEntryId": "", "tokensBefore": 0
        }));
        let after_reset = assistant(Opts { cache_write: 20_000, cost_cache_write: 0.075, ..Default::default() });
        let totals = compute_cache_waste(&[entry("1", &turn1()), reset, entry("2", &after_reset)], models());
        assert_eq!(totals.missed_tokens, 0);
    }

    #[test]
    fn counts_misses_caused_by_model_switches() {
        let other = assistant(Opts { cache_write: 100_000, cost_cache_write: 0.375, model: Some("other-model"), ..Default::default() });
        let totals = compute_cache_waste(&[entry("1", &turn1()), entry("2", &other)], models());
        assert_eq!(totals.missed_tokens, 100_000);
        assert_eq!(totals.miss_count, 1);
    }

    #[test]
    fn skips_providers_that_report_no_cache_activity() {
        let a = assistant(Opts { input: 100_000, ..Default::default() });
        let b = assistant(Opts { input: 110_000, ..Default::default() });
        let totals = compute_cache_waste(&[entry("1", &a), entry("2", &b)], models());
        assert_eq!(totals.missed_tokens, 0);
    }

    #[test]
    fn collect_cache_misses_maps_counted_misses_to_their_entries() {
        let miss_turn = assistant(Opts { cache_write: 110_000, cost_cache_write: 0.4125, timestamp: 120_000, ..Default::default() });
        let misses = collect_cache_misses(
            &[entry("1", &turn1()), entry("2", &turn2()), entry("miss", &miss_turn)],
            models(),
        );
        assert_eq!(misses.len(), 1);
        assert_eq!(misses.get("miss").unwrap().missed_tokens, 105_000);
    }

    #[test]
    fn detects_a_miss_on_a_just_completed_message_with_idle_time() {
        let miss_message = assistant(Opts { cache_write: 110_000, cost_cache_write: 0.4125, timestamp: 600_000, ..Default::default() });
        let miss = detect_cache_miss(&[entry("1", &turn1()), entry("2", &turn2())], &miss_message, models()).unwrap();
        assert_eq!(miss.missed_tokens, 105_000);
        close(miss.missed_cost, 0.36225);
        // 600s - 60s since the previous request
        assert_eq!(miss.idle_ms, 540_000.0);
        assert!(!miss.model_changed);
    }

    #[test]
    fn flags_model_switches_on_detected_misses() {
        let other = assistant(Opts {
            cache_write: 110_000,
            cost_cache_write: 0.4125,
            model: Some("other-model"),
            timestamp: 120_000,
            ..Default::default()
        });
        let miss = detect_cache_miss(&[entry("1", &turn1()), entry("2", &turn2())], &other, models()).unwrap();
        assert_eq!(miss.missed_tokens, 105_000);
        assert!(miss.model_changed);
    }

    #[test]
    fn uses_only_cache_warm_usage_entries_as_cache_refreshes() {
        let miss_message = assistant(Opts { cache_write: 110_000, cost_cache_write: 0.4125, timestamp: 600_000, ..Default::default() });
        let after_warm =
            detect_cache_miss(&[entry("1", &turn1()), usage_entry("cache_warm", 500_000)], &miss_message, models());
        let after_other =
            detect_cache_miss(&[entry("1", &turn1()), usage_entry("custom_operation", 500_000)], &miss_message, models());
        assert_eq!(after_warm.unwrap().idle_ms, 100_000.0);
        assert_eq!(after_other.unwrap().idle_ms, 600_000.0);
    }

    #[test]
    fn returns_none_for_healthy_turns() {
        let healthy = assistant(Opts {
            cache_read: 105_000,
            cache_write: 2_000,
            cost_cache_read: 0.0315,
            cost_cache_write: 0.0075,
            timestamp: 120_000,
            ..Default::default()
        });
        assert!(detect_cache_miss(&[entry("1", &turn1()), entry("2", &turn2())], &healthy, models()).is_none());
    }

    #[test]
    fn returns_none_for_the_first_turn_of_a_session() {
        assert!(detect_cache_miss(&[], &turn1(), models()).is_none());
    }
}
