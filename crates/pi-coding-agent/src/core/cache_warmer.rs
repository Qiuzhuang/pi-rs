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
use parking_lot::Mutex;
use pi_agent::types::AgentMessage;
use pi_ai::models::calculate_cost;
use pi_ai::types::{CacheRetention, StopReason};
use pi_ai::utils::provider_env::get_provider_env_value;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Refresh at 90% of the TTL while preserving at least ten seconds of margin.
pub fn get_cache_warming_delay_ms(ttl_ms: i64) -> Option<i64> {
    if ttl_ms <= 10_000 {
        return None;
    }
    let delay = ((ttl_ms as f64 * 0.9).min((ttl_ms - 10_000) as f64)).floor() as i64;
    Some(delay.max(1))
}

/// Lifetime of the prompt cache entry a request writes, from the model's
/// `promptCache` tier for the retention the request used. Undefined when the
/// model has no lifetime for that tier or caching is off.
pub fn get_prompt_cache_ttl_ms(model: &Model, options: Option<&SimpleStreamOptions>) -> Option<i64> {
    let retention = match options.and_then(|o| o.cache_retention) {
        Some(retention) => retention,
        None => {
            if get_provider_env_value("PI_CACHE_RETENTION", options.and_then(|o| o.env.as_ref()))
                .as_deref()
                == Some("long")
            {
                CacheRetention::Long
            } else {
                CacheRetention::Short
            }
        }
    };
    let cache = model.prompt_cache.as_ref()?;
    let seconds = match retention {
        CacheRetention::None => return None,
        CacheRetention::Short => cache.short,
        CacheRetention::Long => cache.long,
    }?;
    Some((seconds * 1000.0) as i64)
}

/// Whether replaying the request with a one-token output cap leaves its cache
/// entry untouched. Anthropic's budget-based thinking (Claude models without
/// adaptive thinking) derives `budget_tokens` from `max_tokens`; the replay
/// would get a different budget, which Anthropic keys the message cache on,
/// and the model could still think for thousands of tokens.
pub fn is_replayable(model: &Model, options: Option<&SimpleStreamOptions>) -> bool {
    let reasoning = options.and_then(|o| o.reasoning);
    if reasoning.is_none() || model.api != "anthropic-messages" {
        return true;
    }
    model.compat().force_adaptive_thinking == Some(true)
}

/// Prompt size of the most recent real request on the branch, as reported by the provider.
fn last_prompt_tokens(entries: &[SessionEntry]) -> u64 {
    for entry in entries.iter().rev() {
        if let SessionEntry::Message(e) = entry
            && let AgentMessage::Assistant(m) = &e.message
        {
            return m.usage.input + m.usage.cache_read + m.usage.cache_write;
        }
    }
    0
}

