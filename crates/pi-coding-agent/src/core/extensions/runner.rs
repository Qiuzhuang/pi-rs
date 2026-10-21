//! Port of `packages/coding-agent/src/core/extensions/runner.ts`.

use crate::core::cache_warmer::CacheWarmingAction;
use crate::core::cache_warmer::CacheWarmingDecisionEvent;
use crate::core::diagnostics::ResourceDiagnostic;
use crate::core::extensions::types::AgentActivityOutcome;
use crate::core::extensions::types::BoundaryContextPreview;
use crate::core::extensions::types::EntryRenderer;
use crate::core::extensions::types::ExtensionActions;
use crate::core::extensions::types::ExtensionCommandContext;
use crate::core::extensions::types::ExtensionCommandContextActions;
use crate::core::extensions::types::ExtensionContext;
use crate::core::extensions::types::ExtensionContextActions;
use crate::core::extensions::types::ExtensionError;
use crate::core::extensions::types::ExtensionEvent;
use crate::core::extensions::types::ExtensionFlag;
use crate::core::extensions::types::ExtensionMode;
use crate::core::extensions::types::ExtensionRef;
use crate::core::extensions::types::ExtensionRuntime;
use crate::core::extensions::types::ExtensionShortcut;
use crate::core::extensions::types::ExtensionToolContext;
use crate::core::extensions::types::ExtensionUIContext;
use crate::core::extensions::types::InputEventResult;
use crate::core::extensions::types::InputSource;
use crate::core::extensions::types::LoadExtensionsResult;
use crate::core::extensions::types::MarkdownTransformer;
use crate::core::extensions::types::MessageEndEvent;
use crate::core::extensions::types::MessageRenderer;
use crate::core::extensions::types::ProjectTrustContext;
use crate::core::extensions::types::ProjectTrustEvent;
use crate::core::extensions::types::ProjectTrustEventResult;
use crate::core::extensions::types::ProviderConfig;
use crate::core::extensions::types::RegisteredTool;
use crate::core::extensions::types::ReplacedSessionContext;
use crate::core::extensions::types::ResolvedCommand;
use crate::core::extensions::types::SessionBeforeCompactResult;
use crate::core::extensions::types::SessionBeforeForkResult;
use crate::core::extensions::types::SessionBeforeSwitchResult;
use crate::core::extensions::types::SessionBeforeTreeResult;
use crate::core::extensions::types::SessionBoundaryDraft;
use crate::core::extensions::types::SessionShutdownEvent;
use crate::core::extensions::types::ToolCallEvent;
use crate::core::extensions::types::ToolCallEventResult;
use crate::core::extensions::types::ToolDefinition;
use crate::core::extensions::types::ToolRenderers;
use crate::core::extensions::types::ToolResultEvent;
use crate::core::extensions::types::ToolResultEventResult;
use crate::core::extensions::types::UserBashEvent;
use crate::core::extensions::types::UserBashEventResult;
use crate::core::messages::CustomMessage;
use crate::core::model_registry::ModelRegistry;
use crate::core::session_manager::SessionManager;
use crate::core::system_prompt::BuildSystemPromptOptions;
use crate::core::system_prompt::NormalizedBuildSystemPromptOptions;
use crate::core::virtual_models::VirtualModelDefinition;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_agent::types::AgentMessage;
use pi_ai::models::Provider;
use pi_ai::types::ImageContent;
use pi_ai::types::ProviderHeaders;
use pi_ai::types::ToolResultMessage;
use pi_ai::types::UserContent;
use pi_tui::keybindings::KeybindingsConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltInKeyBindingsValue {
    pub keybinding: String,
    pub restrict_override: bool,
}

pub type BuiltInKeyBindings = IndexMap<String, BuiltInKeyBindingsValue>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartTriggerVariant1 {
    /// Always "user".
    pub message_type: String,
    /// The raw user prompt text (after expansion).
    pub prompt: String,
    /// Images attached to the user prompt, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ImageContent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartTriggerVariant2 {
    /// Always "custom".
    pub message_type: String,
    /// The custom message that starts the run.
    pub message: CustomMessage,
}

/// The message that starts a run: the before_agent_start event without its prompt fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BeforeAgentStartTrigger {
    BeforeAgentStartTriggerVariant1(BeforeAgentStartTriggerVariant1),
    BeforeAgentStartTriggerVariant2(BeforeAgentStartTriggerVariant2),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartCombinedResultMessages {
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// Combined result from all before_agent_start handlers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartCombinedResult {
    pub messages: Vec<BeforeAgentStartCombinedResultMessages>,
    pub system_prompt_options: NormalizedBuildSystemPromptOptions,
}

