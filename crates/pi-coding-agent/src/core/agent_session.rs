//! Port of `packages/coding-agent/src/core/agent-session.ts`.

use crate::core::bash_executor::BashResult;
use crate::core::cache_warmer::CacheWarmingStatus;
use crate::core::compaction::compaction::CompactionResult;
use crate::core::extensions::runner::ExtensionErrorListener;
use crate::core::extensions::runner::ExtensionRunner;
use crate::core::extensions::runner::ShutdownHandler;
use crate::core::extensions::types::CompactionReason;
use crate::core::extensions::types::ContextUsage;
use crate::core::extensions::types::ExtensionCommandContextActions;
use crate::core::extensions::types::ExtensionMode;
use crate::core::extensions::types::ExtensionUIContext;
use crate::core::extensions::types::InputSource;
use crate::core::extensions::types::ReplacedSessionContext;
use crate::core::extensions::types::SessionStartEvent;
use crate::core::extensions::types::ToolDefinition;
use crate::core::extensions::types::ToolInfo;
use crate::core::model_runtime::ModelRuntime;
use crate::core::prompt_templates::PromptTemplate;
use crate::core::resource_loader::ResourceLoader;
use crate::core::session_manager::BranchSummaryEntry;
use crate::core::session_manager::SessionEntry;
use crate::core::session_manager::SessionManager;
use crate::core::settings_manager::SettingsManager;
use crate::core::settings_schema::CacheWarmingMode;
use crate::core::source_info::SourceInfo;
use crate::core::tools::bash::BashOperations;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_agent::agent::Agent;
use pi_agent::types::AgentMessage;
use pi_agent::types::AgentState;
use pi_agent::types::AgentToolRef;
use pi_agent::types::ThinkingLevel;
use pi_agent::types::{AgentEvent, AgentToolResult};
use pi_ai::types::ImageContent;
use pi_ai::types::Model;
use pi_ai::types::UserContent;
use pi_ai::types::{AssistantMessageEvent, ToolResultMessage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Parsed skill block from a user message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedSkillBlock {
    pub name: String,
    pub location: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
}

/// Parse a skill block from message text.
/// Returns null if the text doesn't contain a skill block.
pub fn parse_skill_block(text: &str) -> Option<ParsedSkillBlock> {
    todo!("port core/agent-session.ts:parseSkillBlock")
}

/// Source of a `summarization_retry_attempt_start` event (discriminated by `source`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all_fields = "camelCase")]
pub enum SummarizationRetrySource {
    #[serde(rename = "branchSummary")]
    BranchSummary,
    #[serde(rename = "compaction")]
    Compaction { reason: CompactionReason },
}

/// Session-specific events that extend the core `AgentEvent` (TS `AgentSessionEvent`).
///
/// The core agent events are repeated here (except `agent_end`, which gains `willRetry`), and
/// the `tool_execution_*` events carry `parentToolCallId` for calls a tool made through
/// `ctx.executeTool()` (TS `WithParentToolCallId`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentSessionEvent {
    AgentStart,
    TurnStart,
    TurnEnd {
        message: AgentMessage,
        tool_results: Vec<ToolResultMessage>,
    },
    MessageStart {
        message: AgentMessage,
    },
    MessageUpdate {
        message: AgentMessage,
        assistant_message_event: Box<AssistantMessageEvent>,
    },
    MessageEnd {
        message: AgentMessage,
    },
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_tool_call_id: Option<String>,
    },
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        args: Value,
        partial_result: AgentToolResult,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_tool_call_id: Option<String>,
    },
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        result: AgentToolResult,
        is_error: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_tool_call_id: Option<String>,
    },
    AgentEnd {
        messages: Vec<AgentMessage>,
        will_retry: bool,
    },
    AgentSettled {
        aborted: bool,
    },
    QueueUpdate {
        steering: Vec<String>,
        follow_up: Vec<String>,
    },
    CompactionStart {
        reason: CompactionReason,
    },
    EntryAppended {
        entry: SessionEntry,
    },
    SessionInfoChanged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    ThinkingLevelChanged {
        level: ThinkingLevel,
    },
    CompactionEnd {
        reason: CompactionReason,
        /// `CompactionResult | undefined` (omitted when undefined).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<CompactionResult>,
        aborted: bool,
        will_retry: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_message: Option<String>,
    },
    AutoRetryStart {
        attempt: u64,
        max_attempts: u64,
        delay_ms: u64,
        error_message: String,
    },
    AutoRetryEnd {
        success: bool,
        attempt: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        final_error: Option<String>,
    },
    SummarizationRetryScheduled {
        attempt: u64,
        max_attempts: u64,
        delay_ms: u64,
        error_message: String,
    },
    SummarizationRetryAttemptStart(SummarizationRetrySource),
    SummarizationRetryFinished,
    BashExecutionUpdate {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        delta: String,
    },
}