#[derive(Default)]
struct PriceTokens {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

fn price(model: &Model, tokens: PriceTokens) -> f64 {
    let mut usage = Usage {
        input: tokens.input,
        output: tokens.output,
        cache_read: tokens.cache_read,
        cache_write: tokens.cache_write,
        ..Default::default()
    };
    calculate_cost(&model.cost, &mut usage).total
}

/// Streaming warming never continues past this long after the real request that started it.
const MAX_WARMING_AGE_MS: i64 = 60 * 60_000;
/// Idle warming uses a shorter horizon because continuation estimates become less reliable with age.
const MAX_IDLE_WARMING_AGE_MS: i64 = 30 * 60_000;
/// A refresh is sent only when it is expected to save at least this many dollars.
const CACHE_WARMING_MINIMUM_EXPECTED_SAVINGS: f64 = 0.05;
/// Chance that a real request arrives before the cache entry expires while the
/// agent sits idle. Measured from our own usage; per-session estimates were not
/// better than this constant.
const IDLE_CONTINUATION_PROBABILITY: f64 = 0.15;

/// Wall-clock milliseconds that follow tokio's clock, so paused-time tests can advance it.
fn now_ms() -> i64 {
    static ANCHOR: std::sync::LazyLock<(i64, tokio::time::Instant)> = std::sync::LazyLock::new(|| {
        (
            chrono::Utc::now().timestamp_millis(),
            tokio::time::Instant::now(),
        )
    });
    let (epoch, instant) = *ANCHOR;
    let now = tokio::time::Instant::now();
    if now >= instant {
        epoch + now.duration_since(instant).as_millis() as i64
    } else {
        epoch - instant.duration_since(now).as_millis() as i64
    }
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

/// Decision callback of a [`CacheWarmer`]: lets extensions override `event.action`.
pub type CacheWarmingDecideFn = Arc<
    dyn Fn(CacheWarmingDecisionEvent) -> BoxFuture<'static, anyhow::Result<CacheWarmingAction>>
        + Send
        + Sync,
>;

struct ActiveRun {
    id: u64,
    request: CacheWarmRequest,
    /// False once the session's model or messages no longer match the request.
    is_current: Arc<dyn Fn() -> bool + Send + Sync>,
    ttl_ms: i64,
    delay_ms: i64,
    /// Latest safe time to send this refresh, leaving half the original expiry margin.
    refresh_deadline_at: i64,
    started_at: i64,
    controller: CancellationToken,
    phase: CacheWarmingDecisionPhase,
    next_warm_at: i64,
    /// Set while a refresh that an extension forced is in flight.
    extension_override: bool,
    timer: Option<tokio::task::JoinHandle<()>>,
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

struct WarmerState {
    run: Option<ActiveRun>,
    inactive: CacheWarmingStatus,
    on_warmed: Option<Arc<dyn Fn(UsageEntry) + Send + Sync>>,
}

/// Keeps one prompt cache entry alive by re-sending its request with a
/// one-token output cap before the entry expires. `start` replaces any
/// previous run; warm requests never extend the fixed safety windows.
pub struct CacheWarmer {
    weak: std::sync::Weak<CacheWarmer>,
    state: Mutex<WarmerState>,
    next_run_id: std::sync::atomic::AtomicU64,
    models: CacheWarmerModels,
    session_manager: CacheWarmerSessionManager,
    get_mode: Arc<dyn Fn() -> CacheWarmingMode + Send + Sync>,
    /// Lets extensions override `event.action`; failures fall back to pi's decision.
    decide: CacheWarmingDecideFn,
}

/// Snapshot of the run fields `evaluate` needs.
struct RunView {
    model: Model,
    phase: CacheWarmingDecisionPhase,
}

fn inactive(reason: &str) -> CacheWarmingStatus {
    CacheWarmingStatus {
        state: CacheWarmingStatusState::Inactive,
        reason: Some(reason.to_string()),
        next_warm_at: None,
        decision: None,
        extension_override: None,
    }
}

impl CacheWarmer {
    pub fn new(
        models: &CacheWarmerModels,
        session_manager: &CacheWarmerSessionManager,
        get_mode: Arc<dyn Fn() -> CacheWarmingMode + Send + Sync>,
        decide: Option<CacheWarmingDecideFn>,
    ) -> Arc<Self> {
        let decide = decide.unwrap_or_else(|| {
            Arc::new(|event: CacheWarmingDecisionEvent| {
                Box::pin(async move { Ok(event.action) }) as BoxFuture<'static, _>
            })
        });
        Arc::new_cyclic(|weak| Self {
            weak: weak.clone(),
            state: Mutex::new(WarmerState {
                run: None,
                inactive: inactive("waiting for first request"),
                on_warmed: None,
            }),
            next_run_id: std::sync::atomic::AtomicU64::new(1),
            models: models.clone(),
            session_manager: session_manager.clone(),
            get_mode,
            decide,
        })
    }

    /// Called with the persisted usage entry after each successful refresh.
    pub fn on_warmed(&self) -> Option<Arc<dyn Fn(UsageEntry) + Send + Sync>> {
        self.state.lock().on_warmed.clone()
    }

    pub fn set_on_warmed(&self, value: Option<Arc<dyn Fn(UsageEntry) + Send + Sync>>) {
        self.state.lock().on_warmed = value;
    }

    pub fn status(&self) -> CacheWarmingStatus {
        if (self.get_mode)() == CacheWarmingMode::Off {
            return inactive("cache warming disabled");
        }
        let (view, is_current, refreshing, next_warm_at, extension_override) = {
            let state = self.state.lock();
            let Some(run) = &state.run else {
                return state.inactive.clone();
            };
            (
                RunView {
                    model: run.request.model.clone(),
                    phase: run.phase,
                },
                run.is_current.clone(),
                run.timer.is_none(),
                run.next_warm_at,
                run.extension_override,
            )
        };
        if !is_current() {
            return inactive("conversation context changed");
        }
        let decision = self.evaluate(&view);
        if !decision.economics_available && !refreshing {
            return inactive("cache economics unavailable");
        }
        CacheWarmingStatus {
            state: if refreshing {
                CacheWarmingStatusState::Refreshing
            } else {
                CacheWarmingStatusState::Scheduled
            },
            reason: None,
            next_warm_at: Some(next_warm_at as f64),
            decision: Some(decision),
            extension_override: Some(extension_override),
        }
    }

    /// Keep the prompt cache entry written by `request` warm while `isCurrent` holds.
    pub fn start(
        &self,
        request: &CacheWarmRequest,
        is_current: Arc<dyn Fn() -> bool + Send + Sync>,
    ) {
        self.clear_run();
        let mode = (self.get_mode)();
        if mode == CacheWarmingMode::Off {
            self.stop("cache warming disabled", None);
            return;
        }
        let options = &request.options.options;
        if !is_replayable(&request.model, Some(options)) {
            self.stop("request cannot be replayed safely", None);
            return;
        }
        let Some(ttl_ms) = get_prompt_cache_ttl_ms(&request.model, Some(options)) else {
            self.stop(
                if options.cache_retention == Some(CacheRetention::None) {
                    "request disabled prompt caching"
                } else {
                    "cache lifetime unavailable"
                },
                None,
            );
            return;
        };
        let Some(delay_ms) = get_cache_warming_delay_ms(ttl_ms) else {
            self.stop("cache lifetime unavailable", None);
            return;
        };
        let id = self
            .next_run_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.state.lock().run = Some(ActiveRun {
            id,
            request: request.clone(),
            is_current,
            ttl_ms,
            delay_ms,
            refresh_deadline_at: 0,
            started_at: now_ms(),
            controller: CancellationToken::new(),
            phase: CacheWarmingDecisionPhase::Streaming,
            next_warm_at: 0,
            extension_override: false,
            timer: None,
        });
        self.schedule(id);
    }

    pub fn on_agent_settled(&self) {
        let mut state = self.state.lock();
        let Some(run) = state.run.as_mut() else {
            return;
        };
        if (self.get_mode)() == CacheWarmingMode::Streaming {
            drop(state);
            self.stop("agent run settled", None);
            return;
        }
        run.phase = CacheWarmingDecisionPhase::Idle;
        let deadline = run.started_at + MAX_IDLE_WARMING_AGE_MS;
        if run.next_warm_at > deadline || now_ms() >= deadline {
            drop(state);
            self.stop("30-minute idle safety limit reached", None);
        }
    }

    /// Reconcile an active run after the persisted warming mode changes.
    pub fn on_mode_changed(&self) {
        let phase = match &self.state.lock().run {
            Some(run) => run.phase,
            None => return,
        };
        if let Some(reason) = self.get_mode_stop_reason(phase) {
            self.stop(reason, None);
        }
    }

    pub fn cancel(&self) {
        self.stop("inactive", None);
    }

    fn clear_run(&self) {
        let run = self.state.lock().run.take();
        if let Some(run) = run {
            if let Some(timer) = run.timer {
                timer.abort();
            }
            run.controller.cancel();
        }
    }

    fn stop(&self, reason: &str, stopped: Option<(CacheWarmingDecision, bool)>) {
        self.clear_run();
        let mut status = inactive(reason);
        if let Some((decision, extension_override)) = stopped {
            status.decision = Some(decision);
            status.extension_override = Some(extension_override);
        }
        self.state.lock().inactive = status;
    }

    fn schedule(&self, run_id: u64) {
        let mut state = self.state.lock();
        let Some(run) = state.run.as_mut().filter(|r| r.id == run_id) else {
            return;
        };
        run.extension_override = false;
        let now = now_ms();
        run.next_warm_at = now + run.delay_ms;
        // A timer can run late after sleep or event-loop blockage. Keep half of
        // the planned pre-expiry margin for that delay and request dispatch; a
        // late refresh is likely a full-price cache write, not a cache warm.
        run.refresh_deadline_at = run.next_warm_at + (run.ttl_ms - run.delay_ms) / 2;
        let idle = run.phase == CacheWarmingDecisionPhase::Idle;
        let deadline = run.started_at
            + if idle {
                MAX_IDLE_WARMING_AGE_MS
            } else {
                MAX_WARMING_AGE_MS
            };
        if run.next_warm_at > deadline || now >= deadline {
            drop(state);
            self.stop(
                if idle {
                    "30-minute idle safety limit reached"
                } else {
                    "one-hour safety limit reached"
                },
                None,
            );
            return;
        }
        let wait = (run.next_warm_at - now).max(0) as u64;
        let weak = self.weak.clone();
        if tokio::runtime::Handle::try_current().is_err() {
            // No runtime to run the timer on: warming cannot proceed.
            return;
        }
        run.timer = Some(tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            if let Some(warmer) = weak.upgrade() {
                warmer.refresh(run_id).await;
            }
        }));
    }

    async fn refresh(&self, run_id: u64) {
        {
            let mut state = self.state.lock();
            match state.run.as_mut() {
                Some(run) if run.id == run_id => run.timer = None,
                _ => return,
            }
        }
        if !self.validate_run(run_id) || self.refresh_deadline_missed(run_id) {
            return;
        }
        let Some((view, request, controller)) = self.run_snapshot(run_id) else {
            return;
        };
        let decision = self.evaluate(&view);
        let mut action = decision.action;
        if let Ok(decided) = (self.decide)(CacheWarmingDecisionEvent {
            warm_cost: decision.warm_cost,
            miss_cost: decision.miss_cost,
            continuation_probability: decision.continuation_probability,
            action,
        })
        .await
        {
            action = decided;
        }
        // Extension failures fall back to pi's own decision.
        if !self.validate_run(run_id) || self.refresh_deadline_missed(run_id) {
            return;
        }
        let extension_override = action != decision.action;
        if action == CacheWarmingAction::Stop {
            let reason = if extension_override {
                "stopped by extension"
            } else if decision.economics_available {
                "expected savings below threshold"
            } else {
                "cache economics unavailable"
            };
            self.stop(reason, Some((decision, extension_override)));
            return;
        }

        if let Some(run) = self.state.lock().run.as_mut().filter(|r| r.id == run_id) {
            run.extension_override = extension_override;
        }
        let mut options = request.options.clone();
        options.options.max_tokens = Some(1);
        options.options.max_retries = Some(0);
        options.options.signal = Some(controller.clone());
        let stream = (self.models.stream_simple)(
            request.model.clone(),
            request.context.clone(),
            Some(options),
        );
        // Cache warming is best-effort and must not affect the active agent run.
        let message = tokio::select! {
            message = stream.result() => Some(message),
            _ = controller.cancelled() => None,
        };
        if let Some(message) = message {
            if !self.validate_run(run_id) {
                return;
            }
            if message.stop_reason != StopReason::Error && message.stop_reason != StopReason::Aborted {
                let entry = (self.session_manager.append_usage)(
                    "cache_warm".to_string(),
                    message.provider.clone(),
                    message
                        .response_model
                        .clone()
                        .unwrap_or_else(|| message.model.clone()),
                    message.usage.clone(),
                    extension_override.then(|| "extension override".to_string()),
                );
                let on_warmed = self.state.lock().on_warmed.clone();
                if let Some(on_warmed) = on_warmed {
                    on_warmed(entry);
                }
            }
        }
        let still_current = self
            .state
            .lock()
            .run
            .as_ref()
            .is_some_and(|r| r.id == run_id);
        if still_current {
            self.schedule(run_id);
        }
    }

    fn run_snapshot(&self, run_id: u64) -> Option<(RunView, CacheWarmRequest, CancellationToken)> {
        let state = self.state.lock();
        let run = state.run.as_ref().filter(|r| r.id == run_id)?;
        Some((
            RunView {
                model: run.request.model.clone(),
                phase: run.phase,
            },
            run.request.clone(),
            run.controller.clone(),
        ))
    }

    fn refresh_deadline_missed(&self, run_id: u64) -> bool {
        let deadline = match self.state.lock().run.as_ref().filter(|r| r.id == run_id) {
            Some(run) => run.refresh_deadline_at,
            None => return false,
        };
        if now_ms() <= deadline {
            return false;
        }
        self.stop("cache refresh deadline missed", None);
        true
    }

    fn validate_run(&self, run_id: u64) -> bool {
        let (phase, is_current) = match self.state.lock().run.as_ref() {
            Some(run) if run.id == run_id => (run.phase, run.is_current.clone()),
            _ => return false,
        };
        let reason = self.get_mode_stop_reason(phase).or_else(|| {
            if is_current() {
                None
            } else {
                Some("conversation context changed")
            }
        });
        match reason {
            None => true,
            Some(reason) => {
                self.stop(reason, None);
                false
            }
        }
    }

    fn get_mode_stop_reason(&self, phase: CacheWarmingDecisionPhase) -> Option<&'static str> {
        match (self.get_mode)() {
            CacheWarmingMode::Off => Some("cache warming disabled"),
            CacheWarmingMode::Streaming if phase == CacheWarmingDecisionPhase::Idle => {
                Some("agent run settled")
            }
            _ => None,
        }
    }

