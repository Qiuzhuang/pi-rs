//! Port of `packages/coding-agent/src/core/cache-warmer.ts`.

use crate::core::session_manager::SessionEntry;
use crate::core::session_manager::UsageEntry;
use crate::core::settings_schema::CacheWarmingMode;
use futures::future::BoxFuture;
use pi_ai::models::ModelsRequestOptions;
use pi_ai::types::Context;
use pi_ai::types::Model;
use pi_ai::types::SimpleStreamOptions;
use pi_ai::types::Usage;
use pi_ai::utils::event_stream::AssistantMessageEventStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Refresh at 90% of the TTL while preserving at least ten seconds of margin.
pub fn get_cache_warming_delay_ms(ttl_ms: i64) -> Option<i64> {
    todo!("port core/cache-warmer.ts:getCacheWarmingDelayMs")
}

/// Lifetime of the prompt cache entry a request writes, from the model's
/// `promptCache` tier for the retention the request used. Undefined when the
/// model has no lifetime for that tier or caching is off.
pub fn get_prompt_cache_ttl_ms(model: Model, options: Option<SimpleStreamOptions>) -> Option<i64> {
    todo!("port core/cache-warmer.ts:getPromptCacheTtlMs")
}

/// Whether replaying the request with a one-token output cap leaves its cache
/// entry untouched. Anthropic's budget-based thinking (Claude models without
/// adaptive thinking) derives `budget_tokens` from `max_tokens`; the replay
/// would get a different budget, which Anthropic keys the message cache on,
/// and the model could still think for thousands of tokens.
pub fn is_replayable(model: Model, options: Option<SimpleStreamOptions>) -> bool {
    todo!("port core/cache-warmer.ts:isReplayable")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CacheWarmingAction {
    #[serde(rename = "warm")]
    Warm,
    #[serde(rename = "stop")]
    Stop,
}

impl CacheWarmingAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Warm => "warm",
            Self::Stop => "stop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CacheWarmingDecisionPhase {
    #[serde(rename = "streaming")]
    Streaming,
    #[serde(rename = "idle")]
    Idle,
}

impl CacheWarmingDecisionPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::Idle => "idle",
        }
    }
}

/// Inputs and outcome of one warm-or-stop decision, as shown by `/session`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheWarmingDecision {
    /// "streaming" while the agent run that sent the request is still active.
    pub phase: CacheWarmingDecisionPhase,
    /// Price of this refresh: a cache read of the prompt plus one output token.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub warm_cost: f64,
    /// Extra price of the next real request if the cache entry is lost.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub miss_cost: f64,
    /// Estimated chance that a real request arrives before the entry expires.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub continuation_probability: f64,
    /// `continuationProbability * missCost - warmCost`.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub expected_savings: f64,
    /// False when the prompt size or the model's prices are unknown.
    pub economics_available: bool,
    /// Pi's decision: "warm" when `expectedSavings` is at least $0.05.
    pub action: CacheWarmingAction,
}

/// Fired before each refresh with pi's decision filled in. Everything else an
/// extension might want (model, idle state, context size) is on the context.
// TS: extends Pick<CacheWarmingDecision, "warmCost" | "missCost" | "continuationProbability" | "action"> (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheWarmingDecisionEvent {
    /// Price of this refresh: a cache read of the prompt plus one output token.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub warm_cost: f64,
    /// Extra price of the next real request if the cache entry is lost.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub miss_cost: f64,
    /// Estimated chance that a real request arrives before the entry expires.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub continuation_probability: f64,
    /// Pi's decision: "warm" when `expectedSavings` is at least $0.05.
    pub action: CacheWarmingAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheWarmingDecisionEventResult {
    /// Override whether this refresh is sent. "stop" ends warming until the next real request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<CacheWarmingAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CacheWarmingStatusState {
    #[serde(rename = "inactive")]
    Inactive,
    #[serde(rename = "scheduled")]
    Scheduled,
    #[serde(rename = "refreshing")]
    Refreshing,
}

impl CacheWarmingStatusState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Scheduled => "scheduled",
            Self::Refreshing => "refreshing",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheWarmingStatus {
    /// "scheduled": a refresh timer is armed; "refreshing": a warm request is in flight.
    pub state: CacheWarmingStatusState,
    /// Why nothing is scheduled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub next_warm_at: Option<f64>,
    /// The pending decision, or the decision that stopped warming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<CacheWarmingDecision>,
    /// True when an extension changed `decision.action`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_override: Option<bool>,
}