impl AgentSessionEvent {
    /// Map a core agent event (except `agent_end`, which the session re-emits with `will_retry`).
    pub fn from_agent_event(
        event: AgentEvent,
        parent_tool_call_id: Option<String>,
    ) -> Option<Self> {
        Some(match event {
            AgentEvent::AgentStart => Self::AgentStart,
            AgentEvent::AgentEnd { .. } => return None,
            AgentEvent::TurnStart => Self::TurnStart,
            AgentEvent::TurnEnd {
                message,
                tool_results,
            } => Self::TurnEnd {
                message,
                tool_results,
            },
            AgentEvent::MessageStart { message } => Self::MessageStart { message },
            AgentEvent::MessageUpdate {
                message,
                assistant_message_event,
            } => Self::MessageUpdate {
                message,
                assistant_message_event,
            },
            AgentEvent::MessageEnd { message } => Self::MessageEnd { message },
            AgentEvent::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
            } => Self::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
                parent_tool_call_id,
            },
            AgentEvent::ToolExecutionUpdate {
                tool_call_id,
                tool_name,
                args,
                partial_result,
            } => Self::ToolExecutionUpdate {
                tool_call_id,
                tool_name,
                args,
                partial_result,
                parent_tool_call_id,
            },
            AgentEvent::ToolExecutionEnd {
                tool_call_id,
                tool_name,
                result,
                is_error,
                duration_ms,
            } => Self::ToolExecutionEnd {
                tool_call_id,
                tool_name,
                result,
                is_error,
                duration_ms,
                parent_tool_call_id,
            },
        })
    }
}

pub type AgentSessionEventListener = Arc<dyn Fn(AgentSessionEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionConfigScopedModels {
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Clone)]
pub struct AgentSessionConfigExtensionRunnerRef {
    pub current: Option<Arc<ExtensionRunner>>,
}

#[derive(Clone)]
pub struct AgentSessionConfig {
    pub agent: Agent,
    pub session_manager: Arc<SessionManager>,
    pub settings_manager: Arc<SettingsManager>,
    pub cwd: String,
    /// Models to cycle through with Ctrl+P (from --models flag)
    pub scoped_models: Option<Vec<AgentSessionConfigScopedModels>>,
    /// Resource loader for extensions, skills, prompts, themes, context files, and system prompt
    pub resource_loader: Arc<dyn ResourceLoader>,
    /// SDK custom tools registered outside extensions
    pub custom_tools: Option<Vec<ToolDefinition>>,
    /// Canonical model/auth runtime used by coding-agent internals.
    pub model_runtime: Arc<ModelRuntime>,
    /// Keeps the prompt cache entry of the last session request warm.
    pub cache_warmer: Option<Value>,
    /// Initial active built-in tool names. Default: [read, bash, edit, write]
    pub initial_active_tool_names: Option<Vec<String>>,
    /// Whether the initial tools come from the `defaultTools` setting. When true, a resumed session
    /// replaces them with the loadout its transcript records, with `defaultToolModifiers` applied, and
    /// reload activates tools newly added to the setting. Tools removed from it stay active.
    pub uses_default_tools: Option<bool>,
    /// `+name`/`-name` entries applied on top of the `defaultTools` setting, from `--tools`. Reload
    /// applies them to the reloaded setting too, so a removed tool stays removed.
    pub default_tool_modifiers: Option<Vec<String>>,
    /// Optional allowlist of tool names or patterns (`*` matches any characters). When provided, only
    /// matching tools are exposed. A non-empty list without `mcp__` entries also keeps MCP tools
    /// registered for codemode and tool_search; only tool_search can declare them. An empty list
    /// exposes no tools.
    pub allowed_tool_names: Option<Vec<String>>,
    /// Optional denylist of tool names or patterns. When provided, matching tools are not exposed.
    pub excluded_tool_names: Option<Vec<String>>,
    /// Override base tools (useful for custom runtimes).
    ///
    /// These are synthesized into minimal ToolDefinitions internally so AgentSession can keep
    /// a definition-first registry even when callers provide plain AgentTool instances.
    pub base_tools_override: Option<IndexMap<String, AgentToolRef>>,
    /// Mutable ref used by Agent to access the current ExtensionRunner
    pub extension_runner_ref: Option<AgentSessionConfigExtensionRunnerRef>,
    /// Session start event metadata emitted when extensions bind to this runtime.
    pub session_start_event: Option<SessionStartEvent>,
}