/// Events handled by the generic emit() method.
/// Events with dedicated emitXxx() methods are excluded for stronger type safety.
pub type RunnerEmitEvent = ExtensionEvent;

pub type SessionBeforeEvent = RunnerEmitEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionBeforeEventResult {
    SessionBeforeSwitchResult(SessionBeforeSwitchResult),
    SessionBeforeForkResult(SessionBeforeForkResult),
    SessionBeforeCompactResult(SessionBeforeCompactResult),
    SessionBeforeTreeResult(SessionBeforeTreeResult),
}

pub type RunnerEmitResult = Value;

pub type ExtensionErrorListener = Arc<dyn Fn(ExtensionError) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryBaseEventVariant1 {
    pub message: AgentMessage,
    /// Always "turn_end".
    pub r#type: String,
    pub turn_index: i64,
    pub tool_results: Vec<ToolResultMessage>,
    pub message_entry_id: String,
    pub tool_result_entry_ids: Vec<String>,
    pub outcome: AgentActivityOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryBaseEventVariant2 {
    /// Always "agent_before_settle".
    pub r#type: String,
    pub outcome: AgentActivityOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoundaryBaseEvent {
    BoundaryBaseEventVariant1(BoundaryBaseEventVariant1),
    BoundaryBaseEventVariant2(BoundaryBaseEventVariant2),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryDispatchResult {
    pub entries: Vec<SessionBoundaryDraft>,
    pub r#continue: bool,
    pub context: BoundaryContextPreview,
    pub valid: bool,
}

#[derive(Clone)]
pub struct NewSessionHandlerOptions {
    pub parent_session: Option<String>,
    pub setup: Option<
        Arc<dyn Fn(Arc<SessionManager>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
    pub with_session: Option<
        Arc<dyn Fn(ReplacedSessionContext) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionHandlerResult {
    pub cancelled: bool,
}

pub type NewSessionHandler = Arc<
    dyn Fn(
            Option<NewSessionHandlerOptions>,
        ) -> BoxFuture<'static, anyhow::Result<NewSessionHandlerResult>>
        + Send
        + Sync,
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
    pub with_session: Option<
        Arc<dyn Fn(ReplacedSessionContext) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkHandlerResult {
    pub cancelled: bool,
}

pub type ForkHandler = Arc<
    dyn Fn(
            String,
            Option<ForkHandlerOptions>,
        ) -> BoxFuture<'static, anyhow::Result<ForkHandlerResult>>
        + Send
        + Sync,
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
    dyn Fn(
            String,
            Option<NavigateTreeHandlerOptions>,
        ) -> BoxFuture<'static, anyhow::Result<NavigateTreeHandlerResult>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct SwitchSessionHandlerOptions {
    pub with_session: Option<
        Arc<dyn Fn(ReplacedSessionContext) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchSessionHandlerResult {
    pub cancelled: bool,
}

pub type SwitchSessionHandler = Arc<
    dyn Fn(
            String,
            Option<SwitchSessionHandlerOptions>,
        ) -> BoxFuture<'static, anyhow::Result<SwitchSessionHandlerResult>>
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
    todo!("port core/extensions/runner.ts:emitSessionShutdownEvent")
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
    todo!("port core/extensions/runner.ts:emitProjectTrustEvent")
}

#[derive(Clone)]
pub struct ExtensionRunnerProviderActions {
    pub register_provider: Option<Arc<dyn Fn(String, ProviderConfig) + Send + Sync>>,
    pub register_native_provider: Option<Arc<dyn Fn(Arc<dyn Provider>) + Send + Sync>>,
    pub unregister_provider: Option<Arc<dyn Fn(String) + Send + Sync>>,
    pub register_virtual_model: Option<Arc<dyn Fn(VirtualModelDefinition) + Send + Sync>>,
    pub unregister_virtual_model: Option<Arc<dyn Fn(String, String) + Send + Sync>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtensionRunnerValue {
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtensionRunnerGetFlagValuesResult {
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExtensionRunnerReason {
    #[serde(rename = "startup")]
    Startup,
    #[serde(rename = "reload")]
    Reload,
}

impl ExtensionRunnerReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Reload => "reload",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionRunnerEmitResourcesDiscoverResultSkillPaths {
    pub path: String,
    pub extension_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionRunnerEmitResourcesDiscoverResultPromptPaths {
    pub path: String,
    pub extension_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionRunnerEmitResourcesDiscoverResultThemePaths {
    pub path: String,
    pub extension_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionRunnerEmitResourcesDiscoverResult {
    pub skill_paths: Vec<ExtensionRunnerEmitResourcesDiscoverResultSkillPaths>,
    pub prompt_paths: Vec<ExtensionRunnerEmitResourcesDiscoverResultPromptPaths>,
    pub theme_paths: Vec<ExtensionRunnerEmitResourcesDiscoverResultThemePaths>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExtensionRunnerStreamingBehavior {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

impl ExtensionRunnerStreamingBehavior {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::FollowUp => "followUp",
        }
    }
}

pub struct ExtensionRunner {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ExtensionRunner {
    pub fn new(
        extensions: &[ExtensionRef],
        runtime: Arc<ExtensionRuntime>,
        cwd: &str,
        session_manager: Arc<SessionManager>,
        model_registry: Arc<ModelRegistry>,
    ) -> Self {
        todo!("port core/extensions/runner.ts:ExtensionRunner.constructor")
    }

    pub fn bind_core(
        &self,
        actions: Arc<dyn ExtensionActions>,
        context_actions: Arc<dyn ExtensionContextActions>,
        provider_actions: Option<&ExtensionRunnerProviderActions>,
    ) {
        todo!("port core/extensions/runner.ts:ExtensionRunner.bindCore")
    }

    pub fn bind_command_context(&self, actions: Option<Arc<dyn ExtensionCommandContextActions>>) {
        todo!("port core/extensions/runner.ts:ExtensionRunner.bindCommandContext")
    }

    pub fn set_ui_context(
        &self,
        ui_context: Option<Arc<dyn ExtensionUIContext>>,
        mode: Option<&ExtensionMode>,
    ) {
        todo!("port core/extensions/runner.ts:ExtensionRunner.setUIContext")
    }

    pub fn get_ui_context(&self) -> Arc<dyn ExtensionUIContext> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getUIContext")
    }

    pub fn has_ui(&self) -> bool {
        todo!("port core/extensions/runner.ts:ExtensionRunner.hasUI")
    }

    pub fn get_extension_paths(&self) -> Vec<String> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getExtensionPaths")
    }

    /// Get all registered tools from all extensions (first registration per name wins).
    pub fn get_all_registered_tools(&self) -> Vec<RegisteredTool> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getAllRegisteredTools")
    }

    /// Get a tool definition by name. Returns undefined if not found.
    pub fn get_tool_definition(&self, tool_name: &str) -> Option<ToolDefinition> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getToolDefinition")
    }

    pub fn get_flags(&self) -> IndexMap<String, ExtensionFlag> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getFlags")
    }

    pub fn set_flag_value(&self, name: &str, value: &ExtensionRunnerValue) {
        todo!("port core/extensions/runner.ts:ExtensionRunner.setFlagValue")
    }

    pub fn get_flag_values(&self) -> IndexMap<String, ExtensionRunnerGetFlagValuesResult> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getFlagValues")
    }

    pub fn get_shortcuts(
        &self,
        resolved_keybindings: KeybindingsConfig,
    ) -> IndexMap<String, ExtensionShortcut> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getShortcuts")
    }

    pub fn get_shortcut_diagnostics(&self) -> Vec<ResourceDiagnostic> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getShortcutDiagnostics")
    }

    pub fn invalidate(&self, message: Option<&str>) {
        todo!("port core/extensions/runner.ts:ExtensionRunner.invalidate")
    }

    pub fn on_error(&self, listener: &ExtensionErrorListener) -> Arc<dyn Fn() + Send + Sync> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.onError")
    }

    pub fn emit_error(&self, error: &ExtensionError) {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitError")
    }

    /// Report registered MCP servers when no extension handles `mcp_servers_change`, which means
    /// nothing connects them (for example when another MCP extension replaced the built-in one).
    pub fn report_unhandled_mcp_servers(&self) {
        todo!("port core/extensions/runner.ts:ExtensionRunner.reportUnhandledMcpServers")
    }

    pub fn has_handlers(&self, event_type: &str) -> bool {
        todo!("port core/extensions/runner.ts:ExtensionRunner.hasHandlers")
    }

    pub fn get_message_renderer(&self, custom_type: &str) -> Option<MessageRenderer> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getMessageRenderer")
    }

    pub fn get_markdown_transformers(&self) -> Vec<MarkdownTransformer> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getMarkdownTransformers")
    }

    /// Renderers of calls to `toolName`: extension resolvers in load order, then `base`.
    pub fn resolve_tool_renderers(
        &self,
        tool_name: &str,
        base: Arc<dyn Fn() -> Option<ToolRenderers> + Send + Sync>,
    ) -> Option<ToolRenderers> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.resolveToolRenderers")
    }

    pub fn get_entry_renderer(&self, custom_type: &str) -> Option<EntryRenderer> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getEntryRenderer")
    }

    pub fn get_model_registry(&self) -> Arc<ModelRegistry> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getModelRegistry")
    }

    pub fn get_registered_commands(&self) -> Vec<ResolvedCommand> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getRegisteredCommands")
    }

    pub fn get_command_diagnostics(&self) -> Vec<ResourceDiagnostic> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getCommandDiagnostics")
    }

    pub fn get_command(&self, name: &str) -> Option<ResolvedCommand> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getCommand")
    }

    /// Request a graceful shutdown. Called by extension tools and event handlers.
    /// The actual shutdown behavior is provided by the mode via bindExtensions().
    pub fn shutdown(&self) {
        todo!("port core/extensions/runner.ts:ExtensionRunner.shutdown")
    }

    pub fn get_active_tools(&self) -> Vec<String> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.getActiveTools")
    }

    /// Create an ExtensionContext for use in event handlers and tool execution.
    /// Context values are resolved at call time, so changes via bindCore/bindUI are reflected.
    pub fn create_context(&self) -> ExtensionContext {
        todo!("port core/extensions/runner.ts:ExtensionRunner.createContext")
    }

    /// Create the context for executing the tool call `toolCallId`: the extension context plus
    /// `tools` and `executeTool()`. `signal` is the default signal of nested calls.
    pub fn create_tool_context(
        &self,
        tool_call_id: &str,
        signal: Option<CancellationToken>,
    ) -> ExtensionToolContext {
        todo!("port core/extensions/runner.ts:ExtensionRunner.createToolContext")
    }

    pub fn create_command_context(&self) -> ExtensionCommandContext {
        todo!("port core/extensions/runner.ts:ExtensionRunner.createCommandContext")
    }

    pub async fn emit_boundary(
        &self,
        base_event: &BoundaryBaseEvent,
        build_context: Arc<
            dyn Fn(
                    Vec<SessionBoundaryDraft>,
                ) -> BoxFuture<'static, anyhow::Result<BoundaryContextPreview>>
                + Send
                + Sync,
        >,
    ) -> anyhow::Result<BoundaryDispatchResult> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitBoundary")
    }

    pub async fn emit(&self, event: Value) -> anyhow::Result<RunnerEmitResult> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emit")
    }

    /// Returns the event's own action unless a handler overrides it; the last override wins.
    pub async fn emit_cache_warming_decision(
        &self,
        event: &CacheWarmingDecisionEvent,
    ) -> anyhow::Result<CacheWarmingAction> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitCacheWarmingDecision")
    }

    pub async fn emit_message_end(
        &self,
        event: &MessageEndEvent,
    ) -> anyhow::Result<Option<AgentMessage>> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitMessageEnd")
    }

    pub async fn emit_tool_result(
        &self,
        event: &ToolResultEvent,
    ) -> anyhow::Result<Option<ToolResultEventResult>> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitToolResult")
    }

    pub async fn emit_tool_call(
        &self,
        event: &ToolCallEvent,
    ) -> anyhow::Result<Option<ToolCallEventResult>> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitToolCall")
    }

    pub async fn emit_user_bash(
        &self,
        event: &UserBashEvent,
    ) -> anyhow::Result<Option<UserBashEventResult>> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitUserBash")
    }

    /// Run the request-time transforms in two phases. `context` handlers see the conversation
    /// only and Pi restores the prompt and tool state after each; `context_with_system`
    /// handlers then see the full transcript and their output is used as returned.
    pub async fn emit_context(
        &self,
        messages: &[AgentMessage],
    ) -> anyhow::Result<Vec<AgentMessage>> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitContext")
    }

    pub async fn emit_before_provider_request(&self, payload: Value) -> anyhow::Result<Value> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitBeforeProviderRequest")
    }

    pub async fn emit_before_provider_headers(
        &self,
        headers: ProviderHeaders,
    ) -> anyhow::Result<ProviderHeaders> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitBeforeProviderHeaders")
    }

    pub async fn emit_before_agent_start(
        &self,
        trigger: &BeforeAgentStartTrigger,
        system_prompt_options: &BuildSystemPromptOptions,
    ) -> anyhow::Result<BeforeAgentStartCombinedResult> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitBeforeAgentStart")
    }

    pub async fn emit_resources_discover(
        &self,
        cwd: &str,
        reason: ExtensionRunnerReason,
    ) -> anyhow::Result<ExtensionRunnerEmitResourcesDiscoverResult> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitResourcesDiscover")
    }

    /// Emit input event. Transforms chain, "handled" short-circuits.
    pub async fn emit_input(
        &self,
        text: &str,
        images: Option<&[ImageContent]>,
        source: &InputSource,
        streaming_behavior: Option<ExtensionRunnerStreamingBehavior>,
    ) -> anyhow::Result<InputEventResult> {
        todo!("port core/extensions/runner.ts:ExtensionRunner.emitInput")
    }
}
