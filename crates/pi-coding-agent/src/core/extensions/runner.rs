//! Port of `packages/coding-agent/src/core/extensions/runner.ts`.
//!
//! The runner owns the loaded extensions of one session runtime and dispatches events to their
//! handlers with the exact composition rules of the TypeScript runner:
//!
//! - notification events (`emit`): every handler runs, errors are reported and swallowed;
//!   `session_before_*` results are collected and a `cancel` short-circuits;
//! - in-place mutation (`tool_call` input, `before_provider_headers`, `before_agent_start`
//!   prompt options) is expressed with returned replacements that later handlers observe;
//! - chained transformations (`context`, `before_provider_request`, `input`, `message_end`,
//!   `tool_result`);
//! - gates (`tool_call` fails closed: handler errors propagate; `user_bash`);
//! - actionable boundaries (`turn_end`, `agent_before_settle`);
//! - last override wins (`cache_warming_decision`).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use futures::future::BoxFuture;
use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use pi_agent::types::{AgentMessage, AgentToolRef, ThinkingLevel};
use pi_ai::models::Provider;
use pi_ai::types::{ImageContent, Message, Model, ProviderHeaders, ToolResultMessage};
use pi_tui::keybindings::KeybindingsConfig;

use crate::core::cache_warmer::{CacheWarmingAction, CacheWarmingDecisionEvent, CacheWarmingDecisionEventResult};
use crate::core::diagnostics::{ResourceDiagnostic, ResourceDiagnosticType};
use crate::core::extensions::types::*;
use crate::core::model_registry::{ModelRegistry, ModelRegistryRegisterProviderProviderOrName};
use crate::core::model_resolver::ScopedModel;
use crate::core::provider_composer::ProviderConfigInput;
use crate::core::session_manager::{ReadonlySessionManager, SessionManager};
use crate::core::system_prompt::{
    BuildSystemPromptOptions, NormalizedBuildSystemPromptOptions, build_system_prompt,
    normalize_build_system_prompt_options,
};
use crate::core::virtual_models::VirtualModelDefinition;

// Extension shortcuts compete with canonical keybinding ids from keybindings.json.
// Only editor-global shortcuts are reserved here. Picker-specific bindings are not.
const RESERVED_KEYBINDINGS_FOR_EXTENSION_CONFLICTS: &[&str] = &[
    "app.interrupt",
    "app.clear",
    "app.exit",
    "app.suspend",
    "app.thinking.cycle",
    "app.model.cycleForward",
    "app.model.cycleBackward",
    "app.model.select",
    "app.tools.expand",
    "app.thinking.toggle",
    "app.editor.external",
    "app.message.copy",
    "app.message.followUp",
    "tui.input.submit",
    "tui.select.confirm",
    "tui.select.cancel",
    "tui.input.copy",
    "tui.editor.deleteToLineEnd",
];

/// Default stale message of `invalidate()`.
pub const STALE_EXTENSION_CONTEXT_MESSAGE: &str = "This extension ctx is stale after session replacement or reload. Do not use a captured pi or command ctx after ctx.newSession(), ctx.fork(), ctx.switchSession(), or ctx.reload(). For newSession, fork, and switchSession, move post-replacement work into withSession and use the ctx passed to withSession. For reload, do not use the old ctx after await ctx.reload().";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltInKeyBindingsValue {
    pub keybinding: String,
    pub restrict_override: bool,
}

pub type BuiltInKeyBindings = IndexMap<String, BuiltInKeyBindingsValue>;

fn build_builtin_keybindings(resolved_keybindings: &KeybindingsConfig) -> BuiltInKeyBindings {
    let mut builtin: BuiltInKeyBindings = IndexMap::new();
    for (keybinding, keys) in resolved_keybindings {
        let Some(keys) = keys else { continue };
        let restrict_override = RESERVED_KEYBINDINGS_FOR_EXTENSION_CONFLICTS.contains(&keybinding.as_str());
        for key in keys.to_vec() {
            let normalized_key = key.to_lowercase();
            // If multiple actions bind the same key, the reserved action wins so extensions
            // remain blocked by reserved shortcuts regardless of iteration order.
            if let Some(existing) = builtin.get(&normalized_key)
                && existing.restrict_override
                && !restrict_override
            {
                continue;
            }
            builtin.insert(
                normalized_key,
                BuiltInKeyBindingsValue { keybinding: keybinding.clone(), restrict_override },
            );
        }
    }
    builtin
}

/// The message that starts a run: the before_agent_start event without its prompt fields.
pub type BeforeAgentStartTrigger = BeforeAgentStartKind;

/// Message returned by a `before_agent_start` handler.
pub type BeforeAgentStartCombinedResultMessages = CustomMessageInput;

/// Combined result from all before_agent_start handlers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartCombinedResult {
    pub messages: Vec<BeforeAgentStartCombinedResultMessages>,
    pub system_prompt_options: NormalizedBuildSystemPromptOptions,
}

/// Events handled by the generic emit() method.
/// Events with dedicated emitXxx() methods should use those for the typed composition rules.
pub type RunnerEmitEvent = ExtensionEvent;

pub type SessionBeforeEvent = RunnerEmitEvent;

/// Result of `emit()` for a `session_before_*` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionBeforeEventResult {
    SessionBeforeSwitchResult(SessionBeforeSwitchResult),
    SessionBeforeForkResult(SessionBeforeForkResult),
    SessionBeforeCompactResult(SessionBeforeCompactResult),
    SessionBeforeTreeResult(SessionBeforeTreeResult),
}

impl SessionBeforeEventResult {
    /// Whether the handler cancelled the operation.
    pub fn cancel(&self) -> bool {
        match self {
            Self::SessionBeforeSwitchResult(r) => r.cancel == Some(true),
            Self::SessionBeforeForkResult(r) => r.cancel == Some(true),
            Self::SessionBeforeCompactResult(r) => r.cancel == Some(true),
            Self::SessionBeforeTreeResult(r) => r.cancel == Some(true),
        }
    }
}

pub type RunnerEmitResult = Option<SessionBeforeEventResult>;

pub type ExtensionErrorListener = Arc<dyn Fn(ExtensionError) + Send + Sync>;

/// An error with a JavaScript-style stack (e.g. thrown by a JS extension). The runner reports
/// `stack` alongside the message, like `err.stack` in TS.
#[derive(Debug, Clone)]
pub struct ExtensionErrorWithStack {
    pub message: String,
    pub stack: Option<String>,
}

impl std::fmt::Display for ExtensionErrorWithStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExtensionErrorWithStack {}

/// `(error.message, error.stack)` of an error thrown by a handler.
pub fn error_parts(err: &anyhow::Error) -> (String, Option<String>) {
    if let Some(e) = err.downcast_ref::<ExtensionErrorWithStack>() {
        return (e.message.clone(), e.stack.clone());
    }
    (err.to_string(), None)
}

/// A JS handler that threw after mutating shared event state returns the mutation together with
/// the error (`$error`), so the mutation is kept like in TS.
fn split_bridge_error(result: ExtensionEventResult) -> (ExtensionEventResult, Option<(String, Option<String>)>) {
    match result {
        ExtensionEventResult::Json(Value::Object(mut map)) if map.contains_key("$error") => {
            let error = map.remove("$error").unwrap_or(Value::Null);
            let message = error.get("message").and_then(Value::as_str).unwrap_or_default().to_string();
            let stack = error.get("stack").and_then(Value::as_str).map(str::to_string);
            (ExtensionEventResult::Json(Value::Object(map)), Some((message, stack)))
        }
        other => (other, None),
    }
}

/// Base of a boundary event: `turn_end` or `agent_before_settle` without `entries`, `continue`
/// and `context`.
#[derive(Debug, Clone)]
// Mirrors a TS union; boxing the large variants would add indirection to every
// construction and match across the crate for little memory saved.
#[allow(clippy::large_enum_variant)]
pub enum BoundaryBaseEvent {
    TurnEnd {
        turn_index: u64,
        message: AgentMessage,
        tool_results: Vec<ToolResultMessage>,
        message_entry_id: String,
        tool_result_entry_ids: Vec<String>,
        outcome: AgentActivityOutcome,
    },
    AgentBeforeSettle {
        outcome: AgentActivityOutcome,
    },
}