#[derive(Clone)]
pub struct ExtensionBindings {
    pub ui_context: Option<Arc<dyn ExtensionUIContext>>,
    pub mode: Option<ExtensionMode>,
    pub command_context_actions: Option<Arc<dyn ExtensionCommandContextActions>>,
    pub abort_handler: Option<Arc<dyn Fn() + Send + Sync>>,
    pub shutdown_handler: Option<ShutdownHandler>,
    pub on_error: Option<ExtensionErrorListener>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QueuedInputDisposition {
    #[serde(rename = "handled")]
    Handled,
    #[serde(rename = "queued")]
    Queued,
}

impl QueuedInputDisposition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Handled => "handled",
            Self::Queued => "queued",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PromptDisposition {
    #[serde(rename = "handled")]
    Handled,
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "started")]
    Started,
}

impl PromptDisposition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Handled => "handled",
            Self::Queued => "queued",
            Self::Started => "started",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PromptOptionsStreamingBehavior {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

impl PromptOptionsStreamingBehavior {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::FollowUp => "followUp",
        }
    }
}

/// Options for AgentSession.prompt()
#[derive(Clone)]
pub struct PromptOptions {
    /// Whether to dispatch extension commands and expand skill commands and prompt templates (default: true)
    pub expand_prompt_templates: Option<bool>,
    /// Image attachments
    pub images: Option<Vec<ImageContent>>,
    /// When streaming, how to queue the message: "steer" (interrupt) or "followUp" (wait). Required if streaming.
    pub streaming_behavior: Option<PromptOptionsStreamingBehavior>,
    /// Source of input for extension input event handlers. Defaults to "interactive".
    pub source: Option<InputSource>,
    /// Internal hook used by RPC mode to observe how an accepted prompt was dispatched. Not called if the prompt is rejected.
    pub preflight_result: Option<Arc<dyn Fn(PromptDisposition) + Send + Sync>>,
}

/// Options for model/thinking mutations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMutationOptions {
    /// Persist the new value to global defaults. Defaults to session-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persist: Option<bool>,
}

/// Result from cycleModel()
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCycleResult {
    pub model: Model,
    pub thinking_level: ThinkingLevel,
    /// Whether cycling through scoped models (--models flag) or all available
    pub is_scoped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatsTokens {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub input: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub output: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub cache_read: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub cache_write: f64,
    pub total: i64,
}

/// Session statistics for /session command
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    pub session_id: String,
    pub user_messages: i64,
    pub assistant_messages: i64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub tool_calls: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub tool_results: f64,
    pub total_messages: i64,
    pub tokens: SessionStatsTokens,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub cost: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_usage: Option<ContextUsage>,
}

#[derive(Clone)]
pub struct ToolDefinitionEntry {
    pub definition: ToolDefinition,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRoutedModel {
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentSessionSteeringMode {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "one-at-a-time")]
    OneAtATime,
}