/// The request whose prompt cache entry should be kept warm, exactly as it was sent.
#[derive(Clone)]
pub struct CacheWarmRequest {
    pub model: Model,
    pub context: Context,
    pub options: ModelsRequestOptions,
}

// TS: extends CacheWarmRequest (inherited fields inlined)
#[derive(Clone)]
pub struct ActiveRun {
    pub model: Model,
    pub context: Context,
    pub options: ModelsRequestOptions,
    /// False once the session's model or messages no longer match the request.
    pub is_current: Arc<dyn Fn() -> bool + Send + Sync>,
    pub ttl_ms: i64,
    pub delay_ms: i64,
    /// Latest safe time to send this refresh, leaving half the original expiry margin.
    pub refresh_deadline_at: i64,
    pub started_at: i64,
    pub controller: CancellationToken,
    pub phase: CacheWarmingDecisionPhase,
    pub next_warm_at: f64,
    /// Set while a refresh that an extension forced is in flight.
    pub extension_override: bool,
    pub timer: Option<Value>,
}

#[derive(Clone)]
pub struct CacheWarmerModels {
    pub stream_simple: Arc<
        dyn Fn(Model, Context, Option<ModelsRequestOptions>) -> AssistantMessageEventStream
            + Send
            + Sync,
    >,
}

#[derive(Clone)]
pub struct CacheWarmerSessionManager {
    /// Append model-attributed usage that does not participate in LLM context. Returns the appended entry.
    pub append_usage:
        Arc<dyn Fn(String, String, String, Usage, Option<String>) -> UsageEntry + Send + Sync>,
    /// Walk from entry to root, returning all entries in path order.
    /// Includes all entry types (messages, compaction, model changes, etc.).
    /// Use buildSessionContext() to get the resolved messages for the LLM.
    pub get_branch: Arc<dyn Fn(Option<String>) -> Vec<SessionEntry> + Send + Sync>,
}

/// Keeps one prompt cache entry alive by re-sending its request with a
/// one-token output cap before the entry expires. `start` replaces any
/// previous run; warm requests never extend the fixed safety windows.
pub struct CacheWarmer {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl CacheWarmer {
    pub fn new(
        models: &CacheWarmerModels,
        session_manager: &CacheWarmerSessionManager,
        get_mode: Arc<dyn Fn() -> CacheWarmingMode + Send + Sync>,
        decide: Option<
            Arc<
                dyn Fn(
                        CacheWarmingDecisionEvent,
                    )
                        -> BoxFuture<'static, anyhow::Result<CacheWarmingAction>>
                    + Send
                    + Sync,
            >,
        >,
    ) -> Self {
        todo!("port core/cache-warmer.ts:CacheWarmer.constructor")
    }

    /// Called with the persisted usage entry after each successful refresh.
    pub fn on_warmed(&self) -> Option<Arc<dyn Fn(UsageEntry) + Send + Sync>> {
        todo!("port core/cache-warmer.ts:CacheWarmer.onWarmed")
    }

    pub fn set_on_warmed(&self, value: Option<Arc<dyn Fn(UsageEntry) + Send + Sync>>) {
        todo!("port core/cache-warmer.ts:CacheWarmer.onWarmed")
    }

    pub fn status(&self) -> CacheWarmingStatus {
        todo!("port core/cache-warmer.ts:CacheWarmer.status")
    }

    /// Keep the prompt cache entry written by `request` warm while `isCurrent` holds.
    pub fn start(
        &self,
        request: &CacheWarmRequest,
        is_current: Arc<dyn Fn() -> bool + Send + Sync>,
    ) {
        todo!("port core/cache-warmer.ts:CacheWarmer.start")
    }

    pub fn on_agent_settled(&self) {
        todo!("port core/cache-warmer.ts:CacheWarmer.onAgentSettled")
    }

    /// Reconcile an active run after the persisted warming mode changes.
    pub fn on_mode_changed(&self) {
        todo!("port core/cache-warmer.ts:CacheWarmer.onModeChanged")
    }

    pub fn cancel(&self) {
        todo!("port core/cache-warmer.ts:CacheWarmer.cancel")
    }
}

/// One-line status for `/session`.
pub fn format_cache_warming_status(status: &CacheWarmingStatus, now: Option<f64>) -> String {
    todo!("port core/cache-warmer.ts:formatCacheWarmingStatus")
}

/// One-line transcript text for persisted cache-warming usage.
pub fn format_cache_warming_usage(entry: &UsageEntry) -> String {
    todo!("port core/cache-warmer.ts:formatCacheWarmingUsage")
}