impl BoundaryBaseEvent {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::TurnEnd { .. } => "turn_end",
            Self::AgentBeforeSettle { .. } => "agent_before_settle",
        }
    }

    fn build(&self, entries: Vec<SessionBoundaryDraft>, continue_: bool, context: BoundaryContextPreview) -> ExtensionEvent {
        match self {
            Self::TurnEnd { turn_index, message, tool_results, message_entry_id, tool_result_entry_ids, outcome } => {
                ExtensionEvent::TurnEnd(TurnEndEvent {
                    state: BoundaryState { entries, continue_, context, outcome: *outcome },
                    turn_index: *turn_index,
                    message: message.clone(),
                    tool_results: tool_results.clone(),
                    message_entry_id: message_entry_id.clone(),
                    tool_result_entry_ids: tool_result_entry_ids.clone(),
                })
            }
            Self::AgentBeforeSettle { outcome } => ExtensionEvent::AgentBeforeSettle(AgentBeforeSettleEvent {
                state: BoundaryState { entries, continue_, context, outcome: *outcome },
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryDispatchResult {
    pub entries: Vec<SessionBoundaryDraft>,
    #[serde(rename = "continue")]
    pub r#continue: bool,
    pub context: BoundaryContextPreview,
    pub valid: bool,
}

/// Builds the context preview for boundary entries.
pub type BoundaryContextBuilder = Arc<
    dyn Fn(Vec<SessionBoundaryDraft>) -> BoxFuture<'static, anyhow::Result<BoundaryContextPreview>> + Send + Sync,
>;

#[derive(Clone)]
pub struct NewSessionHandlerOptions {
    pub parent_session: Option<String>,
    pub setup: Option<Arc<dyn Fn(Arc<SessionManager>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>>,
    pub with_session: Option<Arc<dyn Fn(ReplacedSessionContext) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionHandlerResult {
    pub cancelled: bool,
}

pub type NewSessionHandler = Arc<
    dyn Fn(Option<NewSessionHandlerOptions>) -> BoxFuture<'static, anyhow::Result<NewSessionHandlerResult>> + Send + Sync,
>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ForkHandlerOptionsPosition {
    #[serde(rename = "before")]
    Before,
    #[serde(rename = "at")]
    At,
}

impl ForkHandlerOptionsPosition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::At => "at",
        }
    }
}

#[derive(Clone)]
pub struct ForkHandlerOptions {
    pub position: Option<ForkHandlerOptionsPosition>,
    pub with_session: Option<Arc<dyn Fn(ReplacedSessionContext) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkHandlerResult {
    pub cancelled: bool,
}

pub type ForkHandler = Arc<
    dyn Fn(String, Option<ForkHandlerOptions>) -> BoxFuture<'static, anyhow::Result<ForkHandlerResult>> + Send + Sync,
>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigateTreeHandlerOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summarize: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace_instructions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigateTreeHandlerResult {
    pub cancelled: bool,
}

pub type NavigateTreeHandler = Arc<
    dyn Fn(String, Option<NavigateTreeHandlerOptions>) -> BoxFuture<'static, anyhow::Result<NavigateTreeHandlerResult>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct SwitchSessionHandlerOptions {
    pub with_session: Option<Arc<dyn Fn(ReplacedSessionContext) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchSessionHandlerResult {
    pub cancelled: bool,
}

pub type SwitchSessionHandler = Arc<
    dyn Fn(String, Option<SwitchSessionHandlerOptions>) -> BoxFuture<'static, anyhow::Result<SwitchSessionHandlerResult>>
        + Send
        + Sync,
>;

pub type ReloadHandler = Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;

pub type ShutdownHandler = Arc<dyn Fn() + Send + Sync>;

/// Helper function to emit session_shutdown event to extensions.
/// Returns true if the event was emitted, false if there were no handlers.
pub async fn emit_session_shutdown_event(
    extension_runner: Arc<ExtensionRunner>,
    event: &SessionShutdownEvent,
) -> anyhow::Result<bool> {
    if extension_runner.has_handlers("session_shutdown") {
        extension_runner.emit(ExtensionEvent::SessionShutdown(event.clone())).await?;
        return Ok(true);
    }
    Ok(false)
}

/// Snapshot of `(extension path, handlers)` for one event type, so handlers registered or removed
/// while dispatching do not affect the current dispatch.
fn snapshot_event_handlers(extensions: &[ExtensionRef], event: &str) -> Vec<(String, Vec<HandlerFn>)> {
    extensions
        .iter()
        .map(|ext| {
            let ext = ext.read();
            (ext.path.clone(), ext.handlers.get(event).cloned().unwrap_or_default())
        })
        .collect()
}

fn event_handlers(handlers: Vec<HandlerFn>) -> impl Iterator<Item = ExtensionHandler> {
    handlers.into_iter().filter_map(|h| match h {
        HandlerFn::Event(h) => Some(h),
        HandlerFn::ProjectTrust(_) => None,
    })
}

fn same_messages(left: &[AgentMessage], right: &[AgentMessage]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a == b)
}

/// Re-attach the prompt and tool state after a `context` handler. Handlers only see the
/// conversation; the system messages belong to Pi. An unchanged conversation keeps every
/// system message in place, so models with mid-conversation support keep their cached
/// prefix. A changed one gets the replayed prompt sections and tool declarations as one
/// leading system message, so pruning, windowing, or slicing from a compaction summary
/// cannot drop them.
fn restore_system_messages(
    current: Vec<AgentMessage>,
    visible: &[AgentMessage],
    returned: Vec<AgentMessage>,
) -> Vec<AgentMessage> {
    if same_messages(&returned, visible) {
        return current;
    }
    let system: Vec<Message> = current
        .iter()
        .filter_map(|m| match m {
            AgentMessage::System(s) => Some(Message::System(s.clone())),
            _ => None,
        })
        .collect();
    match pi_ai::utils::transcript::get_current_system_message(&system) {
        Some(head) => {
            let mut out = Vec::with_capacity(returned.len() + 1);
            out.push(AgentMessage::System(head));
            out.extend(returned);
            out
        }
        None => returned,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmitProjectTrustEventResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<ProjectTrustEventResult>,
    pub errors: Vec<ExtensionError>,
}

pub async fn emit_project_trust_event(
    extensions_result: &LoadExtensionsResult,
    event: &ProjectTrustEvent,
    ctx: &ProjectTrustContext,
) -> anyhow::Result<EmitProjectTrustEventResult> {
    let mut errors = Vec::new();
    for (path, handlers) in snapshot_event_handlers(&extensions_result.extensions, "project_trust") {
        // A single extension may register multiple handlers for the same event.
        // The first project_trust handler that returns yes/no wins; undecided falls through.
        for handler in handlers {
            let result = match handler {
                HandlerFn::ProjectTrust(h) => h(event.clone(), ctx.clone()).await,
                HandlerFn::Event(h) => {
                    let ctx_value = ExtensionContext::new(Arc::new(ProjectTrustContextHost(ctx.clone())));
                    match h(ExtensionEvent::ProjectTrust(event.clone()), ctx_value).await {
                        Ok(Some(r)) => ProjectTrustEventResult::from_result(r).and_then(|r| {
                            r.ok_or_else(|| anyhow::anyhow!("project_trust handler returned no result"))
                        }),
                        Ok(None) => Err(anyhow::anyhow!("project_trust handler returned no result")),
                        Err(e) => Err(e),
                    }
                }
            };
            match result {
                Ok(r) if r.trusted == ProjectTrustEventDecision::Undecided => continue,
                Ok(r) => return Ok(EmitProjectTrustEventResult { result: Some(r), errors }),
                Err(err) => {
                    let (error, stack) = error_parts(&err);
                    errors.push(ExtensionError { extension_path: path.clone(), event: "project_trust".into(), error, stack });
                }
            }
        }
    }
    Ok(EmitProjectTrustEventResult { result: None, errors })
}

/// Context host for `project_trust` handlers registered with the generic `on()` (the JS bridge):
/// only `ui`, `mode`, `hasUI` and `cwd` exist, like the TS `ProjectTrustContext`.
struct ProjectTrustContextHost(ProjectTrustContext);

impl ExtensionContextHost for ProjectTrustContextHost {
    fn assert_active(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn ui(&self) -> Arc<dyn ExtensionUIContext> {
        self.0.ui.clone()
    }
    fn mode(&self) -> ExtensionMode {
        self.0.mode
    }
    fn has_ui(&self) -> bool {
        self.0.has_ui
    }
    fn cwd(&self) -> String {
        self.0.cwd.clone()
    }
    fn session_manager(&self) -> Option<ReadonlySessionManager> {
        None
    }
    fn model_registry(&self) -> Option<Arc<ModelRegistry>> {
        None
    }
    fn thinking_level(&self) -> Option<ThinkingLevel> {
        None
    }
    fn actions(&self) -> Arc<dyn ExtensionContextActions> {
        Arc::new(DefaultContextActions { cwd: self.0.cwd.clone() })
    }
}

#[derive(Clone, Default)]
pub struct ExtensionRunnerBindCoreProviderActions {
    pub register_provider: Option<Arc<dyn Fn(String, ProviderConfig) -> anyhow::Result<()> + Send + Sync>>,
    pub register_native_provider: Option<Arc<dyn Fn(Arc<dyn Provider>) -> anyhow::Result<()> + Send + Sync>>,
    pub unregister_provider: Option<Arc<dyn Fn(String) + Send + Sync>>,
    pub register_virtual_model: Option<Arc<dyn Fn(VirtualModelDefinition) -> anyhow::Result<()> + Send + Sync>>,
    pub unregister_virtual_model: Option<Arc<dyn Fn(String, String) + Send + Sync>>,
}

/// Paths contributed by `resources_discover` handlers, with the contributing extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredResourcePath {
    pub path: String,
    pub extension_path: String,
}

pub type ExtensionRunnerEmitResourcesDiscoverResultSkillPaths = DiscoveredResourcePath;
pub type ExtensionRunnerEmitResourcesDiscoverResultPromptPaths = DiscoveredResourcePath;
pub type ExtensionRunnerEmitResourcesDiscoverResultThemePaths = DiscoveredResourcePath;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionRunnerEmitResourcesDiscoverResult {
    pub skill_paths: Vec<DiscoveredResourcePath>,
    pub prompt_paths: Vec<DiscoveredResourcePath>,
    pub theme_paths: Vec<DiscoveredResourcePath>,
}

/// Convert the extension-facing provider config into the model registry input.
/// (`ProviderConfigInput` is the extension `ProviderConfig` itself, as in TS.)
pub fn provider_config_input(config: &ProviderConfig) -> ProviderConfigInput {
    config.clone()
}

fn normalized_to_options(options: &NormalizedBuildSystemPromptOptions) -> BuildSystemPromptOptions {
    serde_json::to_value(options)
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(|| BuildSystemPromptOptions {
            custom_prompt: options.custom_prompt.clone(),
            force_system_prompt: options.force_system_prompt.clone(),
            selected_tools: Some(options.selected_tools.clone()),
            hidden_tools: Some(options.hidden_tools.clone()),
            tool_snippets: Some(options.tool_snippets.clone()),
            tool_guidelines: Some(options.tool_guidelines.clone()),
            prompt_guidelines: Some(options.prompt_guidelines.clone()),
            append_system_prompt: Some(options.append_system_prompt.clone()),
            sections: Some(options.sections.clone()),
            cwd: options.cwd.clone(),
            context_files: Some(options.context_files.clone()),
            skills: Some(options.skills.clone()),
        })
}

fn spawn_detached(fut: BoxFuture<'static, ()>) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(fut);
        }
        Err(_) => {
            std::thread::spawn(move || futures::executor::block_on(fut));
        }
    }
}

#[derive(Default)]
struct UiPromptState {
    depth: usize,
    active: Option<(UIPromptKind, Option<String>)>,
}

pub struct ExtensionRunner {
    weak: Weak<ExtensionRunner>,
    extensions: Vec<ExtensionRef>,
    runtime: Arc<ExtensionRuntime>,
    ui_context: RwLock<Arc<dyn ExtensionUIContext>>,
    has_ui: AtomicBool,
    mode: RwLock<ExtensionMode>,
    cwd: String,
    session_manager: Arc<SessionManager>,
    model_registry: Arc<ModelRegistry>,
    error_listeners: Mutex<Vec<(u64, ExtensionErrorListener)>>,
    next_listener_id: AtomicU64,
    context_actions: RwLock<Option<Arc<dyn ExtensionContextActions>>>,
    command_actions: RwLock<Option<Arc<dyn ExtensionCommandContextActions>>>,
    /// Registered MCP servers already reported as unhandled.
    reported_mcp_servers: Mutex<HashSet<String>>,
    shortcut_diagnostics: Mutex<Vec<ResourceDiagnostic>>,
    command_diagnostics: Mutex<Vec<ResourceDiagnostic>>,
    stale_message: Mutex<Option<String>>,
    ui_prompt: Mutex<UiPromptState>,
}

impl ExtensionRunner {
    pub fn new(
        extensions: &[ExtensionRef],
        runtime: Arc<ExtensionRuntime>,
        cwd: &str,
        session_manager: Arc<SessionManager>,
        model_registry: Arc<ModelRegistry>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak| Self {
            weak: weak.clone(),
            extensions: extensions.to_vec(),
            runtime,
            ui_context: RwLock::new(Arc::new(NoOpUIContext)),
            has_ui: AtomicBool::new(false),
            mode: RwLock::new(ExtensionMode::Print),
            cwd: cwd.to_string(),
            session_manager,
            model_registry,
            error_listeners: Mutex::new(Vec::new()),
            next_listener_id: AtomicU64::new(1),
            context_actions: RwLock::new(None),
            command_actions: RwLock::new(None),
            reported_mcp_servers: Mutex::new(HashSet::new()),
            shortcut_diagnostics: Mutex::new(Vec::new()),
            command_diagnostics: Mutex::new(Vec::new()),
            stale_message: Mutex::new(None),
            ui_prompt: Mutex::new(UiPromptState::default()),
        })
    }

    /// The shared extension runtime.
    pub fn runtime(&self) -> &Arc<ExtensionRuntime> {
        &self.runtime
    }

    /// The loaded extensions.
    pub fn extensions(&self) -> &[ExtensionRef] {
        &self.extensions
    }

    pub fn bind_core(
        &self,
        actions: Arc<dyn ExtensionActions>,
        context_actions: Arc<dyn ExtensionContextActions>,
        provider_actions: Option<&ExtensionRunnerBindCoreProviderActions>,
    ) {
        // Copy actions into the shared runtime (all extension APIs reference this)
        *self.runtime.actions.write() = actions;
        // Context actions (required)
        *self.context_actions.write() = Some(context_actions);

        // Servers registered from now on reach the extension that connects them right away. Servers
        // registered during loading are read on session_start.
        let weak = self.weak.clone();
        self.runtime.mcp_servers.set_change_listener(Some(Arc::new(move || {
            let Some(runner) = weak.upgrade() else { return };
            let servers = runner.runtime.mcp_servers.list();
            let r2 = runner.clone();
            spawn_detached(Box::pin(async move {
                let _ = r2.emit(ExtensionEvent::McpServersChange(McpServersChangeEvent { servers })).await;
            }));
            runner.report_unhandled_mcp_servers();
        })));

        let provider_actions = provider_actions.cloned().unwrap_or_default();
        let hooks = Arc::new(BoundRuntimeHooks {
            runner: self.weak.clone(),
            model_registry: self.model_registry.clone(),
            provider_actions,
        });

        // Flush provider registrations queued during extension loading
        let pending = std::mem::take(&mut *self.runtime.pending_provider_registrations.lock());
        for PendingProviderRegistration { name, config, extension_path } in pending {
            if let Err(err) = hooks.do_register_provider(&name, config) {
                let (error, stack) = error_parts(&err);
                self.emit_error(&ExtensionError { extension_path, event: "register_provider".into(), error, stack });
            }
        }
        let pending = std::mem::take(&mut *self.runtime.pending_native_provider_registrations.lock());
        for PendingNativeProviderRegistration { provider, extension_path } in pending {
            if let Err(err) = hooks.do_register_native_provider(provider) {
                let (error, stack) = error_parts(&err);
                self.emit_error(&ExtensionError { extension_path, event: "register_provider".into(), error, stack });
            }
        }
        let pending = std::mem::take(&mut *self.runtime.pending_virtual_model_registrations.lock());
        for PendingVirtualModelRegistration { definition, extension_path } in pending {
            if let Err(err) = hooks.do_register_virtual_model(definition) {
                let (error, stack) = error_parts(&err);
                self.emit_error(&ExtensionError {
                    extension_path,
                    event: "register_virtual_model".into(),
                    error,
                    stack,
                });
            }
        }

        // From this point on, provider registration/unregistration takes effect immediately
        // without requiring a /reload.
        *self.runtime.hooks.write() = hooks;
    }

    pub fn bind_command_context(&self, actions: Option<Arc<dyn ExtensionCommandContextActions>>) {
        *self.command_actions.write() = actions;
    }

    pub fn set_ui_context(&self, ui_context: Option<Arc<dyn ExtensionUIContext>>, mode: Option<ExtensionMode>) {
        match ui_context {
            Some(ui) => {
                *self.ui_context.write() = Arc::new(PromptTrackingUIContext { inner: ui, runner: self.weak.clone() });
                self.has_ui.store(true, Ordering::SeqCst);
            }
            None => {
                *self.ui_context.write() = Arc::new(NoOpUIContext);
                self.has_ui.store(false, Ordering::SeqCst);
            }
        }
        *self.mode.write() = mode.unwrap_or(ExtensionMode::Print);
    }

    fn ui_prompt_start(&self, kind: UIPromptKind, title: Option<&str>) {
        let outer = {
            let mut state = self.ui_prompt.lock();
            let outer = state.depth == 0;
            state.depth += 1;
            if outer {
                state.active = Some((kind, title.map(str::to_string)));
            }
            outer
        };
        if outer {
            self.emit_ui_prompt_event(ExtensionEvent::UIPromptStart(UIPromptStartEvent {
                reason: UIPromptReason::UiPrompt,
                kind,
                title: title.filter(|t| !t.is_empty()).map(str::to_string),
            }));
        }
    }

    fn ui_prompt_finish(&self, kind: UIPromptKind, title: Option<&str>) {
        let prompt = {
            let mut state = self.ui_prompt.lock();
            state.depth = state.depth.saturating_sub(1);
            if state.depth > 0 {
                return;
            }
            state.depth = 0;
            state.active.take().unwrap_or((kind, title.map(str::to_string)))
        };
        self.emit_ui_prompt_event(ExtensionEvent::UIPromptEnd(UIPromptEndEvent {
            reason: UIPromptReason::UiPrompt,
            kind: prompt.0,
            title: prompt.1.filter(|t| !t.is_empty()),
        }));
    }

    fn emit_ui_prompt_event(&self, event: ExtensionEvent) {
        let Some(runner) = self.weak.upgrade() else { return };
        spawn_detached(Box::pin(async move {
            let _ = runner.emit(event).await;
        }));
    }

    pub fn get_ui_context(&self) -> Arc<dyn ExtensionUIContext> {
        self.ui_context.read().clone()
    }

    pub fn has_ui(&self) -> bool {
        self.has_ui.load(Ordering::SeqCst)
    }

    pub fn mode(&self) -> ExtensionMode {
        *self.mode.read()
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn session_manager(&self) -> Arc<SessionManager> {
        self.session_manager.clone()
    }

    pub fn get_extension_paths(&self) -> Vec<String> {
        self.extensions.iter().map(|e| e.read().path.clone()).collect()
    }

    /// Get all registered tools from all extensions (first registration per name wins).
    pub fn get_all_registered_tools(&self) -> Vec<RegisteredTool> {
        let mut by_name: IndexMap<String, RegisteredTool> = IndexMap::new();
        for ext in &self.extensions {
            for tool in ext.read().tools.values() {
                if !by_name.contains_key(&tool.definition.name) {
                    by_name.insert(tool.definition.name.clone(), tool.clone());
                }
            }
        }
        by_name.into_values().collect()
    }

    /// Get a tool definition by name. Returns undefined if not found.
    pub fn get_tool_definition(&self, tool_name: &str) -> Option<ToolDefinition> {
        self.extensions.iter().find_map(|ext| ext.read().tools.get(tool_name).map(|t| t.definition.clone()))
    }

    pub fn get_flags(&self) -> IndexMap<String, ExtensionFlag> {
        let mut all: IndexMap<String, ExtensionFlag> = IndexMap::new();
        for ext in &self.extensions {
            for (name, flag) in &ext.read().flags {
                if !all.contains_key(name) {
                    all.insert(name.clone(), flag.clone());
                }
            }
        }
        all
    }

    pub fn set_flag_value(&self, name: &str, value: FlagValue) {
        self.runtime.flag_values.lock().insert(name.to_string(), value);
    }

    pub fn get_flag_values(&self) -> IndexMap<String, FlagValue> {
        self.runtime.flag_values.lock().clone()
    }

    pub fn get_shortcuts(&self, resolved_keybindings: &KeybindingsConfig) -> IndexMap<String, ExtensionShortcut> {
        self.shortcut_diagnostics.lock().clear();
        let builtin = build_builtin_keybindings(resolved_keybindings);
        let mut extension_shortcuts: IndexMap<String, ExtensionShortcut> = IndexMap::new();
        let has_ui = self.has_ui();
        let add_diagnostic = |message: String, extension_path: &str| {
            self.shortcut_diagnostics.lock().push(ResourceDiagnostic {
                r#type: ResourceDiagnosticType::Warning,
                message: message.clone(),
                path: Some(extension_path.to_string()),
                collision: None,
            });
            if !has_ui {
                eprintln!("{message}");
            }
        };

        for ext in &self.extensions {
            let shortcuts: Vec<(String, ExtensionShortcut)> =
                ext.read().shortcuts.iter().map(|(k, s)| (k.clone(), s.clone())).collect();
            for (key, shortcut) in shortcuts {
                let normalized_key = key.to_lowercase();
                let builtin_binding = builtin.get(&normalized_key);
                if builtin_binding.is_some_and(|b| b.restrict_override) {
                    add_diagnostic(
                        format!(
                            "Extension shortcut '{key}' from {} conflicts with built-in shortcut. Skipping.",
                            shortcut.extension_path
                        ),
                        &shortcut.extension_path,
                    );
                    continue;
                }
                if let Some(b) = builtin_binding
                    && !b.restrict_override
                {
                    add_diagnostic(
                        format!(
                            "Extension shortcut conflict: '{key}' is built-in shortcut for {} and {}. Using {}.",
                            b.keybinding, shortcut.extension_path, shortcut.extension_path
                        ),
                        &shortcut.extension_path,
                    );
                }
                if let Some(existing) = extension_shortcuts.get(&normalized_key) {
                    add_diagnostic(
                        format!(
                            "Extension shortcut conflict: '{key}' registered by both {} and {}. Using {}.",
                            existing.extension_path, shortcut.extension_path, shortcut.extension_path
                        ),
                        &shortcut.extension_path,
                    );
                }
                extension_shortcuts.insert(normalized_key, shortcut);
            }
        }
        extension_shortcuts
    }

    pub fn get_shortcut_diagnostics(&self) -> Vec<ResourceDiagnostic> {
        self.shortcut_diagnostics.lock().clone()
    }

    pub fn invalidate(&self, message: Option<&str>) {
        let message = message.unwrap_or(STALE_EXTENSION_CONTEXT_MESSAGE).to_string();
        {
            let mut stale = self.stale_message.lock();
            if stale.is_some() {
                return;
            }
            *stale = Some(message.clone());
        }
        self.runtime.invalidate(Some(&message));
    }

    fn assert_active(&self) -> anyhow::Result<()> {
        match &*self.stale_message.lock() {
            Some(message) => Err(anyhow::anyhow!("{message}")),
            None => Ok(()),
        }
    }

    pub fn on_error(&self, listener: ExtensionErrorListener) -> Arc<dyn Fn() + Send + Sync> {
        let id = self.next_listener_id.fetch_add(1, Ordering::SeqCst);
        self.error_listeners.lock().push((id, listener));
        let weak = self.weak.clone();
        Arc::new(move || {
            if let Some(runner) = weak.upgrade() {
                runner.error_listeners.lock().retain(|(lid, _)| *lid != id);
            }
        })
    }

    pub fn emit_error(&self, error: &ExtensionError) {
        let listeners: Vec<ExtensionErrorListener> = self.error_listeners.lock().iter().map(|(_, l)| l.clone()).collect();
        for listener in listeners {
            listener(error.clone());
        }
    }

    fn report_handler_error(&self, extension_path: &str, event: &str, err: &anyhow::Error) {
        let (error, stack) = error_parts(err);
        self.emit_error(&ExtensionError { extension_path: extension_path.to_string(), event: event.to_string(), error, stack });
    }

    /// Report registered MCP servers when no extension handles `mcp_servers_change`, which means
    /// nothing connects them (for example when another MCP extension replaced the built-in one).
    pub fn report_unhandled_mcp_servers(&self) {
        if self.has_handlers("mcp_servers_change") {
            return;
        }
        for server in self.runtime.mcp_servers.list() {
            if !self.reported_mcp_servers.lock().insert(server.name.clone()) {
                continue;
            }
            self.emit_error(&ExtensionError {
                extension_path: server.extension_path.clone(),
                event: "register_mcp_server".into(),
                error: format!(
                    "MCP server \"{}\" is registered, but no loaded extension connects MCP servers; another extension may have replaced the built-in MCP support",
                    server.name
                ),
                stack: None,
            });
        }
    }

    pub fn has_handlers(&self, event_type: &str) -> bool {
        self.extensions
            .iter()
            .any(|ext| ext.read().handlers.get(event_type).is_some_and(|h| !h.is_empty()))
    }

    pub fn get_message_renderer(&self, custom_type: &str) -> Option<MessageRenderer> {
        self.extensions.iter().find_map(|ext| ext.read().message_renderers.get(custom_type).cloned())
    }

    pub fn get_markdown_transformers(&self) -> Vec<MarkdownTransformer> {
        self.extensions.iter().filter_map(|ext| ext.read().markdown_transformer.clone()).collect()
    }

    /// Renderers of calls to `toolName`: extension resolvers in load order, then `base`.
    pub fn resolve_tool_renderers(
        &self,
        tool_name: &str,
        base: Arc<dyn Fn() -> Option<ToolRenderers> + Send + Sync>,
    ) -> Option<ToolRenderers> {
        let resolvers: Vec<ToolRendererResolver> =
            self.extensions.iter().flat_map(|ext| ext.read().tool_renderers.clone()).collect();
        fn resolve(
            resolvers: &[ToolRendererResolver],
            index: usize,
            tool_name: &str,
            base: &Arc<dyn Fn() -> Option<ToolRenderers> + Send + Sync>,
        ) -> Option<ToolRenderers> {
            if index < resolvers.len() {
                let next = || resolve(resolvers, index + 1, tool_name, base);
                resolvers[index](tool_name, &next)
            } else {
                base()
            }
        }
        resolve(&resolvers, 0, tool_name, &base)
    }

    pub fn get_entry_renderer(&self, custom_type: &str) -> Option<EntryRenderer> {
        self.extensions.iter().find_map(|ext| ext.read().entry_renderers.get(custom_type).cloned())
    }

    fn resolve_registered_commands(&self) -> Vec<ResolvedCommand> {
        let mut commands: Vec<RegisteredCommand> = Vec::new();
        let mut counts: IndexMap<String, usize> = IndexMap::new();
        for ext in &self.extensions {
            for command in ext.read().commands.values() {
                commands.push(command.clone());
                *counts.entry(command.name.clone()).or_insert(0) += 1;
            }
        }
        let mut seen: IndexMap<String, usize> = IndexMap::new();
        let mut taken: HashSet<String> = HashSet::new();
        commands
            .into_iter()
            .map(|command| {
                let occurrence = seen.get(&command.name).copied().unwrap_or(0) + 1;
                seen.insert(command.name.clone(), occurrence);
                let mut invocation_name = if counts.get(&command.name).copied().unwrap_or(0) > 1 {
                    format!("{}:{occurrence}", command.name)
                } else {
                    command.name.clone()
                };
                if taken.contains(&invocation_name) {
                    let mut suffix = occurrence;
                    loop {
                        suffix += 1;
                        invocation_name = format!("{}:{suffix}", command.name);
                        if !taken.contains(&invocation_name) {
                            break;
                        }
                    }
                }
                taken.insert(invocation_name.clone());
                ResolvedCommand { command, invocation_name }
            })
            .collect()
    }

    pub fn get_model_registry(&self) -> Arc<ModelRegistry> {
        self.model_registry.clone()
    }

    pub fn get_registered_commands(&self) -> Vec<ResolvedCommand> {
        self.command_diagnostics.lock().clear();
        self.resolve_registered_commands()
    }

    pub fn get_command_diagnostics(&self) -> Vec<ResourceDiagnostic> {
        self.command_diagnostics.lock().clone()
    }

    pub fn get_command(&self, name: &str) -> Option<ResolvedCommand> {
        self.resolve_registered_commands().into_iter().find(|c| c.invocation_name == name)
    }

    /// Request a graceful shutdown. Called by extension tools and event handlers.
    /// The actual shutdown behavior is provided by the mode via bindExtensions().
    pub fn shutdown(&self) {
        self.context_actions().shutdown();
    }

    pub fn get_active_tools(&self) -> anyhow::Result<Vec<String>> {
        self.assert_active()?;
        self.runtime.actions().get_active_tools()
    }

    fn context_actions(&self) -> Arc<dyn ExtensionContextActions> {
        match &*self.context_actions.read() {
            Some(actions) => Arc::new(RunnerContextActions { inner: actions.clone(), cwd: self.cwd.clone() }),
            None => Arc::new(DefaultContextActions { cwd: self.cwd.clone() }),
        }
    }

    /// Create an ExtensionContext for use in event handlers and tool execution.
    /// Context values are resolved at call time, so changes via bindCore/bindUI are reflected.
    pub fn create_context(&self) -> ExtensionContext {
        ExtensionContext::new(Arc::new(RunnerContextHost { runner: self.weak.clone(), system_prompt_override: None }))
    }

    /// Create the context for executing the tool call `toolCallId`: the extension context plus
    /// `tools` and `executeTool()`. `signal` is the default signal of nested calls.
    pub fn create_tool_context(&self, tool_call_id: &str, signal: Option<CancellationToken>) -> ExtensionToolContext {
        ExtensionToolContext { ctx: self.create_context(), tool_call_id: tool_call_id.to_string(), signal }
    }

    pub fn create_command_context(&self) -> ExtensionCommandContext {
        ExtensionCommandContext {
            ctx: self.create_context(),
            command_actions: Arc::new(RunnerCommandActions { runner: self.weak.clone() }),
        }
    }

    pub async fn emit_boundary(
        &self,
        base_event: &BoundaryBaseEvent,
        build_context: BoundaryContextBuilder,
    ) -> anyhow::Result<BoundaryDispatchResult> {
        let ctx = self.create_context();
        let mut entries: Vec<SessionBoundaryDraft> = Vec::new();
        let mut should_continue = false;
        let mut context = build_context(entries.clone()).await?;
        let mut valid = true;

        for (path, handlers) in snapshot_event_handlers(&self.extensions, base_event.type_name()) {
            for handler in event_handlers(handlers) {
                let event = base_event.build(entries.clone(), should_continue, context.clone());
                match handler(event, ctx.clone()).await.and_then(|r| match r {
                    Some(r) => {
                        let (r, error) = split_bridge_error(r);
                        if let Some((message, stack)) = error {
                            self.emit_error(&ExtensionError {
                                extension_path: path.clone(),
                                event: base_event.type_name().into(),
                                error: message,
                                stack,
                            });
                        }
                        BoundaryResult::from_result(r)
                    }
                    None => Ok(None),
                }) {
                    Ok(Some(result)) => {
                        if let Some(e) = result.entries {
                            entries = e;
                        }
                        if let Some(c) = result.continue_ {
                            should_continue = c;
                        }
                    }
                    Ok(None) => {}
                    Err(err) => self.report_handler_error(&path, base_event.type_name(), &err),
                }

                match build_context(entries.clone()).await {
                    Ok(c) => {
                        context = c;
                        valid = true;
                    }
                    Err(err) => {
                        valid = false;
                        let (error, stack) = error_parts(&err);
                        self.emit_error(&ExtensionError {
                            extension_path: path.clone(),
                            event: base_event.type_name().into(),
                            error: format!("Invalid boundary entries: {error}"),
                            stack,
                        });
                    }
                }
            }
        }

        Ok(if valid {
            BoundaryDispatchResult { entries, r#continue: should_continue, context, valid: true }
        } else {
            BoundaryDispatchResult { entries: Vec::new(), r#continue: false, context, valid: false }
        })
    }

    fn session_before_result(event_type: &str, result: ExtensionEventResult) -> anyhow::Result<Option<SessionBeforeEventResult>> {
        Ok(match event_type {
            "session_before_switch" => SessionBeforeSwitchResult::from_result(result)?.map(SessionBeforeEventResult::SessionBeforeSwitchResult),
            "session_before_fork" => SessionBeforeForkResult::from_result(result)?.map(SessionBeforeEventResult::SessionBeforeForkResult),
            "session_before_compact" => {
                SessionBeforeCompactResult::from_result(result)?.map(SessionBeforeEventResult::SessionBeforeCompactResult)
            }
            "session_before_tree" => SessionBeforeTreeResult::from_result(result)?.map(SessionBeforeEventResult::SessionBeforeTreeResult),
            _ => None,
        })
    }

    pub async fn emit(&self, event: RunnerEmitEvent) -> anyhow::Result<RunnerEmitResult> {
        let ctx = self.create_context();
        let event_type = event.type_name();
        let is_session_before = matches!(
            event_type,
            "session_before_switch" | "session_before_fork" | "session_before_compact" | "session_before_tree"
        );
        let mut result: Option<SessionBeforeEventResult> = None;

        for (path, handlers) in snapshot_event_handlers(&self.extensions, event_type) {
            for handler in event_handlers(handlers) {
                let outcome = handler(event.clone(), ctx.clone()).await.and_then(|r| match r {
                    Some(r) if is_session_before => Self::session_before_result(event_type, r),
                    _ => Ok(None),
                });
                match outcome {
                    Ok(Some(r)) => {
                        let cancel = r.cancel();
                        result = Some(r);
                        if cancel {
                            return Ok(result);
                        }
                    }
                    Ok(None) => {}
                    Err(err) => self.report_handler_error(&path, event_type, &err),
                }
            }
        }
        Ok(result)
    }

    /// Returns the event's own action unless a handler overrides it; the last override wins.
    pub async fn emit_cache_warming_decision(&self, event: &CacheWarmingDecisionEvent) -> anyhow::Result<CacheWarmingAction> {
        let ctx = self.create_context();
        let mut action = event.action;
        for (path, handlers) in snapshot_event_handlers(&self.extensions, "cache_warming_decision") {
            for handler in event_handlers(handlers) {
                let outcome = handler(ExtensionEvent::CacheWarmingDecision(event.clone()), ctx.clone())
                    .await
                    .and_then(|r| r.map_or(Ok(None), CacheWarmingDecisionEventResult::from_result));
                match outcome {
                    Ok(Some(r)) => {
                        if let Some(a) = r.action {
                            action = a;
                        }
                    }
                    Ok(None) => {}
                    Err(err) => self.report_handler_error(&path, "cache_warming_decision", &err),
                }
            }
        }
        Ok(action)
    }

    pub async fn emit_message_end(&self, event: &MessageEndEvent) -> anyhow::Result<Option<AgentMessage>> {
        let ctx = self.create_context();
        let mut current = event.message.clone();
        let mut modified = false;
        for (path, handlers) in snapshot_event_handlers(&self.extensions, "message_end") {
            for handler in event_handlers(handlers) {
                let current_event = MessageEndEvent { message: current.clone() };
                let outcome = handler(ExtensionEvent::MessageEnd(current_event), ctx.clone())
                    .await
                    .and_then(|r| r.map_or(Ok(None), MessageEndEventResult::from_result));
                match outcome {
                    Ok(Some(MessageEndEventResult { message: Some(message) })) => {
                        if message.role() != current.role() {
                            self.emit_error(&ExtensionError {
                                extension_path: path.clone(),
                                event: "message_end".into(),
                                error: "message_end handlers must return a message with the same role".into(),
                                stack: None,
                            });
                            continue;
                        }
                        current = message;
                        modified = true;
                    }
                    Ok(_) => {}
                    Err(err) => self.report_handler_error(&path, "message_end", &err),
                }
            }
        }
        Ok(modified.then_some(current))
    }

    pub async fn emit_tool_result(&self, event: &ToolResultEvent) -> anyhow::Result<Option<ToolResultEventResult>> {
        let ctx = self.create_context();
        let mut current = event.clone();
        let mut modified = false;
        for (path, handlers) in snapshot_event_handlers(&self.extensions, "tool_result") {
            for handler in event_handlers(handlers) {
                let outcome = handler(ExtensionEvent::ToolResult(current.clone()), ctx.clone())
                    .await
                    .and_then(|r| r.map_or(Ok(None), ToolResultEventResult::from_result));
                match outcome {
                    Ok(Some(r)) => {
                        if let Some(content) = r.content {
                            current.content = content;
                            // Structured content that is not replaced along with the content may no longer match it.
                            if r.structured_content.is_none() {
                                current.structured_content = None;
                            }
                            modified = true;
                        }
                        if let Some(details) = r.details {
                            current.details = Some(details);
                            modified = true;
                        }
                        if let Some(sc) = r.structured_content {
                            current.structured_content = Some(sc);
                            modified = true;
                        }
                        if let Some(is_error) = r.is_error {
                            current.is_error = is_error;
                            modified = true;
                        }
                        if let Some(usage) = r.usage {
                            current.usage = Some(usage);
                            modified = true;
                        }
                    }
                    Ok(None) => {}
                    Err(err) => self.report_handler_error(&path, "tool_result", &err),
                }
            }
        }
        if !modified {
            return Ok(None);
        }
        Ok(Some(ToolResultEventResult {
            content: Some(current.content),
            details: current.details,
            structured_content: current.structured_content,
            is_error: Some(current.is_error),
            usage: current.usage,
        }))
    }

    /// Gate before a tool runs. Handler errors propagate (fail closed). `input` replacements
    /// (TS: in-place mutation of `event.input`) are seen by later handlers and returned in
    /// the result's `input` when the arguments changed.
    pub async fn emit_tool_call(&self, event: &ToolCallEvent) -> anyhow::Result<Option<ToolCallEventResult>> {
        let ctx = self.create_context();
        let mut current = event.clone();
        let mut input_changed = false;
        let mut result: Option<ToolCallEventResult> = None;

        for (_path, handlers) in snapshot_event_handlers(&self.extensions, "tool_call") {
            for handler in event_handlers(handlers) {
                let Some(handler_result) = handler(ExtensionEvent::ToolCall(current.clone()), ctx.clone()).await? else {
                    continue;
                };
                let Some(mut r) = ToolCallEventResult::from_result(handler_result)? else { continue };
                if let Some(input) = r.input.take()
                    && input != current.input
                {
                    current.input = input;
                    input_changed = true;
                }
                // An input-only result (the bridge's form of an in-place mutation without a
                // return value) does not replace an earlier handler's result.
                if r.block.is_some() || r.reason.is_some() || r.terminate.is_some() {
                    let block = r.block == Some(true);
                    result = Some(r);
                    if block {
                        let mut r = result.take().unwrap_or_default();
                        if input_changed {
                            r.input = Some(current.input.clone());
                        }
                        return Ok(Some(r));
                    }
                }
            }
        }
        if input_changed {
            let mut r = result.unwrap_or_default();
            r.input = Some(current.input);
            return Ok(Some(r));
        }
        Ok(result)
    }

    pub async fn emit_user_bash(&self, event: &UserBashEvent) -> anyhow::Result<Option<UserBashEventResult>> {
        let ctx = self.create_context();
        for (path, handlers) in snapshot_event_handlers(&self.extensions, "user_bash") {
            for handler in event_handlers(handlers) {
                let outcome = async {
                    let r = handler(ExtensionEvent::UserBash(event.clone()), ctx.clone()).await?;
                    match r {
                        None | Some(ExtensionEventResult::Json(Value::Null)) => Ok(None),
                        Some(ExtensionEventResult::UserBash(r)) => Ok(Some(r)),
                        Some(_) => Err(anyhow::anyhow!(
                            "Invalid user_bash handler result: return undefined for local execution or exactly one valid {{ operations }} or {{ result }} object"
                        )),
                    }
                }
                .await;
                match outcome {
                    Ok(None) => continue,
                    Ok(Some(r)) => return Ok(Some(r)),
                    Err(err) => {
                        self.report_handler_error(&path, "user_bash", &err);
                        return Err(err);
                    }
                }
            }
        }
        Ok(None)
    }

    /// Run the request-time transforms in two phases. `context` handlers see the conversation
    /// only and Pi restores the prompt and tool state after each; `context_with_system`
    /// handlers then see the full transcript and their output is used as returned.
    pub async fn emit_context(&self, messages: &[AgentMessage]) -> anyhow::Result<Vec<AgentMessage>> {
        let ctx = self.create_context();
        let mut current: Vec<AgentMessage> = messages.to_vec();

        for (path, handlers) in snapshot_event_handlers(&self.extensions, "context") {
            for handler in event_handlers(handlers) {
                let visible: Vec<AgentMessage> =
                    current.iter().filter(|m| m.role() != "system").cloned().collect();
                let event = ExtensionEvent::Context(ContextEvent { messages: visible.clone() });
                let outcome = handler(event, ctx.clone())
                    .await
                    .and_then(|r| r.map_or(Ok(None), ContextEventResult::from_result));
                match outcome {
                    Ok(Some(ContextEventResult { messages: Some(returned) })) => {
                        current = restore_system_messages(current, &visible, returned);
                    }
                    Ok(_) => {}
                    Err(err) => self.report_handler_error(&path, "context", &err),
                }
            }
        }

        for (path, handlers) in snapshot_event_handlers(&self.extensions, "context_with_system") {
            for handler in event_handlers(handlers) {
                let had_leading_system = current.first().is_some_and(|m| m.role() == "system");
                let event = ExtensionEvent::ContextWithSystem(ContextWithSystemEvent { messages: current.clone() });
                let outcome = handler(event, ctx.clone())
                    .await
                    .and_then(|r| r.map_or(Ok(None), ContextEventResult::from_result));
                match outcome {
                    Ok(r) => {
                        if let Some(ContextEventResult { messages: Some(m) }) = r {
                            current = m;
                        }
                        // Providers read the prompt and initial tools from the leading system message.
                        // Losing it is never intended; report it but honor the handler's output.
                        if had_leading_system && !current.first().is_some_and(|m| m.role() == "system") {
                            self.emit_error(&ExtensionError {
                                extension_path: path.clone(),
                                event: "context_with_system".into(),
                                error: "Handler removed the leading system message; the request has no prompt or initial tool declarations. Keep it at index 0 or replace a dropped prefix with getCurrentSystemMessage().".into(),
                                stack: None,
                            });
                        }
                    }
                    Err(err) => self.report_handler_error(&path, "context_with_system", &err),
                }
            }
        }
        Ok(current)
    }

    pub async fn emit_before_provider_request(&self, payload: Value) -> anyhow::Result<Value> {
        let ctx = self.create_context();
        let mut current = payload;
        for (path, handlers) in snapshot_event_handlers(&self.extensions, "before_provider_request") {
            for handler in event_handlers(handlers) {
                let event = ExtensionEvent::BeforeProviderRequest(BeforeProviderRequestEvent { payload: current.clone() });
                match handler(event, ctx.clone()).await {
                    Ok(Some(ExtensionEventResult::BeforeProviderRequest(v))) => current = v,
                    Ok(Some(ExtensionEventResult::Json(Value::Null))) | Ok(None) => {}
                    Ok(Some(ExtensionEventResult::Json(v))) => current = v,
                    Ok(Some(_)) => self.report_handler_error(
                        &path,
                        "before_provider_request",
                        &anyhow::anyhow!("handler returned a result that is not a provider payload"),
                    ),
                    Err(err) => self.report_handler_error(&path, "before_provider_request", &err),
                }
            }
        }
        Ok(current)
    }

    /// Handlers return the complete header map after their changes (TS: in-place mutation);
    /// later handlers observe earlier changes.
    pub async fn emit_before_provider_headers(&self, headers: ProviderHeaders) -> anyhow::Result<ProviderHeaders> {
        let ctx = self.create_context();
        let mut current = headers;
        for (path, handlers) in snapshot_event_handlers(&self.extensions, "before_provider_headers") {
            for handler in event_handlers(handlers) {
                let event = ExtensionEvent::BeforeProviderHeaders(BeforeProviderHeadersEvent { headers: current.clone() });
                let outcome = handler(event, ctx.clone())
                    .await
                    .and_then(|r| r.map_or(Ok(None), BeforeProviderHeadersEventResult::from_result));
                match outcome {
                    Ok(Some(r)) => current = r.headers,
                    Ok(None) => {}
                    Err(err) => self.report_handler_error(&path, "before_provider_headers", &err),
                }
            }
        }
        Ok(current)
    }

    pub async fn emit_before_agent_start(
        &self,
        trigger: &BeforeAgentStartTrigger,
        system_prompt_options: &BuildSystemPromptOptions,
    ) -> anyhow::Result<BeforeAgentStartCombinedResult> {
        let current = Arc::new(Mutex::new(normalize_build_system_prompt_options(system_prompt_options)));
        let render: Arc<dyn Fn() -> String + Send + Sync> = {
            let current = current.clone();
            Arc::new(move || build_system_prompt(&normalized_to_options(&current.lock())).unwrap_or_default())
        };
        let ctx = ExtensionContext::new(Arc::new(RunnerContextHost {
            runner: self.weak.clone(),
            system_prompt_override: Some(render.clone()),
        }));
        let mut messages = Vec::new();

        for (path, handlers) in snapshot_event_handlers(&self.extensions, "before_agent_start") {
            for handler in event_handlers(handlers) {
                let event = ExtensionEvent::BeforeAgentStart(BeforeAgentStartEvent {
                    system_prompt: render(),
                    system_prompt_options: current.lock().clone(),
                    kind: trigger.clone(),
                });
                let outcome = handler(event, ctx.clone())
                    .await
                    .and_then(|r| r.map_or(Ok(None), BeforeAgentStartEventResult::from_result));
                match outcome {
                    Ok(Some(r)) => {
                        if let Some(options) = r.system_prompt_options {
                            *current.lock() = options;
                        }
                        if let Some(message) = r.message {
                            messages.push(message);
                        }
                        if let Some(prompt) = r.system_prompt {
                            current.lock().force_system_prompt = Some(prompt);
                        }
                    }
                    Ok(None) => {}
                    Err(err) => self.report_handler_error(&path, "before_agent_start", &err),
                }
            }
        }
        let system_prompt_options = current.lock().clone();
        Ok(BeforeAgentStartCombinedResult { messages, system_prompt_options })
    }

    pub async fn emit_resources_discover(
        &self,
        cwd: &str,
        reason: ResourcesDiscoverReason,
    ) -> anyhow::Result<ExtensionRunnerEmitResourcesDiscoverResult> {
        let ctx = self.create_context();
        let mut out = ExtensionRunnerEmitResourcesDiscoverResult::default();
        for (path, handlers) in snapshot_event_handlers(&self.extensions, "resources_discover") {
            for handler in event_handlers(handlers) {
                let event = ExtensionEvent::ResourcesDiscover(ResourcesDiscoverEvent { cwd: cwd.to_string(), reason });
                let outcome = handler(event, ctx.clone())
                    .await
                    .and_then(|r| r.map_or(Ok(None), ResourcesDiscoverResult::from_result));
                match outcome {
                    Ok(Some(r)) => {
                        let tag = |paths: Option<Vec<String>>| -> Vec<DiscoveredResourcePath> {
                            paths
                                .unwrap_or_default()
                                .into_iter()
                                .map(|p| DiscoveredResourcePath { path: p, extension_path: path.clone() })
                                .collect()
                        };
                        out.skill_paths.extend(tag(r.skill_paths));
                        out.prompt_paths.extend(tag(r.prompt_paths));
                        out.theme_paths.extend(tag(r.theme_paths));
                    }
                    Ok(None) => {}
                    Err(err) => self.report_handler_error(&path, "resources_discover", &err),
                }
            }
        }
        Ok(out)
    }

    /// Emit input event. Transforms chain, "handled" short-circuits.
    pub async fn emit_input(
        &self,
        text: &str,
        images: Option<Vec<ImageContent>>,
        source: InputSource,
        streaming_behavior: Option<StreamingBehavior>,
    ) -> anyhow::Result<InputEventResult> {
        let ctx = self.create_context();
        let mut current_text = text.to_string();
        let mut current_images = images;
        let mut images_replaced = false;

        for (path, handlers) in snapshot_event_handlers(&self.extensions, "input") {
            for handler in event_handlers(handlers) {
                let event = ExtensionEvent::Input(InputEvent {
                    text: current_text.clone(),
                    images: current_images.clone(),
                    source,
                    streaming_behavior,
                });
                let outcome = handler(event, ctx.clone())
                    .await
                    .and_then(|r| r.map_or(Ok(None), InputEventResult::from_result));
                match outcome {
                    Ok(Some(InputEventResult::Handled)) => return Ok(InputEventResult::Handled),
                    Ok(Some(InputEventResult::Transform { text, images })) => {
                        current_text = text;
                        if let Some(images) = images {
                            current_images = Some(images);
                            images_replaced = true;
                        }
                    }
                    Ok(_) => {}
                    Err(err) => self.report_handler_error(&path, "input", &err),
                }
            }
        }
        Ok(if current_text != text || images_replaced {
            InputEventResult::Transform { text: current_text, images: current_images }
        } else {
            InputEventResult::Continue
        })
    }
}

// ============================================================================
// Context host and action adapters
// ============================================================================

/// Backend of the contexts created by the runner. Holds the runner weakly so contexts captured
/// by extensions do not keep a replaced runtime alive.
struct RunnerContextHost {
    runner: Weak<ExtensionRunner>,
    /// `getSystemPrompt()` of `before_agent_start` contexts renders the options being edited.
    system_prompt_override: Option<Arc<dyn Fn() -> String + Send + Sync>>,
}

const RUNNER_GONE: &str = "This extension ctx is no longer available: its extension runtime was shut down.";

impl RunnerContextHost {
    fn runner(&self) -> Option<Arc<ExtensionRunner>> {
        self.runner.upgrade()
    }
}

impl ExtensionContextHost for RunnerContextHost {
    fn assert_active(&self) -> anyhow::Result<()> {
        match self.runner() {
            Some(r) => r.assert_active(),
            None => Err(anyhow::anyhow!(RUNNER_GONE)),
        }
    }
    fn ui(&self) -> Arc<dyn ExtensionUIContext> {
        self.runner().map_or_else(|| Arc::new(NoOpUIContext) as Arc<dyn ExtensionUIContext>, |r| r.get_ui_context())
    }
    fn mode(&self) -> ExtensionMode {
        self.runner().map_or(ExtensionMode::Print, |r| r.mode())
    }
    fn has_ui(&self) -> bool {
        self.runner().is_some_and(|r| r.has_ui())
    }
    fn cwd(&self) -> String {
        self.runner().map(|r| r.cwd.clone()).unwrap_or_default()
    }
    fn session_manager(&self) -> Option<ReadonlySessionManager> {
        self.runner().map(|r| r.session_manager.clone())
    }
    fn model_registry(&self) -> Option<Arc<ModelRegistry>> {
        self.runner().map(|r| r.model_registry.clone())
    }
    fn thinking_level(&self) -> Option<ThinkingLevel> {
        self.runner().and_then(|r| r.runtime.actions().get_thinking_level().ok())
    }
    fn actions(&self) -> Arc<dyn ExtensionContextActions> {
        let actions = match self.runner() {
            Some(r) => r.context_actions(),
            None => Arc::new(DefaultContextActions { cwd: String::new() }),
        };
        match &self.system_prompt_override {
            Some(render) => Arc::new(SystemPromptOverrideActions { inner: actions, render: render.clone() }),
            None => actions,
        }
    }
}

/// A context whose every accessor fails with `message` (default: the runner is gone).
pub fn stale_extension_context(message: Option<&str>) -> ExtensionContext {
    ExtensionContext::new(Arc::new(StaleContextHost(message.unwrap_or(RUNNER_GONE).to_string())))
}

struct StaleContextHost(String);

impl ExtensionContextHost for StaleContextHost {
    fn assert_active(&self) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("{}", self.0))
    }
    fn ui(&self) -> Arc<dyn ExtensionUIContext> {
        Arc::new(NoOpUIContext)
    }
    fn mode(&self) -> ExtensionMode {
        ExtensionMode::Print
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn cwd(&self) -> String {
        String::new()
    }
    fn session_manager(&self) -> Option<ReadonlySessionManager> {
        None
    }
    fn model_registry(&self) -> Option<Arc<ModelRegistry>> {
        None
    }
    fn thinking_level(&self) -> Option<ThinkingLevel> {
        None
    }
    fn actions(&self) -> Arc<dyn ExtensionContextActions> {
        Arc::new(DefaultContextActions { cwd: String::new() })
    }
}

/// TS defaults of the runner's context functions before `bindCore()`.
struct DefaultContextActions {
    cwd: String,
}

impl ExtensionContextActions for DefaultContextActions {
    fn get_model(&self) -> Option<Model> {
        None
    }
    fn get_scoped_models(&self) -> Vec<ScopedModel> {
        Vec::new()
    }
    fn is_idle(&self) -> bool {
        true
    }
    fn is_project_trusted(&self) -> bool {
        true
    }
    fn get_signal(&self) -> Option<CancellationToken> {
        None
    }
    fn abort(&self) {}
    fn has_pending_messages(&self) -> bool {
        false
    }
    fn shutdown(&self) {}
    fn get_context_usage(&self) -> Option<ContextUsage> {
        None
    }
    fn compact(&self, _options: CompactOptions) {}
    fn get_system_prompt(&self) -> String {
        String::new()
    }
    fn get_system_prompt_options(&self) -> Option<BuildSystemPromptOptions> {
        Some(default_system_prompt_options(&self.cwd))
    }
}

fn default_system_prompt_options(cwd: &str) -> BuildSystemPromptOptions {
    let base = BuildSystemPromptOptions {
        custom_prompt: None,
        force_system_prompt: None,
        selected_tools: None,
        hidden_tools: None,
        tool_snippets: None,
        tool_guidelines: None,
        prompt_guidelines: None,
        append_system_prompt: None,
        sections: None,
        cwd: cwd.to_string(),
        context_files: None,
        skills: None,
    };
    normalized_to_options(&normalize_build_system_prompt_options(&base))
}

/// Bound context actions with the TS fallback for the optional `getSystemPromptOptions`.
struct RunnerContextActions {
    inner: Arc<dyn ExtensionContextActions>,
    cwd: String,
}

impl ExtensionContextActions for RunnerContextActions {
    fn get_model(&self) -> Option<Model> {
        self.inner.get_model()
    }
    fn get_scoped_models(&self) -> Vec<ScopedModel> {
        self.inner.get_scoped_models()
    }
    fn is_idle(&self) -> bool {
        self.inner.is_idle()
    }
    fn is_project_trusted(&self) -> bool {
        self.inner.is_project_trusted()
    }
    fn get_signal(&self) -> Option<CancellationToken> {
        self.inner.get_signal()
    }
    fn abort(&self) {
        self.inner.abort()
    }
    fn has_pending_messages(&self) -> bool {
        self.inner.has_pending_messages()
    }
    fn shutdown(&self) {
        self.inner.shutdown()
    }
    fn get_context_usage(&self) -> Option<ContextUsage> {
        self.inner.get_context_usage()
    }
    fn compact(&self, options: CompactOptions) {
        self.inner.compact(options)
    }
    fn get_system_prompt(&self) -> String {
        self.inner.get_system_prompt()
    }
    fn get_system_prompt_options(&self) -> Option<BuildSystemPromptOptions> {
        self.inner.get_system_prompt_options().or_else(|| Some(default_system_prompt_options(&self.cwd)))
    }
    fn execute_tool_fn(&self) -> Option<ExecuteToolFn> {
        self.inner.execute_tool_fn()
    }
    fn get_callable_tools(&self) -> Option<Vec<AgentToolRef>> {
        Some(self.inner.get_callable_tools().unwrap_or_default())
    }
}

struct SystemPromptOverrideActions {
    inner: Arc<dyn ExtensionContextActions>,
    render: Arc<dyn Fn() -> String + Send + Sync>,
}

impl ExtensionContextActions for SystemPromptOverrideActions {
    fn get_model(&self) -> Option<Model> {
        self.inner.get_model()
    }
    fn get_scoped_models(&self) -> Vec<ScopedModel> {
        self.inner.get_scoped_models()
    }
    fn is_idle(&self) -> bool {
        self.inner.is_idle()
    }
    fn is_project_trusted(&self) -> bool {
        self.inner.is_project_trusted()
    }
    fn get_signal(&self) -> Option<CancellationToken> {
        self.inner.get_signal()
    }
    fn abort(&self) {
        self.inner.abort()
    }
    fn has_pending_messages(&self) -> bool {
        self.inner.has_pending_messages()
    }
    fn shutdown(&self) {
        self.inner.shutdown()
    }
    fn get_context_usage(&self) -> Option<ContextUsage> {
        self.inner.get_context_usage()
    }
    fn compact(&self, options: CompactOptions) {
        self.inner.compact(options)
    }
    fn get_system_prompt(&self) -> String {
        (self.render)()
    }
    fn get_system_prompt_options(&self) -> Option<BuildSystemPromptOptions> {
        self.inner.get_system_prompt_options()
    }
    fn execute_tool_fn(&self) -> Option<ExecuteToolFn> {
        self.inner.execute_tool_fn()
    }
    fn get_callable_tools(&self) -> Option<Vec<AgentToolRef>> {
        self.inner.get_callable_tools()
    }
}

/// Command actions resolved at call time (`bindCommandContext()` may run later), with the TS
/// defaults when unbound.
struct RunnerCommandActions {
    runner: Weak<ExtensionRunner>,
}

impl RunnerCommandActions {
    fn bound(&self) -> Option<Arc<dyn ExtensionCommandContextActions>> {
        self.runner.upgrade().and_then(|r| r.command_actions.read().clone())
    }
}

#[async_trait::async_trait]
impl ExtensionCommandContextActions for RunnerCommandActions {
    async fn wait_for_idle(&self) -> anyhow::Result<()> {
        match self.bound() {
            Some(a) => a.wait_for_idle().await,
            None => Ok(()),
        }
    }
    async fn new_session(&self, options: NewSessionOptions) -> anyhow::Result<SessionReplaceOutcome> {
        match self.bound() {
            Some(a) => a.new_session(options).await,
            None => Ok(SessionReplaceOutcome { cancelled: false }),
        }
    }
    async fn fork(&self, entry_id: &str, options: ForkOptions) -> anyhow::Result<SessionReplaceOutcome> {
        match self.bound() {
            Some(a) => a.fork(entry_id, options).await,
            None => Ok(SessionReplaceOutcome { cancelled: false }),
        }
    }
    async fn navigate_tree(&self, target_id: &str, options: NavigateTreeOptions) -> anyhow::Result<SessionReplaceOutcome> {
        match self.bound() {
            Some(a) => a.navigate_tree(target_id, options).await,
            None => Ok(SessionReplaceOutcome { cancelled: false }),
        }
    }
    async fn switch_session(&self, session_path: &str, options: SwitchSessionOptions) -> anyhow::Result<SessionReplaceOutcome> {
        match self.bound() {
            Some(a) => a.switch_session(session_path, options).await,
            None => Ok(SessionReplaceOutcome { cancelled: false }),
        }
    }
    async fn reload(&self) -> anyhow::Result<()> {
        match self.bound() {
            Some(a) => a.reload().await,
            None => Ok(()),
        }
    }
}

/// Registration backend installed by `bindCore()`: provider and virtual model changes apply
/// immediately.
struct BoundRuntimeHooks {
    runner: Weak<ExtensionRunner>,
    model_registry: Arc<ModelRegistry>,
    provider_actions: ExtensionRunnerBindCoreProviderActions,
}

impl BoundRuntimeHooks {
    fn do_register_provider(&self, name: &str, config: ProviderConfig) -> anyhow::Result<()> {
        if let Some(register) = &self.provider_actions.register_provider {
            return register(name.to_string(), config);
        }
        self.model_registry.register_provider(
            &ModelRegistryRegisterProviderProviderOrName::String(name.to_string()),
            Some(&provider_config_input(&config)),
        )
    }

    fn do_register_native_provider(&self, provider: Arc<dyn Provider>) -> anyhow::Result<()> {
        if let Some(register) = &self.provider_actions.register_native_provider {
            return register(provider);
        }
        self.model_registry.register_provider(&ModelRegistryRegisterProviderProviderOrName::Arc(provider), None)
    }

    fn do_register_virtual_model(&self, definition: VirtualModelDefinition) -> anyhow::Result<()> {
        if let Some(register) = &self.provider_actions.register_virtual_model {
            return register(definition);
        }
        self.model_registry.register_virtual_model(&definition)
    }
}

impl ExtensionRuntimeHooks for BoundRuntimeHooks {
    fn create_context(&self) -> anyhow::Result<ExtensionContext> {
        match self.runner.upgrade() {
            Some(r) => Ok(r.create_context()),
            None => Err(anyhow::anyhow!(RUNNER_GONE)),
        }
    }
    fn register_provider(&self, name: &str, config: ProviderConfig, _extension_path: Option<&str>) -> anyhow::Result<()> {
        self.do_register_provider(name, config)
    }
    fn register_native_provider(&self, provider: Arc<dyn Provider>, _extension_path: Option<&str>) -> anyhow::Result<()> {
        self.do_register_native_provider(provider)
    }
    fn unregister_provider(&self, name: &str, _extension_path: Option<&str>) -> anyhow::Result<()> {
        match &self.provider_actions.unregister_provider {
            Some(unregister) => unregister(name.to_string()),
            None => self.model_registry.unregister_provider(name),
        }
        Ok(())
    }
    fn register_virtual_model(&self, definition: VirtualModelDefinition, _extension_path: Option<&str>) -> anyhow::Result<()> {
        self.do_register_virtual_model(definition)
    }
    fn unregister_virtual_model(&self, provider: &str, id: &str) -> anyhow::Result<()> {
        match &self.provider_actions.unregister_virtual_model {
            Some(unregister) => unregister(provider.to_string(), id.to_string()),
            None => self.model_registry.unregister_virtual_model(provider, id),
        }
        Ok(())
    }
}

/// UI context wrapper that reports blocking prompts as `ui_prompt_start` / `ui_prompt_end`
/// (TS `wrapUIPromptContext`).
struct PromptTrackingUIContext {
    inner: Arc<dyn ExtensionUIContext>,
    runner: Weak<ExtensionRunner>,
}

struct PromptGuard {
    runner: Weak<ExtensionRunner>,
    kind: UIPromptKind,
    title: Option<String>,
}

impl PromptGuard {
    fn start(runner: &Weak<ExtensionRunner>, kind: UIPromptKind, title: Option<&str>) -> Self {
        if let Some(r) = runner.upgrade() {
            r.ui_prompt_start(kind, title);
        }
        Self { runner: runner.clone(), kind, title: title.map(str::to_string) }
    }
}

impl Drop for PromptGuard {
    fn drop(&mut self) {
        if let Some(r) = self.runner.upgrade() {
            r.ui_prompt_finish(self.kind, self.title.as_deref());
        }
    }
}

#[async_trait::async_trait]
impl ExtensionUIContext for PromptTrackingUIContext {
    async fn select(&self, title: &str, options: Vec<String>, opts: ExtensionUIDialogOptions) -> Option<String> {
        let _guard = PromptGuard::start(&self.runner, UIPromptKind::Select, Some(title));
        self.inner.select(title, options, opts).await
    }
    async fn confirm(&self, title: &str, message: &str, opts: ExtensionUIDialogOptions) -> bool {
        let _guard = PromptGuard::start(&self.runner, UIPromptKind::Confirm, Some(title));
        self.inner.confirm(title, message, opts).await
    }
    async fn input(&self, title: &str, placeholder: Option<&str>, opts: ExtensionUIDialogOptions) -> Option<String> {
        let _guard = PromptGuard::start(&self.runner, UIPromptKind::Input, Some(title));
        self.inner.input(title, placeholder, opts).await
    }
    fn notify(&self, message: &str, kind: NotifyType) {
        self.inner.notify(message, kind)
    }
    fn on_terminal_input(&self, handler: TerminalInputHandler) -> Unsubscribe {
        self.inner.on_terminal_input(handler)
    }
    fn set_status(&self, key: &str, text: Option<&str>) {
        self.inner.set_status(key, text)
    }
    fn set_working_message(&self, message: Option<&str>) {
        self.inner.set_working_message(message)
    }
    fn set_working_visible(&self, visible: bool) {
        self.inner.set_working_visible(visible)
    }
    fn set_working_indicator(&self, options: Option<WorkingIndicatorOptions>) {
        self.inner.set_working_indicator(options)
    }
    fn set_hidden_thinking_label(&self, label: Option<&str>) {
        self.inner.set_hidden_thinking_label(label)
    }
    fn set_widget(&self, key: &str, content: Option<WidgetContent>, options: ExtensionWidgetOptions) {
        self.inner.set_widget(key, content, options)
    }
    fn set_footer(&self, factory: Option<FooterFactory>) {
        self.inner.set_footer(factory)
    }
    fn set_header(&self, factory: Option<ComponentFactory>) {
        self.inner.set_header(factory)
    }
    fn set_title(&self, title: &str) {
        self.inner.set_title(title)
    }
    async fn custom(&self, factory: CustomComponentFactory, options: CustomUiOptions) -> anyhow::Result<CustomUiResult> {
        let _guard = PromptGuard::start(&self.runner, UIPromptKind::Custom, None);
        self.inner.custom(factory, options).await
    }
    fn paste_to_editor(&self, text: &str) {
        self.inner.paste_to_editor(text)
    }
    fn set_editor_text(&self, text: &str) {
        self.inner.set_editor_text(text)
    }
    fn get_editor_text(&self) -> String {
        self.inner.get_editor_text()
    }
    async fn editor(&self, title: &str, prefill: Option<&str>) -> Option<String> {
        let _guard = PromptGuard::start(&self.runner, UIPromptKind::Editor, Some(title));
        self.inner.editor(title, prefill).await
    }
    fn add_autocomplete_provider(&self, factory: AutocompleteProviderFactory) {
        self.inner.add_autocomplete_provider(factory)
    }
    fn set_editor_component(&self, factory: Option<EditorFactory>) {
        self.inner.set_editor_component(factory)
    }
    fn get_editor_component(&self) -> Option<EditorFactory> {
        self.inner.get_editor_component()
    }
    fn theme(&self) -> Arc<crate::modes::interactive::theme::theme::Theme> {
        self.inner.theme()
    }
    fn get_all_themes(&self) -> Vec<ThemeInfo> {
        self.inner.get_all_themes()
    }
    fn get_theme(&self, name: &str) -> Option<Arc<crate::modes::interactive::theme::theme::Theme>> {
        self.inner.get_theme(name)
    }
    fn set_theme(&self, theme: ThemeSelection) -> SetThemeResult {
        self.inner.set_theme(theme)
    }
    fn get_tools_expanded(&self) -> bool {
        self.inner.get_tools_expanded()
    }
    fn set_tools_expanded(&self, expanded: bool) {
        self.inner.set_tools_expanded(expanded)
    }
}

/// UI context used when no mode provided one (TS `noOpUIContext`): dialogs resolve empty and
/// everything else does nothing. `has_ui()` is false while it is installed.
pub struct NoOpUIContext;

#[async_trait::async_trait]
impl ExtensionUIContext for NoOpUIContext {
    async fn select(&self, _title: &str, _options: Vec<String>, _opts: ExtensionUIDialogOptions) -> Option<String> {
        None
    }
    async fn confirm(&self, _title: &str, _message: &str, _opts: ExtensionUIDialogOptions) -> bool {
        false
    }
    async fn input(&self, _title: &str, _placeholder: Option<&str>, _opts: ExtensionUIDialogOptions) -> Option<String> {
        None
    }
    fn notify(&self, _message: &str, _kind: NotifyType) {}
    fn on_terminal_input(&self, _handler: TerminalInputHandler) -> Unsubscribe {
        Arc::new(|| {})
    }
    fn set_status(&self, _key: &str, _text: Option<&str>) {}
    fn set_working_message(&self, _message: Option<&str>) {}
    fn set_working_visible(&self, _visible: bool) {}
    fn set_working_indicator(&self, _options: Option<WorkingIndicatorOptions>) {}
    fn set_hidden_thinking_label(&self, _label: Option<&str>) {}
    fn set_widget(&self, _key: &str, _content: Option<WidgetContent>, _options: ExtensionWidgetOptions) {}
    fn set_footer(&self, _factory: Option<FooterFactory>) {}
    fn set_header(&self, _factory: Option<ComponentFactory>) {}
    fn set_title(&self, _title: &str) {}
    async fn custom(&self, _factory: CustomComponentFactory, _options: CustomUiOptions) -> anyhow::Result<CustomUiResult> {
        // TS resolves `undefined`.
        Ok(Box::new(()))
    }
    fn paste_to_editor(&self, _text: &str) {}
    fn set_editor_text(&self, _text: &str) {}
    fn get_editor_text(&self) -> String {
        String::new()
    }
    async fn editor(&self, _title: &str, _prefill: Option<&str>) -> Option<String> {
        None
    }
    fn add_autocomplete_provider(&self, _factory: AutocompleteProviderFactory) {}
    fn set_editor_component(&self, _factory: Option<EditorFactory>) {}
    fn get_editor_component(&self) -> Option<EditorFactory> {
        None
    }
    fn theme(&self) -> Arc<crate::modes::interactive::theme::theme::Theme> {
        crate::modes::interactive::theme::theme::theme()
    }
    fn get_all_themes(&self) -> Vec<ThemeInfo> {
        Vec::new()
    }
    fn get_theme(&self, _name: &str) -> Option<Arc<crate::modes::interactive::theme::theme::Theme>> {
        None
    }
    fn set_theme(&self, _theme: ThemeSelection) -> SetThemeResult {
        SetThemeResult { success: false, error: Some("UI not available".to_string()) }
    }
    fn get_tools_expanded(&self) -> bool {
        false
    }
    fn set_tools_expanded(&self, _expanded: bool) {}
}