impl AgentSessionSteeringMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::OneAtATime => "one-at-a-time",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionScopedModels {
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSetScopedModelsScopedModels {
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSteerOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<InputSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionFollowUpOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<InputSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSendCustomMessageMessage {
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentSessionSendCustomMessageOptionsDeliverAs {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
    #[serde(rename = "nextTurn")]
    NextTurn,
}

impl AgentSessionSendCustomMessageOptionsDeliverAs {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::FollowUp => "followUp",
            Self::NextTurn => "nextTurn",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSendCustomMessageOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_turn: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliver_as: Option<AgentSessionSendCustomMessageOptionsDeliverAs>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSendUserMessageOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliver_as: Option<PromptOptionsStreamingBehavior>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand_prompt_templates: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionClearQueueResult {
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentSessionCycleModelDirection {
    #[serde(rename = "forward")]
    Forward,
    #[serde(rename = "backward")]
    Backward,
}

impl AgentSessionCycleModelDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Backward => "backward",
        }
    }
}

#[derive(Clone)]
pub struct AgentSessionReloadOptions {
    pub before_session_start:
        Option<Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>>,
}

#[derive(Clone)]
pub struct AgentSessionExecuteBashOptions {
    pub exclude_from_context: Option<bool>,
    pub id: Option<String>,
    pub operations: Option<BashOperations>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRecordBashResultOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_from_context: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionNavigateTreeOptions {
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
pub struct AgentSessionNavigateTreeResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_text: Option<String>,
    pub cancelled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aborted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_entry: Option<BranchSummaryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionGetUserMessagesForForkingResult {
    pub entry_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionExportToHtmlOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentSessionSummarizeForBugReportOptions {
    pub hint: Option<String>,
    pub signal: CancellationToken,
}

pub struct AgentSession {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl AgentSession {
    pub fn new(config: &AgentSessionConfig) -> Self {
        todo!("port core/agent-session.ts:AgentSession.constructor")
    }

    pub fn agent(&self) -> Agent {
        todo!("port core/agent-session.ts:AgentSession.agent")
    }

    pub fn session_manager(&self) -> Arc<SessionManager> {
        todo!("port core/agent-session.ts:AgentSession.sessionManager")
    }

    pub fn settings_manager(&self) -> Arc<SettingsManager> {
        todo!("port core/agent-session.ts:AgentSession.settingsManager")
    }

    pub fn model_runtime(&self) -> Arc<ModelRuntime> {
        todo!("port core/agent-session.ts:AgentSession.modelRuntime")
    }

    /// Subscribe to agent events.
    /// Session persistence is handled internally (saves messages on message_end).
    /// Multiple listeners can be added. Returns unsubscribe function for this listener.
    pub fn subscribe(&self, listener: &AgentSessionEventListener) -> Arc<dyn Fn() + Send + Sync> {
        todo!("port core/agent-session.ts:AgentSession.subscribe")
    }

    /// Remove all listeners and disconnect from agent.
    /// Call this when completely done with the session.
    pub fn dispose(&self) {
        todo!("port core/agent-session.ts:AgentSession.dispose")
    }

    /// Refresh the public finalized transcript from the canonical session projection.
    pub fn refresh_context(&self) {
        todo!("port core/agent-session.ts:AgentSession.refreshContext")
    }

    /// Full agent state
    pub fn state(&self) -> AgentState {
        todo!("port core/agent-session.ts:AgentSession.state")
    }

    /// Current cache-warming state and the policy inputs that produced it.
    pub fn cache_warming_status(&self) -> Option<CacheWarmingStatus> {
        todo!("port core/agent-session.ts:AgentSession.cacheWarmingStatus")
    }

    /// Persist the cache-warming mode and immediately reconcile active warming.
    pub fn set_cache_warming_mode(&self, mode: &CacheWarmingMode) {
        todo!("port core/agent-session.ts:AgentSession.setCacheWarmingMode")
    }

    /// Current model (may be undefined if not yet selected)
    pub fn model(&self) -> Option<Model> {
        todo!("port core/agent-session.ts:AgentSession.model")
    }

    /// Current thinking level
    pub fn thinking_level(&self) -> ThinkingLevel {
        todo!("port core/agent-session.ts:AgentSession.thinkingLevel")
    }

    /// Under a virtual selection, the physical model and thinking level of the latest successful response.
    pub fn routed_model(&self) -> Option<AgentSessionRoutedModel> {
        todo!("port core/agent-session.ts:AgentSession.routedModel")
    }

    /// Whether the session is currently processing an agent run or post-run continuation.
    pub fn is_streaming(&self) -> bool {
        todo!("port core/agent-session.ts:AgentSession.isStreaming")
    }

    /// Whether the session has no active agent run, compaction, branch summary, retry, or queued continuation.
    pub fn is_idle(&self) -> bool {
        todo!("port core/agent-session.ts:AgentSession.isIdle")
    }

    /// Current effective system prompt, including changes not yet sent to the model.
    pub fn system_prompt(&self) -> String {
        todo!("port core/agent-session.ts:AgentSession.systemPrompt")
    }

    /// Current retry attempt (0 if not retrying)
    pub fn retry_attempt(&self) -> i64 {
        todo!("port core/agent-session.ts:AgentSession.retryAttempt")
    }

    /// Get the names of currently active tools, which are the tools declared to the model.
    /// Tools with `codemode` or `deferred` exposure are callable from other tools without being active.
    pub fn get_active_tool_names(&self) -> Vec<String> {
        todo!("port core/agent-session.ts:AgentSession.getActiveToolNames")
    }

    /// Get the names of the tools that tools can call through `ctx.executeTool()`.
    pub fn get_callable_tool_names(&self) -> Vec<String> {
        todo!("port core/agent-session.ts:AgentSession.getCallableToolNames")
    }

    /// Get all configured tools with name, description, parameter schema, prompt guidelines, and source metadata.
    pub fn get_all_tools(&self) -> Vec<ToolInfo> {
        todo!("port core/agent-session.ts:AgentSession.getAllTools")
    }

    pub fn get_tool_definition(&self, name: &str) -> Option<ToolDefinition> {
        todo!("port core/agent-session.ts:AgentSession.getToolDefinition")
    }

    /// Set active tools by name.
    /// Only tools in the registry can be enabled. Unknown and hidden tool names are ignored.
    /// Also rebuilds the system prompt to reflect the new tool set.
    /// Changes take effect on the next agent turn.
    pub fn set_active_tools_by_name(&self, tool_names: &[String]) {
        todo!("port core/agent-session.ts:AgentSession.setActiveToolsByName")
    }

    /// Whether compaction or branch summarization is currently running
    pub fn is_compacting(&self) -> bool {
        todo!("port core/agent-session.ts:AgentSession.isCompacting")
    }

    /// All messages including custom types like BashExecutionMessage
    pub fn messages(&self) -> Vec<AgentMessage> {
        todo!("port core/agent-session.ts:AgentSession.messages")
    }

    /// Current steering mode
    pub fn steering_mode(&self) -> AgentSessionSteeringMode {
        todo!("port core/agent-session.ts:AgentSession.steeringMode")
    }

    /// Current follow-up mode
    pub fn follow_up_mode(&self) -> AgentSessionSteeringMode {
        todo!("port core/agent-session.ts:AgentSession.followUpMode")
    }

    /// Current session file path, or undefined if sessions are disabled
    pub fn session_file(&self) -> Option<String> {
        todo!("port core/agent-session.ts:AgentSession.sessionFile")
    }

    /// Current session ID
    pub fn session_id(&self) -> String {
        todo!("port core/agent-session.ts:AgentSession.sessionId")
    }

    /// Current session display name, if set
    pub fn session_name(&self) -> Option<String> {
        todo!("port core/agent-session.ts:AgentSession.sessionName")
    }

    /// Scoped models for cycling (from --models flag)
    pub fn scoped_models(&self) -> Vec<AgentSessionScopedModels> {
        todo!("port core/agent-session.ts:AgentSession.scopedModels")
    }

    /// Update scoped models for cycling
    pub fn set_scoped_models(&self, scoped_models: &[AgentSessionSetScopedModelsScopedModels]) {
        todo!("port core/agent-session.ts:AgentSession.setScopedModels")
    }

    /// File-based prompt templates
    pub fn prompt_templates(&self) -> Vec<PromptTemplate> {
        todo!("port core/agent-session.ts:AgentSession.promptTemplates")
    }

    /// Send a prompt to the agent.
    /// - Handles extension commands (registered via pi.registerCommand) immediately, even during streaming
    /// - Expands file-based prompt templates by default
    /// - During streaming, queues via steer() or followUp() based on streamingBehavior option
    /// - Validates model and API key before sending (when not streaming)
    /// @throws Error if streaming and no streamingBehavior specified
    /// @throws Error if no model selected or no API key available (when not streaming)
    pub async fn prompt(&self, text: &str, options: Option<&PromptOptions>) -> anyhow::Result<()> {
        todo!("port core/agent-session.ts:AgentSession.prompt")
    }

    /// Queue a steering message while the agent is running.
    /// Delivered after the current assistant turn finishes executing its tool calls,
    /// before the next LLM call.
    /// Expands skill commands and prompt templates. Errors on extension commands.
    /// @param images Optional image attachments to include with the message
    /// @param options Input source; defaults to interactive
    /// @throws Error if text is an extension command
    pub async fn steer(
        &self,
        text: &str,
        images: Option<&[ImageContent]>,
        options: Option<&AgentSessionSteerOptions>,
    ) -> anyhow::Result<QueuedInputDisposition> {
        todo!("port core/agent-session.ts:AgentSession.steer")
    }

    /// Queue a follow-up message to be processed after the agent finishes.
    /// Delivered only when agent has no more tool calls or steering messages.
    /// Expands skill commands and prompt templates. Errors on extension commands.
    /// @param images Optional image attachments to include with the message
    /// @param options Input source; defaults to interactive
    /// @throws Error if text is an extension command
    pub async fn follow_up(
        &self,
        text: &str,
        images: Option<&[ImageContent]>,
        options: Option<&AgentSessionFollowUpOptions>,
    ) -> anyhow::Result<QueuedInputDisposition> {
        todo!("port core/agent-session.ts:AgentSession.followUp")
    }

    /// Send a custom message to the session. Creates a CustomMessageEntry.
    ///
    /// Handles four cases:
    /// - Streaming: queues message, processed when loop pulls from queue
    /// - Streaming + triggerTurn false: appended to state/session once the current turn ends
    /// - Not streaming + triggerTurn: appends to state/session, starts new turn
    /// - Not streaming + no trigger: appends to state/session, no turn
    ///
    /// @param message Custom message with customType, content, display, details
    /// @param options.triggerTurn If true and not streaming, triggers a new LLM turn
    /// @param options.deliverAs Delivery mode: "steer", "followUp", or "nextTurn"
    pub async fn send_custom_message(
        &self,
        message: &AgentSessionSendCustomMessageMessage,
        options: Option<&AgentSessionSendCustomMessageOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/agent-session.ts:AgentSession.sendCustomMessage")
    }

    /// Send a user message to the agent. Always triggers a turn.
    /// When the agent is streaming, use deliverAs to specify how to queue the message.
    ///
    /// @param content User message content (string or content array)
    /// @param options.deliverAs Delivery mode when streaming: "steer" or "followUp"
    /// @param options.expandPromptTemplates Whether to dispatch extension commands and expand skill commands and prompt templates. Default: false.
    pub async fn send_user_message(
        &self,
        content: UserContent,
        options: Option<&AgentSessionSendUserMessageOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/agent-session.ts:AgentSession.sendUserMessage")
    }

    /// Clear all queued messages and return them.
    /// Useful for restoring to editor when user aborts.
    /// @returns Object with steering and followUp arrays
    pub fn clear_queue(&self) -> AgentSessionClearQueueResult {
        todo!("port core/agent-session.ts:AgentSession.clearQueue")
    }

    /// Number of pending messages (includes both steering and follow-up)
    pub fn pending_message_count(&self) -> i64 {
        todo!("port core/agent-session.ts:AgentSession.pendingMessageCount")
    }

    /// Get pending steering messages (read-only)
    pub fn get_steering_messages(&self) -> Vec<String> {
        todo!("port core/agent-session.ts:AgentSession.getSteeringMessages")
    }

    /// Get pending follow-up messages (read-only)
    pub fn get_follow_up_messages(&self) -> Vec<String> {
        todo!("port core/agent-session.ts:AgentSession.getFollowUpMessages")
    }

    pub fn resource_loader(&self) -> Arc<dyn ResourceLoader> {
        todo!("port core/agent-session.ts:AgentSession.resourceLoader")
    }

    /// Abort current operation and wait for agent to become idle.
    pub async fn abort(&self) -> anyhow::Result<()> {
        todo!("port core/agent-session.ts:AgentSession.abort")
    }

    pub async fn wait_for_idle(&self) -> anyhow::Result<()> {
        todo!("port core/agent-session.ts:AgentSession.waitForIdle")
    }

    /// Set model directly.
    /// Validates that auth is configured and saves to the session transcript.
    /// Persists to global defaults only when options.persist is true.
    /// @throws Error if no auth is configured for the model
    pub async fn set_model(
        &self,
        model: Model,
        options: Option<&ModelMutationOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/agent-session.ts:AgentSession.setModel")
    }

    /// Cycle to next/previous model.
    /// Uses scoped models (from --models flag) if available, otherwise all available models.
    /// @param direction - "forward" (default) or "backward"
    /// @returns The new model info, or undefined if only one model available
    pub async fn cycle_model(
        &self,
        direction: Option<AgentSessionCycleModelDirection>,
        options: Option<&ModelMutationOptions>,
    ) -> anyhow::Result<Option<ModelCycleResult>> {
        todo!("port core/agent-session.ts:AgentSession.cycleModel")
    }

    /// Set thinking level.
    /// Clamps to model capabilities based on available thinking levels.
    /// Saves the clamped level to the session transcript only if the level actually changes.
    /// Persists the requested level to global defaults only when options.persist is true.
    pub fn set_thinking_level(&self, level: ThinkingLevel, options: Option<&ModelMutationOptions>) {
        todo!("port core/agent-session.ts:AgentSession.setThinkingLevel")
    }

    /// Cycle to next thinking level.
    /// @returns New level, or undefined if model doesn't support thinking
    pub fn cycle_thinking_level(
        &self,
        options: Option<&ModelMutationOptions>,
    ) -> Option<ThinkingLevel> {
        todo!("port core/agent-session.ts:AgentSession.cycleThinkingLevel")
    }

    /// Get available thinking levels for current model.
    /// The provider will clamp to what the specific model supports internally.
    pub fn get_available_thinking_levels(&self) -> Vec<ThinkingLevel> {
        todo!("port core/agent-session.ts:AgentSession.getAvailableThinkingLevels")
    }

    /// Check if current model supports thinking/reasoning.
    pub fn supports_thinking(&self) -> bool {
        todo!("port core/agent-session.ts:AgentSession.supportsThinking")
    }

    /// Set steering message mode.
    /// Saves to settings.
    pub fn set_steering_mode(&self, mode: AgentSessionSteeringMode) {
        todo!("port core/agent-session.ts:AgentSession.setSteeringMode")
    }

    /// Set follow-up message mode.
    /// Saves to settings.
    pub fn set_follow_up_mode(&self, mode: AgentSessionSteeringMode) {
        todo!("port core/agent-session.ts:AgentSession.setFollowUpMode")
    }

    /// Manually compact the session context.
    ///
    /// This is the manual entry point used by `/compact`, RPC, and extensions. It is
    /// separate from automatic threshold/overflow compaction, which enters through
    /// `_checkCompaction()` and `_runAutoCompaction()`. After preparation and the
    /// `session_before_compact` hook, both paths call the lower-level `compact()`
    /// function imported from `./compaction/index.ts`, unless the hook cancels or
    /// supplies a custom result.
    ///
    /// Aborts the current agent operation first. Manual compaction never retries or
    /// continues the interrupted agent turn.
    ///
    /// @param customInstructions Optional instructions for the compaction summary
    pub async fn compact(
        &self,
        custom_instructions: Option<&str>,
    ) -> anyhow::Result<CompactionResult> {
        todo!("port core/agent-session.ts:AgentSession.compact")
    }

    /// Cancel in-progress compaction (manual or auto).
    pub fn abort_compaction(&self) {
        todo!("port core/agent-session.ts:AgentSession.abortCompaction")
    }

    /// Cancel in-progress branch summarization.
    pub fn abort_branch_summary(&self) {
        todo!("port core/agent-session.ts:AgentSession.abortBranchSummary")
    }

    /// Toggle auto-compaction setting.
    pub fn set_auto_compaction_enabled(&self, enabled: bool) {
        todo!("port core/agent-session.ts:AgentSession.setAutoCompactionEnabled")
    }

    /// Whether auto-compaction is enabled
    pub fn auto_compaction_enabled(&self) -> bool {
        todo!("port core/agent-session.ts:AgentSession.autoCompactionEnabled")
    }

    pub async fn bind_extensions(&self, bindings: &ExtensionBindings) -> anyhow::Result<()> {
        todo!("port core/agent-session.ts:AgentSession.bindExtensions")
    }

    pub async fn reload(&self, options: Option<&AgentSessionReloadOptions>) -> anyhow::Result<()> {
        todo!("port core/agent-session.ts:AgentSession.reload")
    }

    /// Cancel in-progress retry.
    pub fn abort_retry(&self) {
        todo!("port core/agent-session.ts:AgentSession.abortRetry")
    }

    /// Whether auto-retry is currently in progress
    pub fn is_retrying(&self) -> bool {
        todo!("port core/agent-session.ts:AgentSession.isRetrying")
    }

    /// Whether auto-retry is enabled
    pub fn auto_retry_enabled(&self) -> bool {
        todo!("port core/agent-session.ts:AgentSession.autoRetryEnabled")
    }

    /// Toggle auto-retry setting.
    pub fn set_auto_retry_enabled(&self, enabled: bool) {
        todo!("port core/agent-session.ts:AgentSession.setAutoRetryEnabled")
    }

    /// Execute a bash command.
    /// Adds result to agent context and session.
    /// @param command The bash command to execute
    /// @param onChunk Optional streaming callback for output
    /// @param options.excludeFromContext If true, command output won't be sent to LLM (!! prefix)
    /// @param options.id Optional identifier included in bash execution update events
    /// @param options.operations Custom BashOperations for remote execution
    pub async fn execute_bash(
        &self,
        command: &str,
        on_chunk: Option<Arc<dyn Fn(String) + Send + Sync>>,
        options: Option<&AgentSessionExecuteBashOptions>,
    ) -> anyhow::Result<BashResult> {
        todo!("port core/agent-session.ts:AgentSession.executeBash")
    }

    /// Record a bash execution result in session history.
    /// Used by executeBash and by extensions that handle bash execution themselves.
    pub fn record_bash_result(
        &self,
        command: &str,
        result: &BashResult,
        options: Option<&AgentSessionRecordBashResultOptions>,
    ) {
        todo!("port core/agent-session.ts:AgentSession.recordBashResult")
    }

    /// Cancel running bash command.
    pub fn abort_bash(&self) {
        todo!("port core/agent-session.ts:AgentSession.abortBash")
    }

    /// Whether a bash command is currently running
    pub fn is_bash_running(&self) -> bool {
        todo!("port core/agent-session.ts:AgentSession.isBashRunning")
    }

    /// Whether there are pending bash messages waiting to be flushed
    pub fn has_pending_bash_messages(&self) -> bool {
        todo!("port core/agent-session.ts:AgentSession.hasPendingBashMessages")
    }

    /// Set a display name for the current session.
    pub fn set_session_name(&self, name: &str) {
        todo!("port core/agent-session.ts:AgentSession.setSessionName")
    }

    /// Navigate to a different node in the session tree.
    /// Unlike fork() which creates a new session file, this stays in the same file.
    ///
    /// @param targetId The entry ID to navigate to
    /// @param options.summarize Whether user wants to summarize abandoned branch
    /// @param options.customInstructions Custom instructions for summarizer
    /// @param options.replaceInstructions If true, customInstructions replaces the default prompt
    /// @param options.label Label to attach to the branch summary entry
    /// @returns Result with editorText (if user message) and cancelled status
    pub async fn navigate_tree(
        &self,
        target_id: &str,
        options: Option<&AgentSessionNavigateTreeOptions>,
    ) -> anyhow::Result<AgentSessionNavigateTreeResult> {
        todo!("port core/agent-session.ts:AgentSession.navigateTree")
    }

    /// Get all user messages from session for fork selector.
    pub fn get_user_messages_for_forking(
        &self,
    ) -> Vec<AgentSessionGetUserMessagesForForkingResult> {
        todo!("port core/agent-session.ts:AgentSession.getUserMessagesForForking")
    }

    /// Get session statistics. Aggregates over ALL session entries (including
    /// history that was compacted away), so token/cost totals reflect what was
    /// actually billed across the session.
    pub fn get_session_stats(&self) -> SessionStats {
        todo!("port core/agent-session.ts:AgentSession.getSessionStats")
    }

    pub fn get_context_usage(&self) -> Option<ContextUsage> {
        todo!("port core/agent-session.ts:AgentSession.getContextUsage")
    }

    /// Export session to HTML.
    /// @param outputPath Optional output path (defaults to session directory)
    /// @param options Optional export presentation settings
    /// @returns Path to exported file
    pub async fn export_to_html(
        &self,
        output_path: Option<&str>,
        options: Option<&AgentSessionExportToHtmlOptions>,
    ) -> anyhow::Result<String> {
        todo!("port core/agent-session.ts:AgentSession.exportToHtml")
    }

    /// Export the current session branch to a JSONL file.
    /// Writes the session header followed by all entries on the current branch path.
    /// @param outputPath Target file path. If omitted, generates a timestamped file in cwd.
    /// @returns The resolved output file path.
    pub fn export_to_jsonl(&self, output_path: Option<&str>) -> String {
        todo!("port core/agent-session.ts:AgentSession.exportToJsonl")
    }

    /// Ask the current model to describe what went wrong in this session for a bug report.
    /// Used when the user declines to share the transcript itself.
    pub async fn summarize_for_bug_report(
        &self,
        options: &AgentSessionSummarizeForBugReportOptions,
    ) -> anyhow::Result<String> {
        todo!("port core/agent-session.ts:AgentSession.summarizeForBugReport")
    }

    /// Get text content of last assistant message.
    /// Useful for /copy command.
    /// @returns Text content, or undefined if no assistant message exists
    pub fn get_last_assistant_text(&self) -> Option<String> {
        todo!("port core/agent-session.ts:AgentSession.getLastAssistantText")
    }

    pub fn create_replaced_session_context(&self) -> ReplacedSessionContext {
        todo!("port core/agent-session.ts:AgentSession.createReplacedSessionContext")
    }

    /// Check if extensions have handlers for a specific event type.
    pub fn has_extension_handlers(&self, event_type: &str) -> bool {
        todo!("port core/agent-session.ts:AgentSession.hasExtensionHandlers")
    }

    /// Get the extension runner (for setting UI context and error handlers).
    pub fn extension_runner(&self) -> Arc<ExtensionRunner> {
        todo!("port core/agent-session.ts:AgentSession.extensionRunner")
    }
}