    fn evaluate(&self, run: &RunView) -> CacheWarmingDecision {
        let model = &run.model;
        let prompt_tokens = last_prompt_tokens(&(self.session_manager.get_branch)(None));
        let cache_hit_cost = price(
            model,
            PriceTokens {
                cache_read: prompt_tokens,
                ..Default::default()
            },
        );
        let cache_miss_cost = price(
            model,
            if model.cost.cache_write > 0.0 {
                PriceTokens {
                    cache_write: prompt_tokens,
                    ..Default::default()
                }
            } else {
                PriceTokens {
                    input: prompt_tokens,
                    ..Default::default()
                }
            },
        );
        let warm_cost = price(
            model,
            PriceTokens {
                cache_read: prompt_tokens,
                output: 1,
                ..Default::default()
            },
        );
        let miss_cost = (cache_miss_cost - cache_hit_cost).max(0.0);
        let continuation_probability = if run.phase == CacheWarmingDecisionPhase::Idle {
            IDLE_CONTINUATION_PROBABILITY
        } else {
            1.0
        };
        let economics_available =
            prompt_tokens > 0 && (cache_hit_cost > 0.0 || cache_miss_cost > 0.0);
        let expected_savings = continuation_probability * miss_cost - warm_cost;
        CacheWarmingDecision {
            phase: run.phase,
            warm_cost,
            miss_cost,
            continuation_probability,
            expected_savings,
            economics_available,
            action: if expected_savings >= CACHE_WARMING_MINIMUM_EXPECTED_SAVINGS {
                CacheWarmingAction::Warm
            } else {
                CacheWarmingAction::Stop
            },
        }
    }
}

impl Drop for CacheWarmer {
    fn drop(&mut self) {
        if let Some(run) = self.state.get_mut().run.take() {
            if let Some(timer) = run.timer {
                timer.abort();
            }
            run.controller.cancel();
        }
    }
}

fn format_dollars(value: f64) -> String {
    if value < 0.0 {
        format!("-${:.3}", value.abs())
    } else {
        format!("${value:.3}")
    }
}

fn format_cache_warming_economics(decision: &CacheWarmingDecision) -> String {
    if !decision.economics_available {
        return "cache economics unavailable".to_string();
    }
    let probability = (decision.continuation_probability * 100.0).round() as i64;
    let probability_text = if decision.phase == CacheWarmingDecisionPhase::Streaming {
        format!("{probability}% continuation probability while agent is running")
    } else {
        format!("{probability}% continuation probability")
    };
    let comparison = if decision.action == CacheWarmingAction::Warm {
        ">="
    } else {
        "<"
    };
    format!(
        "{probability_text}, expected savings {} {comparison} ${:.3}",
        format_dollars(decision.expected_savings),
        CACHE_WARMING_MINIMUM_EXPECTED_SAVINGS
    )
}

fn format_cache_warming_decision_time(next_warm_at: Option<f64>, now: f64) -> String {
    let Some(next_warm_at) = next_warm_at.filter(|n| *n > now) else {
        return "Decision now".to_string();
    };
    let mut remaining_seconds = ((next_warm_at - now) / 1000.0).ceil() as i64;
    let hours = remaining_seconds / 3600;
    remaining_seconds %= 3600;
    let minutes = remaining_seconds / 60;
    let seconds = remaining_seconds % 60;
    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{seconds}s"));
    }
    format!("Decision in {}", parts.join(" "))
}

/// One-line status for `/session`.
pub fn format_cache_warming_status(status: &CacheWarmingStatus, now: Option<f64>) -> String {
    let now = now.unwrap_or_else(|| now_ms() as f64);
    // A decision is attached once pi (or an extension) acted on it; "inactive"
    // without one never got that far.
    let decision = match &status.decision {
        Some(decision)
            if !(status.state == CacheWarmingStatusState::Inactive
                && !decision.economics_available
                && status.extension_override != Some(true)) =>
        {
            decision
        }
        _ => {
            return format!(
                "Inactive ({})",
                status.reason.as_deref().unwrap_or("unknown reason")
            );
        }
    };
    let details = if status.extension_override == Some(true) {
        format!(
            "extension override, {}",
            format_cache_warming_economics(decision)
        )
    } else {
        format!(
            "{} -> {}",
            format_cache_warming_economics(decision),
            decision.action.as_str()
        )
    };
    match status.state {
        CacheWarmingStatusState::Inactive => format!("Stopped ({details})"),
        CacheWarmingStatusState::Refreshing => format!("Warming cache ({details})"),
        CacheWarmingStatusState::Scheduled => format!(
            "{} ({details})",
            format_cache_warming_decision_time(status.next_warm_at, now)
        ),
    }
}

/// One-line transcript text for persisted cache-warming usage.
pub fn format_cache_warming_usage(entry: &UsageEntry) -> String {
    let note = entry
        .note
        .as_deref()
        .map(|n| format!(" ({n})"))
        .unwrap_or_default();
    // toFixed(6), then drop trailing zeros beyond the third decimal.
    let fixed = format!("{:.6}", entry.usage.cost.total);
    let cost = match fixed.find('.') {
        Some(dot) => {
            let keep = dot + 4;
            let trimmed_tail = fixed[keep..].trim_end_matches('0');
            format!("{}{}", &fixed[..keep], trimmed_tail)
        }
        None => fixed,
    };
    format!("Cache warmed{note}: ${cost}")
}
