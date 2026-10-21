//! Port of `packages/coding-agent/src/core/extensions/types.ts`.

use crate::core::bash_executor::BashResult;
use crate::core::compaction::compaction::CompactionPreparation;
use crate::core::compaction::compaction::CompactionResult;
use crate::core::event_bus::EventBus;
use crate::core::footer_data_provider::ReadonlyFooterDataProvider;
use crate::core::mcp_servers::McpServerConfig;
use crate::core::mcp_servers::McpServerRegistry;
use crate::core::mcp_servers::RegisteredMcpServer;
use crate::core::messages::CustomMessage;
use crate::core::model_registry::ModelRegistry;
use crate::core::model_resolver::ScopedModel;
use crate::core::session_manager::BranchSummaryEntry;
use crate::core::session_manager::CompactionEntry;
use crate::core::session_manager::ContextEditableContent;
use crate::core::session_manager::CustomEntry;
use crate::core::session_manager::ProjectedSessionEntry;
use crate::core::session_manager::ReadonlySessionManager;
use crate::core::session_manager::SessionEntry;
use crate::core::session_manager::SessionManager;
use crate::core::settings_schema::Settings;
use crate::core::slash_commands::SlashCommandInfo;
use crate::core::source_info::SourceInfo;
use crate::core::tools::bash::BashOperations;
use crate::core::tools::bash::BashToolDetails;
use crate::core::tools::bash::BashToolInput;
use crate::core::tools::edit::EditToolDetails;
use crate::core::tools::edit::EditToolInput;
use crate::core::tools::find::FindToolDetails;
use crate::core::tools::find::FindToolInput;
use crate::core::tools::grep::GrepToolDetails;
use crate::core::tools::grep::GrepToolInput;
use crate::core::tools::ls::LsToolDetails;
use crate::core::tools::ls::LsToolInput;
use crate::core::tools::powershell::PowerShellToolDetails;
use crate::core::tools::powershell::PowerShellToolInput;
use crate::core::tools::read::ReadToolDetails;
use crate::core::tools::read::ReadToolInput;
use crate::core::tools::write::WriteToolInput;
use crate::core::virtual_models::ModelRoute;
use crate::core::virtual_models::ModelRouteRequest;
use crate::core::virtual_models::VirtualModelDefinition;
use crate::modes::interactive::components::status_indicator::CompactionStatusReason;
use crate::modes::interactive::theme::theme::Theme;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_agent::types::AgentMessage;
use pi_agent::types::AgentToolCallOutcome;
use pi_agent::types::AgentToolRef;
use pi_agent::types::AgentToolResult;
use pi_agent::types::AgentToolUpdateCallback;
use pi_agent::types::ThinkingLevel;
use pi_agent::types::ToolExecutionMode;
use pi_ai::auth::types::OAuthCredentials;
use pi_ai::models::Provider;
use pi_ai::models::ProviderClassifier;
use pi_ai::models::ProviderImages;
use pi_ai::models::RefreshModelsContext;
use pi_ai::providers::model_schema::ModelCost;
use pi_ai::providers::model_schema::ModelInputLimits;
use pi_ai::providers::model_schema::ModelPromptCache;
use pi_ai::providers::model_schema::ModelThinkingLevel;
use pi_ai::providers::model_schema::ThinkingLevelMap;
use pi_ai::types::Api;
use pi_ai::types::AssistantMessageEvent;
use pi_ai::types::ClassifierApi;
use pi_ai::types::GrammarVariants;
use pi_ai::types::ImageApi;
use pi_ai::types::ImageContent;
use pi_ai::types::JsonValue;
use pi_ai::types::Message;
use pi_ai::types::Model;
use pi_ai::types::ProviderHeaders;
use pi_ai::types::ProviderId;
use pi_ai::types::SimpleStreamOptions;
use pi_ai::types::ToolResultMessage;
use pi_ai::types::TranscriptContext;
use pi_ai::types::Usage;
use pi_ai::types::UserContent;
use pi_ai::types::UserContentBlock;
use pi_ai::utils::event_stream::AssistantMessageEventStream;
use pi_tui::tui::ComponentRef;
use pi_tui::tui::OverlayHandle;
use pi_tui::tui::OverlayOptions;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub use crate::core::cache_warmer::CacheWarmingDecisionEvent;
pub use crate::core::cache_warmer::CacheWarmingDecisionEventResult;
pub use crate::core::exec::ExecOptions;
pub use crate::core::exec::ExecResult;
pub use crate::core::keybindings::AppKeybinding;
pub use crate::core::keybindings::KeybindingsManager;
pub use crate::core::system_prompt::BuildSystemPromptOptions;
pub use crate::core::system_prompt::NormalizedBuildSystemPromptOptions;

/// Options for extension UI dialogs.
#[derive(Debug, Clone)]
pub struct ExtensionUIDialogOptions {
    /// AbortSignal to programmatically dismiss the dialog.
    pub signal: Option<CancellationToken>,
    /// Timeout in milliseconds. Dialog auto-dismisses with live countdown display.
    pub timeout: Option<i64>,
}

/// Placement for extension widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WidgetPlacement {
    #[serde(rename = "aboveEditor")]
    AboveEditor,
    #[serde(rename = "belowEditor")]
    BelowEditor,
}

impl WidgetPlacement {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AboveEditor => "aboveEditor",
            Self::BelowEditor => "belowEditor",
        }
    }
}

/// Options for extension widgets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionWidgetOptions {
    /// Where the widget is rendered. Defaults to "aboveEditor".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<WidgetPlacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInputHandlerResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consume: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

/// Raw terminal input listener for extensions.
pub type TerminalInputHandler =
    Arc<dyn Fn(String) -> Option<TerminalInputHandlerResult> + Send + Sync>;

/// Working indicator configuration for the interactive streaming loader.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingIndicatorOptions {
    /// Animation frames. Use an empty array to hide the indicator entirely. Custom frames are rendered verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<Vec<String>>,
    /// Frame interval in milliseconds for animated indicators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<i64>,
}

/// Wrap the current autocomplete provider with additional behavior.
pub type AutocompleteProviderFactory = Arc<
    dyn Fn(Value /* TS: pi-tui::AutocompleteProvider */) -> Value
        /* TS: pi-tui::AutocompleteProvider */
        + Send
        + Sync,
>;

pub type EditorFactory = Arc<
    dyn Fn(Arc<Tui>, Value /* TS: pi-tui::EditorTheme */, Arc<KeybindingsManager>) -> Value
        /* TS: pi-tui::EditorComponent */
        + Send
        + Sync,
>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExtensionUIContextType {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
}

impl ExtensionUIContextType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Clone)]
pub enum ExtensionUIContextOptionsOverlayOptions {
    Arc(Arc<OverlayOptions>),
    Fn(Arc<dyn Fn() -> Arc<OverlayOptions> + Send + Sync>),
}

#[derive(Clone)]
pub struct ExtensionUIContextOptions {
    pub overlay: Option<bool>,
    /// Overlay positioning/sizing options. Can be static or a function for dynamic updates.
    pub overlay_options: Option<ExtensionUIContextOptionsOverlayOptions>,
    /// Called with the overlay handle after the overlay is shown. Use to control visibility.
    pub on_handle: Option<Arc<dyn Fn(Arc<OverlayHandle>) + Send + Sync>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionUIContextGetAllThemesResult {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Clone)]
pub enum ExtensionUIContextTheme {
    String(String),
    Arc(Arc<Theme>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionUIContextSetThemeResult {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// UI context for extensions to request interactive UI.
/// Each mode (interactive, RPC, print) provides its own implementation.
#[async_trait::async_trait]
pub trait ExtensionUIContext: Send + Sync {
    /// Show a selector and return the user's choice.
    async fn select(
        &self,
        title: &str,
        options: &[String],
        opts: Option<&ExtensionUIDialogOptions>,
    ) -> anyhow::Result<Option<String>>;
    /// Show a confirmation dialog.
    async fn confirm(
        &self,
        title: &str,
        message: &str,
        opts: Option<&ExtensionUIDialogOptions>,
    ) -> anyhow::Result<bool>;
    /// Show a text input dialog.
    async fn input(
        &self,
        title: &str,
        placeholder: Option<&str>,
        opts: Option<&ExtensionUIDialogOptions>,
    ) -> anyhow::Result<Option<String>>;
    /// Show a notification to the user.
    fn notify(&self, message: &str, r#type: Option<ExtensionUIContextType>);
    /// Listen to raw terminal input (interactive mode only). Returns an unsubscribe function.
    fn on_terminal_input(&self, handler: &TerminalInputHandler) -> Arc<dyn Fn() + Send + Sync>;
    /// Set status text in the footer/status bar. Pass undefined to clear.
    fn set_status(&self, key: &str, text: Option<&str>);
    /// Set the working/loading message shown during streaming. Call with no argument to restore default.
    fn set_working_message(&self, message: Option<&str>);
    /// Show or hide the built-in interactive working loader row during streaming.
    fn set_working_visible(&self, visible: bool);
    /// Configure the interactive working indicator shown during streaming.
    ///
    /// - Omit the argument to restore the default animated spinner.
    /// - Use `frames: ["●"]` for a static indicator.
    /// - Use `frames: []` to hide the indicator entirely.
    /// - Custom frames are rendered as provided, so extensions must add their own colors.
    fn set_working_indicator(&self, options: Option<&WorkingIndicatorOptions>);
    /// Set the label shown for hidden thinking blocks. Call with no argument to restore default.
    fn set_hidden_thinking_label(&self, label: Option<&str>);
    /// Set a widget to display above or below the editor. Accepts string array or component factory.
    fn set_widget(
        &self,
        key: &str,
        content: Option<&[String]>,
        options: Option<&ExtensionWidgetOptions>,
    );
    /// Set a custom footer component, or undefined to restore the built-in footer.
    ///
    /// The factory receives a FooterDataProvider for data not otherwise accessible:
    /// git branch and extension statuses from setStatus(). Context usage is on
    /// ctx.getContextUsage(), token stats on ctx.sessionManager.getEntries(), model info on ctx.model.
    fn set_footer(
        &self,
        factory: Option<
            Arc<
                dyn Fn(Arc<Tui>, Arc<Theme>, ReadonlyFooterDataProvider) -> ComponentRef
                    + Send
                    + Sync,
            >,
        >,
    );
    /// Set a custom header component (shown at startup, above chat), or undefined to restore the built-in header.
    fn set_header(
        &self,
        factory: Option<Arc<dyn Fn(Arc<Tui>, Arc<Theme>) -> ComponentRef + Send + Sync>>,
    );
    /// Set the terminal window/tab title.
    fn set_title(&self, title: &str);
    /// Show a custom component with keyboard focus.
    async fn custom(
        &self,
        factory: Arc<
            dyn Fn(
                    Arc<Tui>,
                    Arc<Theme>,
                    Arc<KeybindingsManager>,
                    Arc<dyn Fn(Value) + Send + Sync>,
                ) -> BoxFuture<'static, anyhow::Result<ComponentRef>>
                + Send
                + Sync,
        >,
        options: Option<&ExtensionUIContextOptions>,
    ) -> anyhow::Result<Value>;
    /// Paste text into the editor, triggering paste handling (collapse for large content).
    fn paste_to_editor(&self, text: &str);
    /// Set the text in the core input editor.
    fn set_editor_text(&self, text: &str);
    /// Get the current text from the core input editor.
    fn get_editor_text(&self) -> String;
    /// Show a multi-line editor for text editing.
    async fn editor(&self, title: &str, prefill: Option<&str>) -> anyhow::Result<Option<String>>;
    /// Stack additional autocomplete behavior on top of the built-in provider.
    fn add_autocomplete_provider(&self, factory: &AutocompleteProviderFactory);
    /// Set a custom editor component via factory function.
    /// Pass undefined to restore the default editor.
    ///
    /// The factory receives:
    /// - `theme`: EditorTheme for styling borders and autocomplete
    /// - `keybindings`: KeybindingsManager for app-level keybindings
    ///
    /// For full app keybinding support (escape, ctrl+d, model switching, etc.),
    /// extend `CustomEditor` from `@earendil-works/pi-coding-agent` and call
    /// `super.handleInput(data)` for keys you don't handle.
    ///
    /// @example
    /// ```text
    /// import { CustomEditor } from "@earendil-works/pi-coding-agent";
    ///
    /// class VimEditor extends CustomEditor {
    ///   private mode: "normal" | "insert" = "insert";
    ///
    ///   handleInput(data: string): void {
    ///     if (this.mode === "normal") {
    ///       // Handle vim normal mode keys...
    ///       if (data === "i") { this.mode = "insert"; return; }
    ///     }
    ///     super.handleInput(data);  // App keybindings + text editing
    ///   }
    /// }
    ///
    /// ctx.ui.setEditorComponent((tui, theme, keybindings) =>
    ///   new VimEditor(tui, theme, keybindings)
    /// );
    /// ```
    fn set_editor_component(&self, factory: Option<&EditorFactory>);
    /// Get the currently configured custom editor factory, or undefined when using the default editor.
    fn get_editor_component(&self) -> Option<EditorFactory>;
    /// Get the current theme for styling.
    fn theme(&self) -> Arc<Theme>;
    /// Get all available themes with their names and file paths.
    fn get_all_themes(&self) -> Vec<ExtensionUIContextGetAllThemesResult>;
    /// Load a theme by name without switching to it. Returns undefined if not found.
    fn get_theme(&self, name: &str) -> Option<Arc<Theme>>;
    /// Set the current theme by name or Theme object.
    fn set_theme(&self, theme: &ExtensionUIContextTheme) -> ExtensionUIContextSetThemeResult;
    /// Get current tool output expansion state.
    fn get_tools_expanded(&self) -> bool;
    /// Set tool output expansion state.
    fn set_tools_expanded(&self, expanded: bool);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    /// Estimated context tokens, or null if unknown (e.g. right after compaction, before next LLM response).
    #[serde(default)]
    pub tokens: Option<u64>,
    pub context_window: u64,
    /// Context usage as percentage of context window, or null if tokens is unknown.
    #[serde(default, with = "pi_ai::serde_util::opt_js_f64")]
    pub percent: Option<f64>,
}

#[derive(Clone)]
pub struct CompactOptions {
    pub custom_instructions: Option<String>,
    pub on_complete: Option<Arc<dyn Fn(CompactionResult) + Send + Sync>>,
    pub on_error: Option<Arc<dyn Fn(Arc<anyhow::Error>) + Send + Sync>>,
}

/// Context passed to extension event handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExtensionMode {
    #[serde(rename = "tui")]
    Tui,
    #[serde(rename = "rpc")]
    Rpc,
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "print")]
    Print,
}

impl ExtensionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tui => "tui",
            Self::Rpc => "rpc",
            Self::Json => "json",
            Self::Print => "print",
        }
    }
}

pub trait ExtensionContext: Send + Sync {
    /// UI methods for user interaction
    fn ui(&self) -> Arc<dyn ExtensionUIContext>;
    fn set_ui(&self, value: Arc<dyn ExtensionUIContext>);
    /// Current run mode. Use "tui" to guard terminal-only UI such as custom components.
    fn mode(&self) -> ExtensionMode;
    fn set_mode(&self, value: ExtensionMode);
    /// Whether dialog-capable UI is available (true in TUI and RPC modes)
    fn has_ui(&self) -> bool;
    fn set_has_ui(&self, value: bool);
    /// Current working directory
    fn cwd(&self) -> String;
    fn set_cwd(&self, value: String);
    /// Session manager (read-only)
    fn session_manager(&self) -> ReadonlySessionManager;
    fn set_session_manager(&self, value: ReadonlySessionManager);
    /// Model registry for API key resolution
    fn model_registry(&self) -> Arc<ModelRegistry>;
    fn set_model_registry(&self, value: Arc<ModelRegistry>);
    /// Current model (may be undefined)
    fn model(&self) -> Option<Model>;
    fn set_model(&self, value: Option<Model>);
    /// Models scoped to this session (resolved from `--models` /
    /// `enabledModels` settings against the available catalogue). Same set
    /// the `/scoped-models` command shows. Empty when no scoping is
    /// configured (all available models are usable). Read-only snapshot.
    fn scoped_models(&self) -> Vec<ScopedModel>;
    fn set_scoped_models(&self, value: Vec<ScopedModel>);
    /// Current thinking level, when provided by the session runtime.
    /// Optional in TS: the default implementation means "not provided".
    fn thinking_level(&self) -> Option<ThinkingLevel> {
        None
    }
    fn set_thinking_level(&self, value: Option<ThinkingLevel>);
    /// Whether the agent is idle (not streaming)
    fn is_idle(&self) -> bool;
    /// Whether project-local trust is active for this context.
    fn is_project_trusted(&self) -> bool;
    /// The current abort signal, or undefined when the agent is not streaming.
    fn signal(&self) -> Option<CancellationToken>;
    fn set_signal(&self, value: Option<CancellationToken>);
    /// Abort the current agent operation
    fn abort(&self);
    /// Whether there are queued messages waiting
    fn has_pending_messages(&self) -> bool;
    /// Gracefully shutdown pi and exit. Available in all contexts.
    fn shutdown(&self);
    /// Get current context usage for the active model.
    fn get_context_usage(&self) -> Option<ContextUsage>;
    /// Trigger compaction without awaiting completion.
    fn compact(&self, options: Option<&CompactOptions>);
    /// Get the current effective system prompt.
    fn get_system_prompt(&self) -> String;
}

/// Options for {@link ExtensionToolContext.executeTool}.
#[derive(Clone)]
pub struct ExecuteToolOptions {
    /// Defaults to the calling tool's signal.
    pub signal: Option<CancellationToken>,
    /// Receives partial results of the nested tool, in addition to `tool_execution_update` events.
    pub on_update: Option<AgentToolUpdateCallback>,
}

/// Context passed to tool `execute()` in a session: the extension context plus `executeTool()`
/// for running other tools through the same validation, hooks, and permission checks as
/// model-issued calls.
///
/// A tool wrapped with `wrapToolDefinition()` without a context factory, such as a built-in tool
/// created with `createBashTool()` and run in a plain `Agent` or called directly, gets no context.
#[async_trait::async_trait]
pub trait ExtensionToolContext: ExtensionContext + Send + Sync {
    /// Tools {@link executeTool} can call.
    fn tools(&self) -> Vec<AgentToolRef>;
    /// Run another tool. The call gets the id `<calling id>/<n>`, and the `tool_call`, `tool_result`,
    /// and `tool_execution_*` events carry `parentToolCallId`. It does not appear in the transcript;
    /// a bounded record of it is kept as `nestedCalls` on the calling tool's result message.
    ///
    /// Never rejects for tool failures: unknown tools, validation errors, blocked calls, and thrown
    /// errors come back as `isError: true`.
    async fn execute_tool(
        &self,
        name: &str,
        args: Value,
        options: Option<&ExecuteToolOptions>,
    ) -> anyhow::Result<AgentToolCallOutcome>;
}

#[derive(Clone)]
pub struct ExtensionCommandContextOptions {
    pub parent_session: Option<String>,
    pub setup: Option<
        Arc<dyn Fn(Arc<SessionManager>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
    pub with_session: Option<
        Arc<
            dyn Fn(Arc<dyn ReplacedSessionContext>) -> BoxFuture<'static, anyhow::Result<()>>
                + Send
                + Sync,
        >,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommandContextNewSessionResult {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExtensionCommandContextOptions2Position {
    #[serde(rename = "before")]
    Before,
    #[serde(rename = "at")]
    At,
}

impl ExtensionCommandContextOptions2Position {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::At => "at",
        }
    }
}

#[derive(Clone)]
pub struct ExtensionCommandContextOptions2 {
    pub position: Option<ExtensionCommandContextOptions2Position>,
    pub with_session: Option<
        Arc<
            dyn Fn(Arc<dyn ReplacedSessionContext>) -> BoxFuture<'static, anyhow::Result<()>>
                + Send
                + Sync,
        >,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommandContextForkResult {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommandContextOptions3 {
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
pub struct ExtensionCommandContextNavigateTreeResult {
    pub cancelled: bool,
}

#[derive(Clone)]
pub struct ExtensionCommandContextOptions4 {
    pub with_session: Option<
        Arc<
            dyn Fn(Arc<dyn ReplacedSessionContext>) -> BoxFuture<'static, anyhow::Result<()>>
                + Send
                + Sync,
        >,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommandContextSwitchSessionResult {
    pub cancelled: bool,
}

/// Extended context for command handlers.
/// Includes session control methods only safe in user-initiated commands.
#[async_trait::async_trait]
pub trait ExtensionCommandContext: ExtensionContext + Send + Sync {
    /// Get the current base system-prompt construction options.
    fn get_system_prompt_options(&self) -> BuildSystemPromptOptions;
    /// Wait for the agent to finish streaming
    async fn wait_for_idle(&self) -> anyhow::Result<()>;
    /// Start a new session, optionally with initialization.
    async fn new_session(
        &self,
        options: Option<&ExtensionCommandContextOptions>,
    ) -> anyhow::Result<ExtensionCommandContextNewSessionResult>;
    /// Fork from a specific entry, creating a new session file.
    async fn fork(
        &self,
        entry_id: &str,
        options: Option<&ExtensionCommandContextOptions2>,
    ) -> anyhow::Result<ExtensionCommandContextForkResult>;
    /// Navigate to a different point in the session tree.
    async fn navigate_tree(
        &self,
        target_id: &str,
        options: Option<&ExtensionCommandContextOptions3>,
    ) -> anyhow::Result<ExtensionCommandContextNavigateTreeResult>;
    /// Switch to a different session file.
    async fn switch_session(
        &self,
        session_path: &str,
        options: Option<&ExtensionCommandContextOptions4>,
    ) -> anyhow::Result<ExtensionCommandContextSwitchSessionResult>;
    /// Reload extensions, skills, prompts, themes, and context files.
    async fn reload(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacedSessionContextMessage {
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReplacedSessionContextOptionsDeliverAs {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
    #[serde(rename = "nextTurn")]
    NextTurn,
}

impl ReplacedSessionContextOptionsDeliverAs {
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
pub struct ReplacedSessionContextOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_turn: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliver_as: Option<ReplacedSessionContextOptionsDeliverAs>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReplacedSessionContextOptions2DeliverAs {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

impl ReplacedSessionContextOptions2DeliverAs {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::FollowUp => "followUp",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacedSessionContextOptions2 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliver_as: Option<ReplacedSessionContextOptions2DeliverAs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand_prompt_templates: Option<bool>,
}

/// Fresh command-capable context bound to the replacement session after a session switch.
///
/// This is passed to `withSession()` callbacks on `newSession()`, `fork()`, and `switchSession()`.
#[async_trait::async_trait]
pub trait ReplacedSessionContext: ExtensionCommandContext + Send + Sync {
    async fn send_message(
        &self,
        message: &ReplacedSessionContextMessage,
        options: Option<&ReplacedSessionContextOptions>,
    ) -> anyhow::Result<()>;
    async fn send_user_message(
        &self,
        content: UserContent,
        options: Option<&ReplacedSessionContextOptions2>,
    ) -> anyhow::Result<()>;
}

/// Rendering options for tool results
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRenderResultOptions {
    /// Whether the result view is expanded
    pub expanded: bool,
    /// Whether this is a partial/streaming result
    pub is_partial: bool,
}

/// Context passed to tool renderers.
#[derive(Clone)]
pub struct ToolRenderContext {
    /// Current tool call arguments. Shared across call/result renders for the same tool call.
    pub args: Value,
    /// Unique id for this tool execution. Stable across call/result renders for the same tool call.
    pub tool_call_id: String,
    /// Invalidate just this tool execution component for redraw.
    pub invalidate: Arc<dyn Fn() + Send + Sync>,
    /// Previously returned component for this render slot, if any.
    pub last_component: Option<ComponentRef>,
    /// Shared renderer state for this tool row. Initialized by tool-execution.ts.
    pub state: Value,
    /// Working directory for this tool execution.
    pub cwd: String,
    /// Whether the tool execution has started.
    pub execution_started: bool,
    /// Whether the tool call arguments are complete.
    pub args_complete: bool,
    /// Whether the tool result is partial/streaming.
    pub is_partial: bool,
    /// Whether the result view is expanded.
    pub expanded: bool,
    /// Whether inline images are currently shown in the TUI.
    pub show_images: bool,
    /// Whether the current result is an error.
    pub is_error: bool,
    /// Milliseconds the tool's execution took, from the final result; `undefined` while it runs, when it did not run, or
    /// for results stored before durations were recorded.
    pub duration_ms: Option<f64>,
    /// Horizontal padding configured by the outputPad setting. Renderers with `renderShell: "self"` apply it themselves.
    pub output_pad: i64,
}

/// How the model reaches a tool. "Callable" means callable from other tools through
/// `ctx.executeTool()`, as the `codemode` tool does.
///
/// - `direct`: declared to the model while active, and callable while active.
/// - `model-only`: declared to the model while active, never callable. Use it for orchestrating or
/// interactive tools.
/// - `codemode`: callable whenever registered. Not declared to the model unless explicitly
/// activated. Codemode tools list it in their description.
/// - `deferred`: like `codemode`, but codemode tools do not list it; tool search can find it.
/// - `hidden`: registered but unreachable. Activating it has no effect.
///
/// `direct` and `model-only` tools are activated when they are registered; the others are not.
/// The active tool set (`getActiveTools`/`setActiveTools`) is the set declared to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolExposure {
    #[serde(rename = "direct")]
    Direct,
    #[serde(rename = "model-only")]
    ModelOnly,
    #[serde(rename = "codemode")]
    Codemode,
    #[serde(rename = "deferred")]
    Deferred,
    #[serde(rename = "hidden")]
    Hidden,
}

impl ToolExposure {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::ModelOnly => "model-only",
            Self::Codemode => "codemode",
            Self::Deferred => "deferred",
            Self::Hidden => "hidden",
        }
    }
}

/// Hints about what a tool does, with the meaning of MCP tool annotations. They come from the tool's
/// author and are not verified; permission extensions can use them to decide which calls to confirm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    /// The tool does not modify its environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// The tool may delete or overwrite data, rather than only add to it. Meaningful when not read-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// Repeating a call with the same arguments has no further effect. Meaningful when not read-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    /// The tool reaches an open world of external entities, such as the web, rather than a closed domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

/// A group of related tools, such as the tools of one MCP server. Codemode tools list them together.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolNamespace {
    /// For example `mcp__docs`.
    pub name: String,
    /// Other names that refer to the namespace where tools look it up by name (codemode's
    /// `describeNamespace()` and the `namespace` option of `searchTools()`), for example `docs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    /// Short summary shown once with the group in model-facing tool listings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Longer usage guidance, such as MCP server instructions. Not part of tool listings; tools that
    /// describe the namespace on request (codemode's `describeNamespace()`) return it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// The tools of a session as {@link ToolDefinition.prepareLoadout} sees them.
pub trait ToolLoadout: Send + Sync {
    /// Tools declared to the model (the active tools), in order, with their original descriptions.
    fn declared(&self) -> Vec<AgentToolRef>;
    /// Tools callable through `ctx.executeTool()`.
    fn callable(&self) -> Vec<AgentToolRef>;
    /// Every registered tool.
    fn registered(&self) -> Vec<AgentToolRef>;
    fn get_exposure(&self, name: &str) -> ToolExposure;
    fn get_namespace(&self, name: &str) -> Option<ToolNamespace>;
    /// A tool's `promptGuidelines`. Hidden declarations leave them out of the system prompt.
    fn get_prompt_guidelines(&self, name: &str) -> Vec<String>;
}

/// Changes {@link ToolDefinition.prepareLoadout} makes to what the model sees.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLoadoutChanges {
    /// Model-facing descriptions of declared tools, by tool name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptions: Option<IndexMap<String, String>>,
    /// Declared tools whose declarations requests leave out. They stay active and callable, and the
    /// transcript still declares them, so the active set survives `/tree` and resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_declarations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolDefinitionConstrainedSamplingVariant2Strict {
    #[serde(rename = "prefer")]
    Prefer,
    #[serde(rename = "require")]
    Require,
}

impl ToolDefinitionConstrainedSamplingVariant2Strict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Prefer => "prefer",
            Self::Require => "require",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinitionConstrainedSamplingVariant2 {
    /// Always "json_schema".
    pub r#type: String,
    pub strict: ToolDefinitionConstrainedSamplingVariant2Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinitionConstrainedSamplingVariant3 {
    /// Always "grammar".
    pub r#type: String,
    pub variants: GrammarVariants,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolDefinitionConstrainedSampling {
    Bool(bool),
    ToolDefinitionConstrainedSamplingVariant2(ToolDefinitionConstrainedSamplingVariant2),
    ToolDefinitionConstrainedSamplingVariant3(ToolDefinitionConstrainedSamplingVariant3),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolDefinitionRenderShell {
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "self")]
    SelfValue,
}

impl ToolDefinitionRenderShell {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::SelfValue => "self",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolDefinitionResult {
    String(String),
    List(Vec<String>),
}

/// Tool definition for registerTool().
#[async_trait::async_trait]
pub trait ToolDefinition: Send + Sync {
    /// Tool name (used in LLM tool calls)
    fn name(&self) -> String;
    fn set_name(&self, value: String);
    /// Human-readable label for UI
    fn label(&self) -> String;
    fn set_label(&self, value: String);
    /// Description for LLM
    fn description(&self) -> String;
    fn set_description(&self, value: String);
    /// Optional one-line snippet for the Available tools section in the default system prompt. Custom tools are omitted from that section when this is not provided.
    /// Optional in TS: the default implementation means "not provided".
    fn prompt_snippet(&self) -> Option<String> {
        None
    }
    fn set_prompt_snippet(&self, value: Option<String>);
    /// Optional guideline bullets appended to the default system prompt Guidelines section when this tool is active.
    /// Optional in TS: the default implementation means "not provided".
    fn prompt_guidelines(&self) -> Option<Vec<String>> {
        None
    }
    fn set_prompt_guidelines(&self, value: Option<Vec<String>>);
    /// Parameter schema (TypeBox)
    fn parameters(&self) -> Value;
    fn set_parameters(&self, value: Value);
    /// Optional provider-side constrained sampling request for this tool. Set false to explicitly disable it, equivalent to leaving it undefined.
    /// Optional in TS: the default implementation means "not provided".
    fn constrained_sampling(&self) -> Option<ToolDefinitionConstrainedSampling> {
        None
    }
    fn set_constrained_sampling(&self, value: Option<ToolDefinitionConstrainedSampling>);
    /// Controls whether ToolExecutionComponent renders the standard colored shell or the tool renders its own framing.
    /// Optional in TS: the default implementation means "not provided".
    fn render_shell(&self) -> Option<ToolDefinitionRenderShell> {
        None
    }
    fn set_render_shell(&self, value: Option<ToolDefinitionRenderShell>);
    /// Optional compatibility shim to prepare raw tool call arguments before schema validation. Must return an object conforming to TParams.
    /// Optional in TS: the default implementation means "not provided".
    fn prepare_arguments(&self) -> Option<Arc<dyn Fn(Value) -> Value + Send + Sync>> {
        None
    }
    /// JSON Schema of `structuredContent` in successful results. Tools that declare it should always
    /// set `structuredContent`; codemode scripts then receive it instead of the text content.
    /// Optional in TS: the default implementation means "not provided".
    fn output_schema(&self) -> Option<Value> {
        None
    }
    fn set_output_schema(&self, value: Option<Value>);
    /// How the model reaches the tool. Default: `"direct"`. See {@link ToolExposure}.
    /// Optional in TS: the default implementation means "not provided".
    fn exposure(&self) -> Option<ToolExposure> {
        None
    }
    fn set_exposure(&self, value: Option<ToolExposure>);
    /// Group the tool belongs to, for example its MCP server.
    /// Optional in TS: the default implementation means "not provided".
    fn namespace(&self) -> Option<ToolNamespace> {
        None
    }
    fn set_namespace(&self, value: Option<ToolNamespace>);
    /// For tools that reach other tools (such as `codemode`, which calls them, and `tool_search`, which
    /// loads them): which tools a call with `input` may reach. `"all"`, or the words of the input that
    /// may name them, where a word refers to a tool or namespace whose name it contains, so words may
    /// over-match. Extensions that register tools late, such as MCP servers that are still connecting,
    /// use it from their `tool_call` handler to finish registering those tools before the call runs.
    /// Optional in TS: the default implementation means "not provided".
    fn reaches_tools(
        &self,
    ) -> Option<Arc<dyn Fn(IndexMap<String, Value>) -> ToolDefinitionResult + Send + Sync>> {
        None
    }
    /// Hints about what the tool does, for example from an MCP server.
    /// Optional in TS: the default implementation means "not provided".
    fn annotations(&self) -> Option<ToolAnnotations> {
        None
    }
    fn set_annotations(&self, value: Option<ToolAnnotations>);
    /// Whether registering the tool activates it. Default: `true` for `direct` and `model-only` tools;
    /// other exposures are never activated on registration. A tool with `defaultActive: false` is
    /// activated by naming it in `--tools` or the `defaultTools` setting, or with `setActiveTools()`.
    /// Optional in TS: the default implementation means "not provided".
    fn default_active(&self) -> Option<bool> {
        None
    }
    fn set_default_active(&self, value: Option<bool>);
    /// Adjust how the loadout is presented to the model while this tool is active. Called whenever
    /// the active tools change. Tools that orchestrate other tools use it, for example to list the
    /// callable tools in their own description.
    /// Optional in TS: the default implementation means "not provided".
    fn prepare_loadout(
        &self,
    ) -> Option<Arc<dyn Fn(Arc<dyn ToolLoadout>) -> Option<ToolLoadoutChanges> + Send + Sync>> {
        None
    }
    /// Per-tool execution mode override.
    /// - "sequential": this tool must execute one at a time with other tool calls.
    /// - "parallel": this tool can execute concurrently with other tool calls.
    ///
    /// If omitted, the default execution mode applies.
    /// Optional in TS: the default implementation means "not provided".
    fn execution_mode(&self) -> Option<ToolExecutionMode> {
        None
    }
    fn set_execution_mode(&self, value: Option<ToolExecutionMode>);
    /// Execute the tool.
    async fn execute(
        &self,
        tool_call_id: &str,
        params: Value,
        signal: Option<CancellationToken>,
        on_update: Option<AgentToolUpdateCallback>,
        ctx: Arc<dyn ExtensionToolContext>,
    ) -> anyhow::Result<AgentToolResult>;
    /// Custom rendering for tool call display
    /// Optional in TS: the default implementation means "not provided".
    fn render_call(
        &self,
    ) -> Option<Arc<dyn Fn(Value, Arc<Theme>, ToolRenderContext) -> ComponentRef + Send + Sync>>
    {
        None
    }
    /// Custom rendering for tool result display
    /// Optional in TS: the default implementation means "not provided".
    fn render_result(
        &self,
    ) -> Option<
        Arc<
            dyn Fn(
                    AgentToolResult,
                    ToolRenderResultOptions,
                    Arc<Theme>,
                    ToolRenderContext,
                ) -> ComponentRef
                + Send
                + Sync,
        >,
    > {
        None
    }
}

pub type AnyToolDefinition = Arc<dyn ToolDefinition>;

// TS: Pick<AnyToolDefinition, "renderShell" | "renderCall" | "renderResult">
#[derive(Clone)]
pub struct ToolRenderers {
    /// Controls whether ToolExecutionComponent renders the standard colored shell or the tool renders its own framing.
    pub render_shell: Option<ToolDefinitionRenderShell>,
    /// Custom rendering for tool call display
    pub render_call:
        Option<Arc<dyn Fn(Value, Arc<Theme>, ToolRenderContext) -> ComponentRef + Send + Sync>>,
    /// Custom rendering for tool result display
    pub render_result: Option<
        Arc<
            dyn Fn(
                    AgentToolResult,
                    ToolRenderResultOptions,
                    Arc<Theme>,
                    ToolRenderContext,
                ) -> ComponentRef
                + Send
                + Sync,
        >,
    >,
}

/// Chooses how calls to a tool are drawn, including tools that are not registered. `next()` returns
/// the renderers the remaining resolvers, then the registered tool, would use.
pub type ToolRendererResolver = Arc<
    dyn Fn(String, Arc<dyn Fn() -> Option<ToolRenderers> + Send + Sync>) -> Option<ToolRenderers>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefineToolDefineToolResultConstrainedSamplingVariant2 {
    /// Always "json_schema".
    pub r#type: String,
    pub strict: ToolDefinitionConstrainedSamplingVariant2Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefineToolDefineToolResultConstrainedSamplingVariant3 {
    /// Always "grammar".
    pub r#type: String,
    pub variants: GrammarVariants,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DefineToolDefineToolResultConstrainedSampling {
    Bool(bool),
    DefineToolDefineToolResultConstrainedSamplingVariant2(
        DefineToolDefineToolResultConstrainedSamplingVariant2,
    ),
    DefineToolDefineToolResultConstrainedSamplingVariant3(
        DefineToolDefineToolResultConstrainedSamplingVariant3,
    ),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DefineToolDefineToolResultResult {
    String(String),
    List(Vec<String>),
}

#[derive(Clone)]
pub struct DefineToolDefineToolResult {
    /// Tool name (used in LLM tool calls)
    pub name: String,
    /// Human-readable label for UI
    pub label: String,
    /// Description for LLM
    pub description: String,
    /// Optional one-line snippet for the Available tools section in the default system prompt. Custom tools are omitted from that section when this is not provided.
    pub prompt_snippet: Option<String>,
    /// Optional guideline bullets appended to the default system prompt Guidelines section when this tool is active.
    pub prompt_guidelines: Option<Vec<String>>,
    /// Parameter schema (TypeBox)
    pub parameters: Value,
    /// Optional provider-side constrained sampling request for this tool. Set false to explicitly disable it, equivalent to leaving it undefined.
    pub constrained_sampling: Option<DefineToolDefineToolResultConstrainedSampling>,
    /// Controls whether ToolExecutionComponent renders the standard colored shell or the tool renders its own framing.
    pub render_shell: Option<ToolDefinitionRenderShell>,
    /// Optional compatibility shim to prepare raw tool call arguments before schema validation. Must return an object conforming to TParams.
    pub prepare_arguments: Option<Arc<dyn Fn(Value) -> Value + Send + Sync>>,
    /// JSON Schema of `structuredContent` in successful results. Tools that declare it should always
    /// set `structuredContent`; codemode scripts then receive it instead of the text content.
    pub output_schema: Option<Value>,
    /// How the model reaches the tool. Default: `"direct"`. See {@link ToolExposure}.
    pub exposure: Option<ToolExposure>,
    /// Group the tool belongs to, for example its MCP server.
    pub namespace: Option<ToolNamespace>,
    /// For tools that reach other tools (such as `codemode`, which calls them, and `tool_search`, which
    /// loads them): which tools a call with `input` may reach. `"all"`, or the words of the input that
    /// may name them, where a word refers to a tool or namespace whose name it contains, so words may
    /// over-match. Extensions that register tools late, such as MCP servers that are still connecting,
    /// use it from their `tool_call` handler to finish registering those tools before the call runs.
    pub reaches_tools: Option<
        Arc<dyn Fn(IndexMap<String, Value>) -> DefineToolDefineToolResultResult + Send + Sync>,
    >,
    /// Hints about what the tool does, for example from an MCP server.
    pub annotations: Option<ToolAnnotations>,
    /// Whether registering the tool activates it. Default: `true` for `direct` and `model-only` tools;
    /// other exposures are never activated on registration. A tool with `defaultActive: false` is
    /// activated by naming it in `--tools` or the `defaultTools` setting, or with `setActiveTools()`.
    pub default_active: Option<bool>,
    /// Adjust how the loadout is presented to the model while this tool is active. Called whenever
    /// the active tools change. Tools that orchestrate other tools use it, for example to list the
    /// callable tools in their own description.
    pub prepare_loadout:
        Option<Arc<dyn Fn(Arc<dyn ToolLoadout>) -> Option<ToolLoadoutChanges> + Send + Sync>>,
    /// Per-tool execution mode override.
    /// - "sequential": this tool must execute one at a time with other tool calls.
    /// - "parallel": this tool can execute concurrently with other tool calls.
    ///
    /// If omitted, the default execution mode applies.
    pub execution_mode: Option<ToolExecutionMode>,
    /// Execute the tool.
    pub execute: Arc<
        dyn Fn(
                String,
                Value,
                Option<CancellationToken>,
                Option<AgentToolUpdateCallback>,
                Arc<dyn ExtensionToolContext>,
            ) -> BoxFuture<'static, anyhow::Result<AgentToolResult>>
            + Send
            + Sync,
    >,
    /// Custom rendering for tool call display
    pub render_call:
        Option<Arc<dyn Fn(Value, Arc<Theme>, ToolRenderContext) -> ComponentRef + Send + Sync>>,
    /// Custom rendering for tool result display
    pub render_result: Option<
        Arc<
            dyn Fn(
                    AgentToolResult,
                    ToolRenderResultOptions,
                    Arc<Theme>,
                    ToolRenderContext,
                ) -> ComponentRef
                + Send
                + Sync,
        >,
    >,
}

/// Preserve parameter inference for standalone tool definitions.
///
/// Use this when assigning a tool to a variable or passing it through arrays such
/// as `customTools`, where contextual typing would otherwise widen params to
/// `unknown`.
pub fn define_tool(tool: Arc<dyn ToolDefinition>) -> DefineToolDefineToolResult {
    todo!("port core/extensions/types.ts:defineTool")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrustEvent {
    /// Always "project_trust".
    pub r#type: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProjectTrustEventDecision {
    #[serde(rename = "yes")]
    Yes,
    #[serde(rename = "no")]
    No,
    #[serde(rename = "undecided")]
    Undecided,
}

impl ProjectTrustEventDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Undecided => "undecided",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrustEventResult {
    pub trusted: ProjectTrustEventDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remember: Option<bool>,
}

#[derive(Clone)]
pub struct ProjectTrustContextUi {
    /// Show a text input dialog.
    pub input: Arc<
        dyn Fn(
                String,
                Option<String>,
                Option<ExtensionUIDialogOptions>,
            ) -> BoxFuture<'static, anyhow::Result<Option<String>>>
            + Send
            + Sync,
    >,
    /// Show a selector and return the user's choice.
    pub select: Arc<
        dyn Fn(
                String,
                Vec<String>,
                Option<ExtensionUIDialogOptions>,
            ) -> BoxFuture<'static, anyhow::Result<Option<String>>>
            + Send
            + Sync,
    >,
    /// Show a confirmation dialog.
    pub confirm: Arc<
        dyn Fn(
                String,
                String,
                Option<ExtensionUIDialogOptions>,
            ) -> BoxFuture<'static, anyhow::Result<bool>>
            + Send
            + Sync,
    >,
    /// Show a notification to the user.
    pub notify: Arc<dyn Fn(String, Option<ExtensionUIContextType>) + Send + Sync>,
}

#[derive(Clone)]
pub struct ProjectTrustContext {
    pub cwd: String,
    pub mode: ExtensionMode,
    pub has_ui: bool,
    pub ui: ProjectTrustContextUi,
}

pub type ProjectTrustHandler = Arc<
    dyn Fn(
            ProjectTrustEvent,
            ProjectTrustContext,
        ) -> BoxFuture<'static, anyhow::Result<ProjectTrustEventResult>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourcesDiscoverEventReason {
    #[serde(rename = "startup")]
    Startup,
    #[serde(rename = "reload")]
    Reload,
}

impl ResourcesDiscoverEventReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Reload => "reload",
        }
    }
}

/// Fired after session_start to allow extensions to provide additional resource paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesDiscoverEvent {
    /// Always "resources_discover".
    pub r#type: String,
    pub cwd: String,
    pub reason: ResourcesDiscoverEventReason,
}

/// Result from resources_discover event handler
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesDiscoverResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_paths: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_paths: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_paths: Option<Vec<String>>,
}

/// Fired when an extension registers or unregisters an MCP server after the extensions are bound
/// (see {@link ExtensionAPI.registerMcpServer}). Servers registered while extensions load are read
/// with `pi.getMcpServers()` on `session_start`. Handling this event marks an extension as the one
/// that connects registered servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServersChangeEvent {
    /// Always "mcp_servers_change".
    pub r#type: String,
    /// Every registered server after the change.
    pub servers: Vec<RegisteredMcpServer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionStartEventReason {
    #[serde(rename = "startup")]
    Startup,
    #[serde(rename = "reload")]
    Reload,
    #[serde(rename = "new")]
    New,
    #[serde(rename = "resume")]
    Resume,
    #[serde(rename = "fork")]
    Fork,
}

impl SessionStartEventReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Reload => "reload",
            Self::New => "new",
            Self::Resume => "resume",
            Self::Fork => "fork",
        }
    }
}

/// Fired when a session is started, loaded, or reloaded
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStartEvent {
    /// Why this session start happened.
    pub reason: SessionStartEventReason,
    /// Previously active session file. Present for "new", "resume", and "fork".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_session_file: Option<String>,
}

/// Fired when the current session metadata changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfoChangedEvent {
    /// Current normalized session name. Undefined when the name is cleared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionBeforeSwitchEventReason {
    #[serde(rename = "new")]
    New,
    #[serde(rename = "resume")]
    Resume,
}

impl SessionBeforeSwitchEventReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Resume => "resume",
        }
    }
}

/// Fired before switching to another session (can be cancelled)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBeforeSwitchEvent {
    pub reason: SessionBeforeSwitchEventReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_session_file: Option<String>,
}

/// Fired before forking a session (can be cancelled)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBeforeForkEvent {
    pub entry_id: String,
    pub position: ExtensionCommandContextOptions2Position,
}

/// Fired before context compaction (can be cancelled or customized)
#[derive(Debug, Clone)]
pub struct SessionBeforeCompactEvent {
    pub preparation: CompactionPreparation,
    pub branch_entries: Vec<SessionEntry>,
    pub custom_instructions: Option<String>,
    /// What triggered the compaction: manual /compact, the context threshold, or context overflow recovery
    pub reason: CompactionStatusReason,
    /// True when the aborted turn is retried after this compaction (overflow recovery)
    pub will_retry: bool,
    pub signal: CancellationToken,
}

/// Fired after context compaction succeeds
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCompactEvent {
    pub compaction_entry: CompactionEntry,
    pub from_extension: bool,
    /// What triggered the compaction: manual /compact, the context threshold, or context overflow recovery
    pub reason: CompactionStatusReason,
    /// True when the aborted turn is retried after this compaction (overflow recovery)
    pub will_retry: bool,
}

/// Fired after context compaction fails or is aborted
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCompactFailedEvent {
    /// What triggered the compaction: manual /compact, the context threshold, or context overflow recovery
    pub reason: CompactionStatusReason,
    /// Error text when compaction failed for a non-abort reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// True when compaction was cancelled or aborted.
    pub aborted: bool,
    /// True when the aborted turn would have been retried after this compaction (overflow recovery)
    pub will_retry: bool,
    /// True when the failing compaction content came from a session_before_compact handler.
    pub from_extension: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionShutdownEventReason {
    #[serde(rename = "quit")]
    Quit,
    #[serde(rename = "reload")]
    Reload,
    #[serde(rename = "new")]
    New,
    #[serde(rename = "resume")]
    Resume,
    #[serde(rename = "fork")]
    Fork,
}

impl SessionShutdownEventReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Reload => "reload",
            Self::New => "new",
            Self::Resume => "resume",
            Self::Fork => "fork",
        }
    }
}

/// Fired before an extension runtime is torn down due to quit, reload, or session replacement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionShutdownEvent {
    pub reason: SessionShutdownEventReason,
    /// Destination session file when shutting down due to session replacement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_session_file: Option<String>,
}

/// Preparation data for tree navigation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreePreparation {
    pub target_id: String,
    #[serde(default)]
    pub old_leaf_id: Option<String>,
    #[serde(default)]
    pub common_ancestor_id: Option<String>,
    pub entries_to_summarize: Vec<SessionEntry>,
    pub user_wants_summary: bool,
    /// Custom instructions for summarization
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
    /// If true, customInstructions replaces the default prompt instead of being appended
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace_instructions: Option<bool>,
    /// Label to attach to the branch summary entry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Fired before navigating in the session tree (can be cancelled)
#[derive(Debug, Clone)]
pub struct SessionBeforeTreeEvent {
    pub preparation: TreePreparation,
    pub signal: CancellationToken,
}

/// Fired after navigating in the session tree
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTreeEvent {
    #[serde(default)]
    pub new_leaf_id: Option<String>,
    #[serde(default)]
    pub old_leaf_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_entry: Option<BranchSummaryEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_extension: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    SessionStart(SessionStartEvent),
    SessionInfoChanged(SessionInfoChangedEvent),
    SessionBeforeSwitch(SessionBeforeSwitchEvent),
    SessionBeforeFork(SessionBeforeForkEvent),
    SessionBeforeCompact(SessionBeforeCompactEvent),
    SessionCompact(SessionCompactEvent),
    SessionCompactFailed(SessionCompactFailedEvent),
    SessionShutdown(SessionShutdownEvent),
    SessionBeforeTree(SessionBeforeTreeEvent),
    SessionTree(SessionTreeEvent),
}

/// Fired before each LLM call. Can modify messages.
///
/// `messages` holds the conversation without system messages. The prompt and tool state
/// belong to Pi: it restores them after the handler returns, so a handler cannot drop
/// them and does not need to preserve them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEvent {
    /// Always "context".
    pub r#type: String,
    pub messages: Vec<AgentMessage>,
}

/// Fired before each LLM call, after every `context` handler has run and Pi has restored
/// the prompt and tool state. `messages` is the full transcript including system messages,
/// and the result is sent as returned: the handler owns the prompt and tool declarations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextWithSystemEvent {
    /// Always "context_with_system".
    pub r#type: String,
    pub messages: Vec<AgentMessage>,
}

/// Fired before a provider request is sent. Can replace the payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeProviderRequestEvent {
    /// Always "before_provider_request".
    pub r#type: String,
    pub payload: Value,
}

/// Fired after request headers are assembled, before the provider HTTP call.
/// Handlers mutate `headers` in place (e.g. to inject tracing/session headers);
/// the return value is ignored. A `null` value deletes that header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeProviderHeadersEvent {
    /// Always "before_provider_headers".
    pub r#type: String,
    pub headers: ProviderHeaders,
}

/// Fired after a provider response is received and before the response stream is consumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AfterProviderResponseEvent {
    /// Always "after_provider_response".
    pub r#type: String,
    pub status: i64,
    pub headers: IndexMap<String, String>,
}

/// Fired for a parsed provider stream event before Pi normalizes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStreamEvent {
    /// Always "provider_stream_event".
    pub r#type: String,
    pub provider: ProviderId,
    pub api: Api,
    pub model: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartEventBase {
    /// Always "before_agent_start".
    pub r#type: String,
    /// The current system prompt, rendered from systemPromptOptions and earlier handler changes.
    pub system_prompt: String,
    /// Mutable prompt sections. Later handlers observe mutations made by earlier handlers.
    pub system_prompt_options: NormalizedBuildSystemPromptOptions,
}

/// A run started by a submitted prompt, RPC prompt, or `pi.sendUserMessage()`.
// TS: extends BeforeAgentStartEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartUserEvent {
    /// Always "before_agent_start".
    pub r#type: String,
    /// The current system prompt, rendered from systemPromptOptions and earlier handler changes.
    pub system_prompt: String,
    /// Mutable prompt sections. Later handlers observe mutations made by earlier handlers.
    pub system_prompt_options: NormalizedBuildSystemPromptOptions,
    /// The raw user prompt text (after expansion).
    pub prompt: String,
    /// Images attached to the user prompt, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ImageContent>>,
}

/// A run started by `pi.sendMessage(..., { triggerTurn: true })` while idle.
// TS: extends BeforeAgentStartEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartCustomEvent {
    /// Always "before_agent_start".
    pub r#type: String,
    /// The current system prompt, rendered from systemPromptOptions and earlier handler changes.
    pub system_prompt: String,
    /// Mutable prompt sections. Later handlers observe mutations made by earlier handlers.
    pub system_prompt_options: NormalizedBuildSystemPromptOptions,
    /// The custom message that starts the run.
    pub message: CustomMessage,
}

/// Fired before a run starts from a user message or an idle custom message with `triggerTurn`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "messageType", rename_all_fields = "camelCase")]
pub enum BeforeAgentStartEvent {
    #[serde(rename = "user")]
    User(BeforeAgentStartUserEvent),
    #[serde(rename = "custom")]
    Custom(BeforeAgentStartCustomEvent),
}

/// Fired when an agent loop starts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStartEvent {
    /// Always "agent_start".
    pub r#type: String,
}

/// Fired when an agent loop ends
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEndEvent {
    /// Always "agent_end".
    pub r#type: String,
    pub messages: Vec<AgentMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentActivityOutcome {
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "aborted")]
    Aborted,
    #[serde(rename = "error")]
    Error,
}

impl AgentActivityOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Aborted => "aborted",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEntryDraft {
    pub custom_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomMessageEntryDraft {
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEditEntryDraftReplacement {
    pub content: ContextEditableContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEditEntryDraft {
    pub target_id: String,
    #[serde(default)]
    pub replacement: Option<ContextEditEntryDraftReplacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionEntryDraft {
    pub summary: String,
    /// Null creates a self-retaining compaction that keeps no preceding entries.
    #[serde(default)]
    pub first_kept_entry_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum SessionBoundaryDraft {
    #[serde(rename = "custom")]
    Custom(CustomEntryDraft),
    #[serde(rename = "custom_message")]
    CustomMessage(CustomMessageEntryDraft),
    #[serde(rename = "context_edit")]
    ContextEdit(ContextEditEntryDraft),
    #[serde(rename = "compaction")]
    Compaction(CompactionEntryDraft),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryContextPreview {
    pub context_entries: Vec<ProjectedSessionEntry>,
    pub context_messages: Vec<AgentMessage>,
    pub llm_messages: Vec<Message>,
    pub pending_messages: Vec<AgentMessage>,
    pub can_continue: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryState {
    pub entries: Vec<SessionBoundaryDraft>,
    pub r#continue: bool,
    pub context: BoundaryContextPreview,
    pub outcome: AgentActivityOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<SessionBoundaryDraft>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#continue: Option<bool>,
}

/// Fired before final settlement. May append entries and ensure one next provider request.
// TS: extends BoundaryState (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBeforeSettleEvent {
    pub entries: Vec<SessionBoundaryDraft>,
    pub r#continue: bool,
    pub context: BoundaryContextPreview,
    pub outcome: AgentActivityOutcome,
    /// Always "agent_before_settle".
    pub r#type: String,
}

/// Fired after an agent run has fully settled and no automatic retry, compaction, or queued continuation will run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettledEvent {
    /// Always "agent_settled".
    pub r#type: String,
    /// Whether the run ended because it was aborted, for example with Escape.
    pub aborted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UIPromptKind {
    #[serde(rename = "select")]
    Select,
    #[serde(rename = "confirm")]
    Confirm,
    #[serde(rename = "input")]
    Input,
    #[serde(rename = "editor")]
    Editor,
    #[serde(rename = "custom")]
    Custom,
}

impl UIPromptKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Confirm => "confirm",
            Self::Input => "input",
            Self::Editor => "editor",
            Self::Custom => "custom",
        }
    }
}

/// Fired when Pi starts waiting on a blocking user-facing extension UI prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UIPromptStartEvent {
    /// Always "ui_prompt_start".
    pub r#type: String,
    /// Always "ui_prompt".
    pub reason: String,
    pub kind: UIPromptKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Fired when Pi is no longer waiting on a blocking user-facing extension UI prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UIPromptEndEvent {
    /// Always "ui_prompt_end".
    pub r#type: String,
    /// Always "ui_prompt".
    pub reason: String,
    pub kind: UIPromptKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Fired at the start of each turn
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartEvent {
    /// Always "turn_start".
    pub r#type: String,
    pub turn_index: i64,
    pub timestamp: i64,
}

/// Fired at the end of each turn
// TS: extends BoundaryState (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnEndEvent {
    pub entries: Vec<SessionBoundaryDraft>,
    pub r#continue: bool,
    pub context: BoundaryContextPreview,
    pub outcome: AgentActivityOutcome,
    /// Always "turn_end".
    pub r#type: String,
    pub turn_index: i64,
    pub message: AgentMessage,
    pub tool_results: Vec<ToolResultMessage>,
    pub message_entry_id: String,
    pub tool_result_entry_ids: Vec<String>,
}

/// Fired when a message starts (user, assistant, or toolResult)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageStartEvent {
    /// Always "message_start".
    pub r#type: String,
    pub message: AgentMessage,
}

/// Fired during assistant message streaming with token-by-token updates
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageUpdateEvent {
    /// Always "message_update".
    pub r#type: String,
    pub message: AgentMessage,
    pub assistant_message_event: AssistantMessageEvent,
}

/// Fired when a message ends
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageEndEvent {
    /// Always "message_end".
    pub r#type: String,
    pub message: AgentMessage,
}

/// Fired when a tool starts executing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionStartEvent {
    /// Always "tool_execution_start".
    pub r#type: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: Value,
    /// Set when another tool (for example a codemode script) made this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
}

/// Fired during tool execution with partial/streaming output
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionUpdateEvent {
    /// Always "tool_execution_update".
    pub r#type: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: Value,
    pub partial_result: Value,
    /// Set when another tool (for example a codemode script) made this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
}

/// Fired when a tool finishes executing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionEndEvent {
    /// Always "tool_execution_end".
    pub r#type: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub result: Value,
    pub is_error: bool,
    /// Milliseconds `execute()` took, measured with a monotonic clock; absent when the tool did not run.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub duration_ms: Option<f64>,
    /// Set when another tool (for example a codemode script) made this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelSelectSource {
    #[serde(rename = "set")]
    Set,
    #[serde(rename = "cycle")]
    Cycle,
    #[serde(rename = "restore")]
    Restore,
}

impl ModelSelectSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Set => "set",
            Self::Cycle => "cycle",
            Self::Restore => "restore",
        }
    }
}

/// Fired when a new model is selected
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelectEvent {
    /// Always "model_select".
    pub r#type: String,
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_model: Option<Model>,
    pub source: ModelSelectSource,
}

/// Fired when a new thinking level is selected
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingLevelSelectEvent {
    /// Always "thinking_level_select".
    pub r#type: String,
    pub level: ThinkingLevel,
    pub previous_level: ThinkingLevel,
}

/// Fired when user executes a bash command via ! or !! prefix
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserBashEvent {
    /// Always "user_bash".
    pub r#type: String,
    /// The command to execute
    pub command: String,
    /// True if !! prefix was used (excluded from LLM context)
    pub exclude_from_context: bool,
    /// Current working directory
    pub cwd: String,
}

/// Source of user input
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputSource {
    #[serde(rename = "interactive")]
    Interactive,
    #[serde(rename = "rpc")]
    Rpc,
    #[serde(rename = "extension")]
    Extension,
}

impl InputSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Rpc => "rpc",
            Self::Extension => "extension",
        }
    }
}

/// Fired when user input is received, before agent processing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputEvent {
    /// Always "input".
    pub r#type: String,
    /// The input text
    pub text: String,
    /// Attached images, if any
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ImageContent>>,
    /// Where the input came from
    pub source: InputSource,
    /// How the input will be delivered during streaming, or undefined when idle
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming_behavior: Option<ReplacedSessionContextOptions2DeliverAs>,
}

/// Result from input event handler
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all_fields = "camelCase")]
pub enum InputEventResult {
    #[serde(rename = "continue")]
    Continue {},
    #[serde(rename = "transform")]
    Transform {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageContent>>,
    },
    #[serde(rename = "handled")]
    Handled {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallEventBase {
    /// Always "tool_call".
    pub r#type: String,
    /// The call's id. For calls another tool made (with `parentToolCallId` set), pi assigns
    /// `<parent id>/<n>`; such ids never appear as tool calls or tool results in the transcript, only
    /// in the parent result's `nestedCalls` record.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
}

// TS: extends ToolCallEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashToolCallEvent {
    /// Always "tool_call".
    pub r#type: String,
    /// The call's id. For calls another tool made (with `parentToolCallId` set), pi assigns
    /// `<parent id>/<n>`; such ids never appear as tool calls or tool results in the transcript, only
    /// in the parent result's `nestedCalls` record.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    /// Always "bash".
    pub tool_name: String,
    pub input: BashToolInput,
}

// TS: extends ToolCallEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerShellToolCallEvent {
    /// Always "tool_call".
    pub r#type: String,
    /// The call's id. For calls another tool made (with `parentToolCallId` set), pi assigns
    /// `<parent id>/<n>`; such ids never appear as tool calls or tool results in the transcript, only
    /// in the parent result's `nestedCalls` record.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    /// Always "powershell".
    pub tool_name: String,
    pub input: PowerShellToolInput,
}

// TS: extends ToolCallEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadToolCallEvent {
    /// Always "tool_call".
    pub r#type: String,
    /// The call's id. For calls another tool made (with `parentToolCallId` set), pi assigns
    /// `<parent id>/<n>`; such ids never appear as tool calls or tool results in the transcript, only
    /// in the parent result's `nestedCalls` record.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    /// Always "read".
    pub tool_name: String,
    pub input: ReadToolInput,
}

// TS: extends ToolCallEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditToolCallEvent {
    /// Always "tool_call".
    pub r#type: String,
    /// The call's id. For calls another tool made (with `parentToolCallId` set), pi assigns
    /// `<parent id>/<n>`; such ids never appear as tool calls or tool results in the transcript, only
    /// in the parent result's `nestedCalls` record.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    /// Always "edit".
    pub tool_name: String,
    pub input: EditToolInput,
}

// TS: extends ToolCallEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteToolCallEvent {
    /// Always "tool_call".
    pub r#type: String,
    /// The call's id. For calls another tool made (with `parentToolCallId` set), pi assigns
    /// `<parent id>/<n>`; such ids never appear as tool calls or tool results in the transcript, only
    /// in the parent result's `nestedCalls` record.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    /// Always "write".
    pub tool_name: String,
    pub input: WriteToolInput,
}

// TS: extends ToolCallEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepToolCallEvent {
    /// Always "tool_call".
    pub r#type: String,
    /// The call's id. For calls another tool made (with `parentToolCallId` set), pi assigns
    /// `<parent id>/<n>`; such ids never appear as tool calls or tool results in the transcript, only
    /// in the parent result's `nestedCalls` record.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    /// Always "grep".
    pub tool_name: String,
    pub input: GrepToolInput,
}

// TS: extends ToolCallEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindToolCallEvent {
    /// Always "tool_call".
    pub r#type: String,
    /// The call's id. For calls another tool made (with `parentToolCallId` set), pi assigns
    /// `<parent id>/<n>`; such ids never appear as tool calls or tool results in the transcript, only
    /// in the parent result's `nestedCalls` record.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    /// Always "find".
    pub tool_name: String,
    pub input: FindToolInput,
}

// TS: extends ToolCallEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LsToolCallEvent {
    /// Always "tool_call".
    pub r#type: String,
    /// The call's id. For calls another tool made (with `parentToolCallId` set), pi assigns
    /// `<parent id>/<n>`; such ids never appear as tool calls or tool results in the transcript, only
    /// in the parent result's `nestedCalls` record.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    /// Always "ls".
    pub tool_name: String,
    pub input: LsToolInput,
}

// TS: extends ToolCallEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomToolCallEvent {
    /// Always "tool_call".
    pub r#type: String,
    /// The call's id. For calls another tool made (with `parentToolCallId` set), pi assigns
    /// `<parent id>/<n>`; such ids never appear as tool calls or tool results in the transcript, only
    /// in the parent result's `nestedCalls` record.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub tool_name: String,
    pub input: IndexMap<String, Value>,
}

/// Fired before a tool executes. Can block.
///
/// `event.input` is mutable. Mutate it in place to patch tool arguments before execution.
/// Later `tool_call` handlers see earlier mutations. No re-validation is performed after mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolCallEvent {
    BashToolCallEvent(BashToolCallEvent),
    PowerShellToolCallEvent(PowerShellToolCallEvent),
    ReadToolCallEvent(ReadToolCallEvent),
    EditToolCallEvent(EditToolCallEvent),
    WriteToolCallEvent(WriteToolCallEvent),
    GrepToolCallEvent(GrepToolCallEvent),
    FindToolCallEvent(FindToolCallEvent),
    LsToolCallEvent(LsToolCallEvent),
    CustomToolCallEvent(CustomToolCallEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultEventBase {
    /// Always "tool_result".
    pub r#type: String,
    /// The call's id; `<parent id>/<n>` for nested calls, see `ToolCallEvent`.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub input: IndexMap<String, Value>,
    pub content: Vec<UserContentBlock>,
    /// Machine-readable result for tools that declare an `outputSchema`. Handlers that redact
    /// `content` should also replace this; replacing `content` alone drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    pub is_error: bool,
    /// Usage from the tool execution itself, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

// TS: extends ToolResultEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashToolResultEvent {
    /// Always "tool_result".
    pub r#type: String,
    /// The call's id; `<parent id>/<n>` for nested calls, see `ToolCallEvent`.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub input: IndexMap<String, Value>,
    pub content: Vec<UserContentBlock>,
    /// Machine-readable result for tools that declare an `outputSchema`. Handlers that redact
    /// `content` should also replace this; replacing `content` alone drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    pub is_error: bool,
    /// Usage from the tool execution itself, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Always "bash".
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<BashToolDetails>,
}

// TS: extends ToolResultEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerShellToolResultEvent {
    /// Always "tool_result".
    pub r#type: String,
    /// The call's id; `<parent id>/<n>` for nested calls, see `ToolCallEvent`.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub input: IndexMap<String, Value>,
    pub content: Vec<UserContentBlock>,
    /// Machine-readable result for tools that declare an `outputSchema`. Handlers that redact
    /// `content` should also replace this; replacing `content` alone drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    pub is_error: bool,
    /// Usage from the tool execution itself, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Always "powershell".
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<PowerShellToolDetails>,
}

// TS: extends ToolResultEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadToolResultEvent {
    /// Always "tool_result".
    pub r#type: String,
    /// The call's id; `<parent id>/<n>` for nested calls, see `ToolCallEvent`.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub input: IndexMap<String, Value>,
    pub content: Vec<UserContentBlock>,
    /// Machine-readable result for tools that declare an `outputSchema`. Handlers that redact
    /// `content` should also replace this; replacing `content` alone drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    pub is_error: bool,
    /// Usage from the tool execution itself, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Always "read".
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ReadToolDetails>,
}

// TS: extends ToolResultEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditToolResultEvent {
    /// Always "tool_result".
    pub r#type: String,
    /// The call's id; `<parent id>/<n>` for nested calls, see `ToolCallEvent`.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub input: IndexMap<String, Value>,
    pub content: Vec<UserContentBlock>,
    /// Machine-readable result for tools that declare an `outputSchema`. Handlers that redact
    /// `content` should also replace this; replacing `content` alone drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    pub is_error: bool,
    /// Usage from the tool execution itself, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Always "edit".
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<EditToolDetails>,
}

// TS: extends ToolResultEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteToolResultEvent {
    /// Always "tool_result".
    pub r#type: String,
    /// The call's id; `<parent id>/<n>` for nested calls, see `ToolCallEvent`.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub input: IndexMap<String, Value>,
    pub content: Vec<UserContentBlock>,
    /// Machine-readable result for tools that declare an `outputSchema`. Handlers that redact
    /// `content` should also replace this; replacing `content` alone drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    pub is_error: bool,
    /// Usage from the tool execution itself, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Always "write".
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

// TS: extends ToolResultEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepToolResultEvent {
    /// Always "tool_result".
    pub r#type: String,
    /// The call's id; `<parent id>/<n>` for nested calls, see `ToolCallEvent`.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub input: IndexMap<String, Value>,
    pub content: Vec<UserContentBlock>,
    /// Machine-readable result for tools that declare an `outputSchema`. Handlers that redact
    /// `content` should also replace this; replacing `content` alone drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    pub is_error: bool,
    /// Usage from the tool execution itself, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Always "grep".
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<GrepToolDetails>,
}

// TS: extends ToolResultEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindToolResultEvent {
    /// Always "tool_result".
    pub r#type: String,
    /// The call's id; `<parent id>/<n>` for nested calls, see `ToolCallEvent`.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub input: IndexMap<String, Value>,
    pub content: Vec<UserContentBlock>,
    /// Machine-readable result for tools that declare an `outputSchema`. Handlers that redact
    /// `content` should also replace this; replacing `content` alone drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    pub is_error: bool,
    /// Usage from the tool execution itself, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Always "find".
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<FindToolDetails>,
}

// TS: extends ToolResultEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LsToolResultEvent {
    /// Always "tool_result".
    pub r#type: String,
    /// The call's id; `<parent id>/<n>` for nested calls, see `ToolCallEvent`.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub input: IndexMap<String, Value>,
    pub content: Vec<UserContentBlock>,
    /// Machine-readable result for tools that declare an `outputSchema`. Handlers that redact
    /// `content` should also replace this; replacing `content` alone drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    pub is_error: bool,
    /// Usage from the tool execution itself, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Always "ls".
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<LsToolDetails>,
}

// TS: extends ToolResultEventBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomToolResultEvent {
    /// Always "tool_result".
    pub r#type: String,
    /// The call's id; `<parent id>/<n>` for nested calls, see `ToolCallEvent`.
    pub tool_call_id: String,
    /// Set when another tool (for example a codemode script) issued this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    pub input: IndexMap<String, Value>,
    pub content: Vec<UserContentBlock>,
    /// Machine-readable result for tools that declare an `outputSchema`. Handlers that redact
    /// `content` should also replace this; replacing `content` alone drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    pub is_error: bool,
    /// Usage from the tool execution itself, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    pub tool_name: String,
    pub details: Value,
}

/// Fired after a tool executes. Can modify result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultEvent {
    BashToolResultEvent(BashToolResultEvent),
    PowerShellToolResultEvent(PowerShellToolResultEvent),
    ReadToolResultEvent(ReadToolResultEvent),
    EditToolResultEvent(EditToolResultEvent),
    WriteToolResultEvent(WriteToolResultEvent),
    GrepToolResultEvent(GrepToolResultEvent),
    FindToolResultEvent(FindToolResultEvent),
    LsToolResultEvent(LsToolResultEvent),
    CustomToolResultEvent(CustomToolResultEvent),
}

pub fn is_bash_tool_result(e: &ToolResultEvent) -> bool {
    todo!("port core/extensions/types.ts:isBashToolResult")
}

pub fn is_power_shell_tool_result(e: &ToolResultEvent) -> bool {
    todo!("port core/extensions/types.ts:isPowerShellToolResult")
}

pub fn is_read_tool_result(e: &ToolResultEvent) -> bool {
    todo!("port core/extensions/types.ts:isReadToolResult")
}

pub fn is_edit_tool_result(e: &ToolResultEvent) -> bool {
    todo!("port core/extensions/types.ts:isEditToolResult")
}

pub fn is_write_tool_result(e: &ToolResultEvent) -> bool {
    todo!("port core/extensions/types.ts:isWriteToolResult")
}

pub fn is_grep_tool_result(e: &ToolResultEvent) -> bool {
    todo!("port core/extensions/types.ts:isGrepToolResult")
}

pub fn is_find_tool_result(e: &ToolResultEvent) -> bool {
    todo!("port core/extensions/types.ts:isFindToolResult")
}

pub fn is_ls_tool_result(e: &ToolResultEvent) -> bool {
    todo!("port core/extensions/types.ts:isLsToolResult")
}

pub fn is_tool_call_event_type(tool_name: &str, event: &ToolCallEvent) -> bool {
    todo!("port core/extensions/types.ts:isToolCallEventType")
}

/// Union of all event types
#[derive(Debug, Clone)]
pub enum ExtensionEvent {
    ProjectTrustEvent(ProjectTrustEvent),
    ResourcesDiscoverEvent(ResourcesDiscoverEvent),
    McpServersChangeEvent(McpServersChangeEvent),
    SessionStartEvent(SessionStartEvent),
    SessionInfoChangedEvent(SessionInfoChangedEvent),
    SessionBeforeSwitchEvent(SessionBeforeSwitchEvent),
    SessionBeforeForkEvent(SessionBeforeForkEvent),
    SessionBeforeCompactEvent(SessionBeforeCompactEvent),
    SessionCompactEvent(SessionCompactEvent),
    SessionCompactFailedEvent(SessionCompactFailedEvent),
    SessionShutdownEvent(SessionShutdownEvent),
    SessionBeforeTreeEvent(SessionBeforeTreeEvent),
    SessionTreeEvent(SessionTreeEvent),
    ContextEvent(ContextEvent),
    ContextWithSystemEvent(ContextWithSystemEvent),
    CacheWarmingDecisionEvent(CacheWarmingDecisionEvent),
    BeforeProviderRequestEvent(BeforeProviderRequestEvent),
    BeforeProviderHeadersEvent(BeforeProviderHeadersEvent),
    AfterProviderResponseEvent(AfterProviderResponseEvent),
    ProviderStreamEvent(ProviderStreamEvent),
    BeforeAgentStartUserEvent(BeforeAgentStartUserEvent),
    BeforeAgentStartCustomEvent(BeforeAgentStartCustomEvent),
    AgentStartEvent(AgentStartEvent),
    AgentEndEvent(AgentEndEvent),
    AgentBeforeSettleEvent(AgentBeforeSettleEvent),
    AgentSettledEvent(AgentSettledEvent),
    UIPromptStartEvent(UIPromptStartEvent),
    UIPromptEndEvent(UIPromptEndEvent),
    TurnStartEvent(TurnStartEvent),
    TurnEndEvent(TurnEndEvent),
    MessageStartEvent(MessageStartEvent),
    MessageUpdateEvent(MessageUpdateEvent),
    MessageEndEvent(MessageEndEvent),
    ToolExecutionStartEvent(ToolExecutionStartEvent),
    ToolExecutionUpdateEvent(ToolExecutionUpdateEvent),
    ToolExecutionEndEvent(ToolExecutionEndEvent),
    ModelSelectEvent(ModelSelectEvent),
    ThinkingLevelSelectEvent(ThinkingLevelSelectEvent),
    UserBashEvent(UserBashEvent),
    InputEvent(InputEvent),
    BashToolCallEvent(BashToolCallEvent),
    PowerShellToolCallEvent(PowerShellToolCallEvent),
    ReadToolCallEvent(ReadToolCallEvent),
    EditToolCallEvent(EditToolCallEvent),
    WriteToolCallEvent(WriteToolCallEvent),
    GrepToolCallEvent(GrepToolCallEvent),
    FindToolCallEvent(FindToolCallEvent),
    LsToolCallEvent(LsToolCallEvent),
    CustomToolCallEvent(CustomToolCallEvent),
    BashToolResultEvent(BashToolResultEvent),
    PowerShellToolResultEvent(PowerShellToolResultEvent),
    ReadToolResultEvent(ReadToolResultEvent),
    EditToolResultEvent(EditToolResultEvent),
    WriteToolResultEvent(WriteToolResultEvent),
    GrepToolResultEvent(GrepToolResultEvent),
    FindToolResultEvent(FindToolResultEvent),
    LsToolResultEvent(LsToolResultEvent),
    CustomToolResultEvent(CustomToolResultEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEventResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<AgentMessage>>,
}

pub type TurnEndEventResult = BoundaryResult;

pub type AgentBeforeSettleEventResult = BoundaryResult;

pub type BeforeProviderRequestEventResult = Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallEventResult {
    /// Block tool execution. To modify arguments, mutate `event.input` in place instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Hint that the agent should stop after the current tool batch when this call is blocked.
    /// Early termination only happens when every finalized tool result in the batch sets this to true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminate: Option<bool>,
}

#[derive(Clone)]
pub struct UserBashEventResultVariant1 {
    /// Custom operations to use for execution
    pub operations: BashOperations,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserBashEventResultVariant2 {
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub operations: Value,
    /// Full replacement: extension handled execution, use this result
    pub result: BashResult,
}

/// Result from user_bash event handler
#[derive(Clone)]
pub enum UserBashEventResult {
    UserBashEventResultVariant1(UserBashEventResultVariant1),
    UserBashEventResultVariant2(UserBashEventResultVariant2),
}

/// Changes a `tool_result` handler makes. Omitted fields stay as they are, except that replacing
/// `content` without returning `structuredContent` drops the structured content, because it may no
/// longer match. Return it along with `content` to keep it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultEventResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<UserContentBlock>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageEndEventResult {
    /// Replace the finalized message. The replacement must keep the original message role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<AgentMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartEventResultMessage {
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartEventResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<BeforeAgentStartEventResultMessage>,
    /// Replace the complete system prompt for this turn. Later handlers observe this exact override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBeforeSwitchResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBeforeForkResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_conversation_restore: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBeforeCompactResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<CompactionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBeforeTreeResultSummary {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBeforeTreeResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<SessionBeforeTreeResultSummary>,
    /// Override custom instructions for summarization
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
    /// Override whether customInstructions replaces the default prompt
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace_instructions: Option<bool>,
    /// Override label to attach to the branch summary entry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRenderOptions {
    pub expanded: bool,
    /// Horizontal padding configured by the outputPad setting.
    pub output_pad: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarkdownTransformContextMessageType {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "assistant-thinking")]
    AssistantThinking,
}

impl MarkdownTransformContextMessageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::AssistantThinking => "assistant-thinking",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkdownTransformContext {
    pub message_type: MarkdownTransformContextMessageType,
    pub is_streaming: bool,
    pub available_width: i64,
}

pub type MarkdownTransformer =
    Arc<dyn Fn(String, MarkdownTransformContext) -> String + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryRenderOptions {
    pub expanded: bool,
}

pub type MessageRenderer = Arc<
    dyn Fn(CustomMessage, MessageRenderOptions, Arc<Theme>) -> Option<ComponentRef> + Send + Sync,
>;

pub type EntryRenderer =
    Arc<dyn Fn(CustomEntry, EntryRenderOptions, Arc<Theme>) -> Option<ComponentRef> + Send + Sync>;

#[derive(Clone)]
pub struct RegisteredCommand {
    pub name: String,
    pub source_info: SourceInfo,
    pub description: Option<String>,
    pub get_argument_completions: Option<
        Arc<
            dyn Fn(
                    String,
                ) -> BoxFuture<
                    'static,
                    anyhow::Result<Option<Vec<Value /* TS: pi-tui::AutocompleteItem */>>>,
                > + Send
                + Sync,
        >,
    >,
    pub handler: Arc<
        dyn Fn(String, Arc<dyn ExtensionCommandContext>) -> BoxFuture<'static, anyhow::Result<()>>
            + Send
            + Sync,
    >,
}

// TS: extends RegisteredCommand (inherited fields inlined)
#[derive(Clone)]
pub struct ResolvedCommand {
    pub name: String,
    pub source_info: SourceInfo,
    pub description: Option<String>,
    pub get_argument_completions: Option<
        Arc<
            dyn Fn(
                    String,
                ) -> BoxFuture<
                    'static,
                    anyhow::Result<Option<Vec<Value /* TS: pi-tui::AutocompleteItem */>>>,
                > + Send
                + Sync,
        >,
    >,
    pub handler: Arc<
        dyn Fn(String, Arc<dyn ExtensionCommandContext>) -> BoxFuture<'static, anyhow::Result<()>>
            + Send
            + Sync,
    >,
    pub invocation_name: String,
}

/// Handler function type for events
pub type ExtensionHandler = Arc<
    dyn Fn(Value, Arc<dyn ExtensionContext>) -> BoxFuture<'static, anyhow::Result<Option<Value>>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct ExtensionAPIOptions {
    pub description: Option<String>,
    pub get_argument_completions: Option<
        Arc<
            dyn Fn(
                    String,
                ) -> BoxFuture<
                    'static,
                    anyhow::Result<Option<Vec<Value /* TS: pi-tui::AutocompleteItem */>>>,
                > + Send
                + Sync,
        >,
    >,
    pub handler: Arc<
        dyn Fn(String, Arc<dyn ExtensionCommandContext>) -> BoxFuture<'static, anyhow::Result<()>>
            + Send
            + Sync,
    >,
}

#[derive(Clone)]
pub struct ExtensionAPIOptions2 {
    pub description: Option<String>,
    pub handler: Arc<
        dyn Fn(Arc<dyn ExtensionContext>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum ExtensionAPIOptions3 {
    #[serde(rename = "boolean")]
    Boolean {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<bool>,
    },
    #[serde(rename = "string")]
    String {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtensionAPIGetFlagResult {
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionAPIMessage {
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionAPIOptions4 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_turn: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliver_as: Option<ReplacedSessionContextOptionsDeliverAs>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionAPIOptions5 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliver_as: Option<ReplacedSessionContextOptions2DeliverAs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand_prompt_templates: Option<bool>,
}

/// ExtensionAPI passed to extension factory functions.
#[async_trait::async_trait]
pub trait ExtensionAPI: Send + Sync {
    fn on(&self, event: &str, handler: &ProjectTrustHandler) -> Arc<dyn Fn() + Send + Sync>;
    /// Register a tool that the LLM can call.
    fn register_tool(&self, tool: Arc<dyn ToolDefinition>);
    /// Register a custom command.
    fn register_command(&self, name: &str, options: &ExtensionAPIOptions);
    /// Register a keyboard shortcut.
    fn register_shortcut(&self, shortcut: &str, options: &ExtensionAPIOptions2);
    /// Register a CLI flag.
    fn register_flag(&self, name: &str, options: &ExtensionAPIOptions3);
    /// Get the value of a registered CLI flag.
    fn get_flag(&self, name: &str) -> Option<ExtensionAPIGetFlagResult>;
    /// Register a custom renderer for CustomMessageEntry.
    fn register_message_renderer(&self, custom_type: &str, renderer: &MessageRenderer);
    /// Register a transformer for user and assistant Markdown before Pi renders it in the interactive transcript.
    fn register_markdown_transformer(&self, transformer: &MarkdownTransformer);
    /// Register a custom renderer for CustomEntry. Custom entries do not participate in LLM context.
    fn register_entry_renderer(&self, custom_type: &str, renderer: &EntryRenderer);
    /// Choose how tool calls are drawn. Resolvers run in extension load order.
    fn register_tool_renderer(&self, resolver: &ToolRendererResolver);
    /// Send a custom message to the session.
    fn send_message(&self, message: &ExtensionAPIMessage, options: Option<&ExtensionAPIOptions4>);
    /// Send a user message to the agent. Always triggers a turn.
    /// When the agent is streaming, use deliverAs to specify how to queue the message.
    /// Set expandPromptTemplates to dispatch extension commands and expand skill commands and prompt templates.
    fn send_user_message(&self, content: UserContent, options: Option<&ExtensionAPIOptions5>);
    /// Append a custom entry to the session for state persistence (not sent to LLM).
    fn append_entry(&self, custom_type: &str, data: Option<Value>);
    /// Set the session display name (shown in session selector).
    fn set_session_name(&self, name: &str);
    /// Get the current session name, if set.
    fn get_session_name(&self) -> Option<String>;
    /// Set or clear a label on an entry. Labels are user-defined markers for bookmarking/navigation.
    fn set_label(&self, entry_id: &str, label: Option<&str>);
    /// Execute a shell command.
    async fn exec(
        &self,
        command: &str,
        args: &[String],
        options: Option<&ExecOptions>,
    ) -> anyhow::Result<ExecResult>;
    /// Get the names of the active tools, which are the tools declared to the model.
    fn get_active_tools(&self) -> Vec<String>;
    /// Get all configured tools with parameter schema, prompt guidelines, exposure, and source metadata.
    fn get_all_tools(&self) -> Vec<ToolInfo>;
    /// Get a copy of the effective settings (global and project settings merged, with overrides).
    fn get_settings(&self) -> Settings;
    /// Set the active tools by name. Unknown and `hidden` tools are ignored. Tools with `codemode` or
    /// `deferred` exposure stay callable from codemode scripts whether active or not.
    fn set_active_tools(&self, tool_names: &[String]);
    /// Get available slash commands in the current session.
    fn get_commands(&self) -> Vec<SlashCommandInfo>;
    /// Set the model for the current session without changing the configured default for new sessions.
    /// Returns false if authentication is not configured for the model's provider.
    async fn set_model(&self, model: Model) -> anyhow::Result<bool>;
    /// Get current thinking level.
    fn get_thinking_level(&self) -> ThinkingLevel;
    /// Set the thinking level (clamped to model capabilities) for the current session without changing the configured default
    /// for new sessions.
    fn set_thinking_level(&self, level: ThinkingLevel);
    /// Register or override a model provider.
    ///
    /// If `models` is provided: replaces all existing models for this provider.
    /// If only `baseUrl` is provided: overrides the URL for existing models.
    /// If `oauth` is provided: registers OAuth provider for /login support.
    /// If `streamSimple` is provided: registers a custom API stream handler.
    ///
    /// During initial extension load this call is queued and applied once the
    /// runner has bound its context. After that it takes effect immediately, so
    /// it is safe to call from command handlers or event callbacks without
    /// requiring a `/reload`.
    ///
    /// @example
    /// // Register a new provider with custom models
    /// pi.registerProvider("my-proxy", {
    /// baseUrl: "https://proxy.example.com",
    /// apiKey: "$PROXY_API_KEY",
    /// api: "anthropic-messages",
    /// models: [
    /// {
    /// id: "claude-sonnet-4-20250514",
    /// name: "Claude 4 Sonnet (proxy)",
    /// reasoning: false,
    /// input: ["text", "image"],
    /// cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    /// contextWindow: 200000,
    /// maxTokens: 16384
    /// }
    /// ]
    /// });
    ///
    /// @example
    /// // Override baseUrl for an existing provider
    /// pi.registerProvider("anthropic", {
    /// baseUrl: "https://proxy.example.com"
    /// });
    ///
    /// @example
    /// // Register provider with OAuth support
    /// pi.registerProvider("corporate-ai", {
    /// baseUrl: "https://ai.corp.com",
    /// api: "openai-responses",
    /// models: [...],
    /// oauth: {
    /// name: "Corporate AI (SSO)",
    /// async login(callbacks) { ... },
    /// async refreshToken(credentials) { ... },
    /// getApiKey(credentials) { return credentials.access; }
    /// }
    /// });
    fn register_provider(&self, provider: Arc<dyn Provider>);
    /// Unregister a previously registered provider.
    ///
    /// Removes all models belonging to the named provider and restores any
    /// built-in models that were overridden by it. Has no effect if the provider
    /// is not currently registered.
    ///
    /// Like `registerProvider`, this takes effect immediately when called after
    /// the initial load phase.
    ///
    /// @example
    /// pi.unregisterProvider("my-proxy");
    fn unregister_provider(&self, name: &str);
    /// Register an MCP server for this session, with the same config as an `mcpServers` entry in
    /// `mcp.json`. The server connects next to the configured servers: on `session_start` when
    /// registered during extension load, right away when registered later. Registering a name again
    /// replaces the extension's earlier registration.
    ///
    /// The registration is not saved; register again on every load. A server of the same name in
    /// `mcp.json` takes precedence. Throws for invalid configs and for names another extension
    /// registered. When no loaded extension handles MCP servers (for example because another MCP
    /// extension replaced the built-in one), the registration is reported as an extension error.
    ///
    /// @example
    /// pi.registerMcpServer("jira", { url: "https://mcp.example.com/jira" });
    fn register_mcp_server(&self, name: &str, config: &McpServerConfig);
    /// Remove an MCP server this extension registered and close its connection.
    fn unregister_mcp_server(&self, name: &str);
    /// Every MCP server registered by extensions. For extensions that connect MCP servers.
    fn get_mcp_servers(&self) -> Vec<RegisteredMcpServer>;
    /// Register a virtual model: a selectable catalog entry that routes each request to a physical
    /// model. The selection (`ctx.model`, `model_change` entries) names the virtual model; assistant
    /// messages record the physical model and thinking level the router picked.
    ///
    /// `provider` may be any provider id, including one with physical models, and may list several
    /// virtual models. Registering the same provider and id again replaces the virtual model. See
    /// docs/virtual-models.md.
    fn register_virtual_model(&self, model: Arc<dyn ExtensionVirtualModel>);
    /// Remove a virtual model registered with `registerVirtualModel()`.
    fn unregister_virtual_model(&self, provider: &str, id: &str);
    /// Shared event bus for extension communication.
    fn events(&self) -> Arc<dyn EventBus>;
    fn set_events(&self, value: Arc<dyn EventBus>);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExtensionVirtualModelBaseInput {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "image")]
    Image,
}

impl ExtensionVirtualModelBaseInput {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionVirtualModelBase {
    pub name: String,
    /// Input types accepted for selection. Defaults to text and images; routed models without image support get placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<ExtensionVirtualModelBaseInput>>,
    /// Provider the virtual model is listed under. May be a provider with physical models.
    pub provider: String,
    /// Model id. Must not be the id of a physical model of `provider`.
    pub id: String,
    /// Thinking levels offered for selection. Defaults to `["off"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_levels: Option<Vec<ModelThinkingLevel>>,
    /// Limits shown before the first response. Afterwards, Pi uses the limits of the physical model
    /// that answered. Unset limits are unknown (0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
}

/// Virtual model registered via pi.registerVirtualModel().
#[async_trait::async_trait]
pub trait ExtensionVirtualModel: Send + Sync {
    /// Like `VirtualModelDefinition.route`, with an extension context.
    async fn route(
        &self,
        request: &ModelRouteRequest,
        ctx: Arc<dyn ExtensionContext>,
    ) -> anyhow::Result<ModelRoute>;
}

#[derive(Clone)]
pub struct ProviderConfigOauth {
    /// Display name for the provider in login UI.
    pub name: String,
    /// Whether access through this auth method is backed by a provider subscription.
    pub is_subscription: Option<bool>,
    /// @deprecated Retained for source compatibility; canonical auth flows ignore it.
    pub uses_callback_server: Option<bool>,
    /// Run the login flow, return credentials to persist.
    pub login: Arc<
        dyn Fn(
                Value, /* TS: pi-ai::OAuthLoginCallbacks */
            ) -> BoxFuture<'static, anyhow::Result<OAuthCredentials>>
            + Send
            + Sync,
    >,
    /// Refresh expired credentials, return updated credentials to persist.
    pub refresh_token: Arc<
        dyn Fn(
                OAuthCredentials,
                CancellationToken,
            ) -> BoxFuture<'static, anyhow::Result<OAuthCredentials>>
            + Send
            + Sync,
    >,
    /// Convert credentials to API key string for the provider.
    pub get_api_key: Arc<dyn Fn(OAuthCredentials) -> String + Send + Sync>,
    /// Legacy synchronous credential-dependent model projection.
    pub modify_models:
        Option<Arc<dyn Fn(Vec<Model>, OAuthCredentials) -> Vec<Model> + Send + Sync>>,
}

/// Configuration for registering a provider via pi.registerProvider().
#[async_trait::async_trait]
pub trait ProviderConfig: Send + Sync {
    /// Display name for the provider in UI.
    /// Optional in TS: the default implementation means "not provided".
    fn name(&self) -> Option<String> {
        None
    }
    fn set_name(&self, value: Option<String>);
    /// Base URL for the API endpoint. Required when defining models.
    /// Optional in TS: the default implementation means "not provided".
    fn base_url(&self) -> Option<String> {
        None
    }
    fn set_base_url(&self, value: Option<String>);
    /// API key literal, env interpolation ($ENV_VAR or ${ENV_VAR}), or leading !command. Required when defining models (unless oauth provided).
    /// Optional in TS: the default implementation means "not provided".
    fn api_key(&self) -> Option<String> {
        None
    }
    fn set_api_key(&self, value: Option<String>);
    /// API type. Required at provider or model level when defining models.
    /// Optional in TS: the default implementation means "not provided".
    fn api(&self) -> Option<Api> {
        None
    }
    fn set_api(&self, value: Option<Api>);
    /// Optional streamSimple handler for custom APIs.
    /// The context is a normalized transcript: read the prompt and tools from its system messages
    /// (`getCurrentSystemPrompt(context.messages)`, `getCurrentTools(context.messages)`).
    /// Implementations must invoke `options.onPayload` before sending the provider request and use any
    /// returned replacement payload. They must invoke `options.onResponse` after receiving the response
    /// and before consuming its body, matching built-in providers. Implementations may invoke
    /// `options.onProviderStreamEvent(data, model)` with parsed stream events before normalization.
    /// Event data is adapter-owned and must be treated as read-only.
    /// Optional in TS: the default implementation means "not provided".
    fn stream_simple(
        &self,
    ) -> Option<
        Arc<
            dyn Fn(
                    Model,
                    TranscriptContext,
                    Option<SimpleStreamOptions>,
                ) -> AssistantMessageEventStream
                + Send
                + Sync,
        >,
    > {
        None
    }
    /// Image-generation implementations keyed by image API.
    /// Optional in TS: the default implementation means "not provided".
    fn images(&self) -> Option<IndexMap<ImageApi, Arc<dyn ProviderImages>>> {
        None
    }
    fn set_images(&self, value: Option<IndexMap<ImageApi, Arc<dyn ProviderImages>>>);
    /// Classifier implementations keyed by classifier API.
    /// Optional in TS: the default implementation means "not provided".
    fn classifiers(&self) -> Option<IndexMap<ClassifierApi, Arc<dyn ProviderClassifier>>> {
        None
    }
    fn set_classifiers(&self, value: Option<IndexMap<ClassifierApi, Arc<dyn ProviderClassifier>>>);
    /// Custom headers to include in requests.
    /// Optional in TS: the default implementation means "not provided".
    fn headers(&self) -> Option<IndexMap<String, String>> {
        None
    }
    fn set_headers(&self, value: Option<IndexMap<String, String>>);
    /// If true, adds Authorization: Bearer header with the resolved API key.
    /// Optional in TS: the default implementation means "not provided".
    fn auth_header(&self) -> Option<bool> {
        None
    }
    fn set_auth_header(&self, value: Option<bool>);
    /// Models to register. If provided, replaces all existing models for this provider.
    /// Optional in TS: the default implementation means "not provided".
    fn models(&self) -> Option<Vec<ProviderModelConfig>> {
        None
    }
    fn set_models(&self, value: Option<Vec<ProviderModelConfig>>);
    /// Refresh this provider's model list. The returned list replaces extension-provided models.
    /// Use context.publish({ persist: entry }) when the catalog should persist across sessions.
    /// Optional in TS: the default implementation means "not provided".
    async fn refresh_models(
        &self,
        context: RefreshModelsContext,
    ) -> anyhow::Result<Vec<ProviderModelConfig>> {
        todo!("optional method not provided")
    }
    /// OAuth provider for /login support. The `id` is set automatically from the provider name.
    /// Optional in TS: the default implementation means "not provided".
    fn oauth(&self) -> Option<ProviderConfigOauth> {
        None
    }
    fn set_oauth(&self, value: Option<ProviderConfigOauth>);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelConfigBase {
    /// Model ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// API type override for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// API endpoint URL override for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Supported input types.
    pub input: Vec<ExtensionVirtualModelBaseInput>,
    /// Provider input limits and cache-safe image preprocessing metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limits: Option<ModelInputLimits>,
    /// Per-million-token cost rates and optional request-wide input pricing tiers.
    pub cost: ModelCost,
    /// Custom headers for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
}

/// Chat model configuration. Omitted `type` is normalized to `"chat"`.
// TS: extends ProviderModelConfigBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderChatModelConfig {
    /// Model ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// API endpoint URL override for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Supported input types.
    pub input: Vec<ExtensionVirtualModelBaseInput>,
    /// Provider input limits and cache-safe image preprocessing metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limits: Option<ModelInputLimits>,
    /// Per-million-token cost rates and optional request-wide input pricing tiers.
    pub cost: ModelCost,
    /// Custom headers for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<Api>,
    /// Whether the model supports extended thinking.
    pub reasoning: bool,
    /// Maps pi thinking levels to provider/model-specific values; null marks a level unsupported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level_map: Option<ThinkingLevelMap>,
    /// Best-effort prompt cache lifetime in seconds per retention tier. Unset disables cache warming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<ModelPromptCache>,
    /// Maximum context window size in tokens.
    pub context_window: u64,
    /// Maximum output tokens.
    pub max_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling_params: Option<IndexMap<String, Value>>,
    /// OpenAI compatibility settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<Value>,
}

/// Image-generation model configuration.
// TS: extends ProviderModelConfigBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImageModelConfig {
    /// Model ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// API endpoint URL override for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Supported input types.
    pub input: Vec<ExtensionVirtualModelBaseInput>,
    /// Provider input limits and cache-safe image preprocessing metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limits: Option<ModelInputLimits>,
    /// Per-million-token cost rates and optional request-wide input pricing tiers.
    pub cost: ModelCost,
    /// Custom headers for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    /// Always "image".
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<ImageApi>,
    pub output: Vec<ExtensionVirtualModelBaseInput>,
}

/// Structured classifier model configuration.
// TS: extends ProviderModelConfigBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderClassifierModelConfig {
    /// Model ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// API endpoint URL override for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Supported input types.
    pub input: Vec<ExtensionVirtualModelBaseInput>,
    /// Provider input limits and cache-safe image preprocessing metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limits: Option<ModelInputLimits>,
    /// Per-million-token cost rates and optional request-wide input pricing tiers.
    pub cost: ModelCost,
    /// Custom headers for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    /// Always "classifier".
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<ClassifierApi>,
    pub context_window: u64,
}

/// Configuration for a model within a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProviderModelConfig {
    ProviderChatModelConfig(ProviderChatModelConfig),
    ProviderImageModelConfig(ProviderImageModelConfig),
    ProviderClassifierModelConfig(ProviderClassifierModelConfig),
}

/// Extension factory function type. Supports both sync and async initialization.
pub type ExtensionFactory =
    Arc<dyn Fn(Arc<dyn ExtensionAPI>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;

#[derive(Clone)]
pub struct InlineExtensionVariant2 {
    /// Display name shown as `<inline:name>` in the startup Extensions list and errors. With
    /// `builtin`, the extension is named `builtin:name` in errors and diagnostics.
    pub name: String,
    pub factory: ExtensionFactory,
    /// Omit this extension from the startup Extensions list.
    pub hidden: Option<bool>,
    /// Leave this extension out when another extension registers a tool, command, or flag with a
    /// name it registers during loading, instead of reporting a conflict. The CLI's built-in MCP,
    /// codemode, and tool search extensions use it, so for example an MCP extension that registers
    /// `/mcp` replaces the built-in MCP support. The factory still runs, so it should only register
    /// tools, commands, flags, and event handlers.
    pub replaceable: Option<bool>,
    /// Supply the code of the `builtin:<name>` extension instead of loading as an inline extension.
    /// `builtin:<name>` is an extension resource like a file: it loads by default, `pi config` lists
    /// it, `-builtin:<name>` in the `extensions` setting and `--no-extensions` disable it, and
    /// `-e builtin:<name>` loads it explicitly. It is hidden from the startup Extensions list and
    /// loads after project trust is resolved, so it cannot handle `project_trust`. The CLI's built-in
    /// extensions use it.
    pub builtin: Option<bool>,
}

#[derive(Clone)]
pub enum InlineExtension {
    ExtensionFactory(ExtensionFactory),
    InlineExtensionVariant2(InlineExtensionVariant2),
}

#[derive(Clone)]
pub struct RegisteredTool {
    pub definition: Arc<dyn ToolDefinition>,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExtensionFlagType {
    #[serde(rename = "boolean")]
    Boolean,
    #[serde(rename = "string")]
    String,
}

impl ExtensionFlagType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::String => "string",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtensionFlagDefault {
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionFlag {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub r#type: ExtensionFlagType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<ExtensionFlagDefault>,
    pub extension_path: String,
}

#[derive(Clone)]
pub struct ExtensionShortcut {
    pub shortcut: String,
    pub description: Option<String>,
    pub handler: Arc<
        dyn Fn(Arc<dyn ExtensionContext>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync,
    >,
    pub extension_path: String,
}

pub type HandlerFn =
    Arc<dyn Fn(Vec<Vec<Value>>) -> BoxFuture<'static, anyhow::Result<Value>> + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageHandlerMessage {
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageHandlerOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_turn: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliver_as: Option<ReplacedSessionContextOptionsDeliverAs>,
}

pub type SendMessageHandler =
    Arc<dyn Fn(SendMessageHandlerMessage, Option<SendMessageHandlerOptions>) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendUserMessageHandlerOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliver_as: Option<ReplacedSessionContextOptions2DeliverAs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand_prompt_templates: Option<bool>,
}

pub type SendUserMessageHandler =
    Arc<dyn Fn(UserContent, Option<SendUserMessageHandlerOptions>) + Send + Sync>;

pub type AppendEntryHandler = Arc<dyn Fn(String, Option<Value>) + Send + Sync>;

pub type SetSessionNameHandler = Arc<dyn Fn(String) + Send + Sync>;

pub type GetSessionNameHandler = Arc<dyn Fn() -> Option<String> + Send + Sync>;

pub type GetActiveToolsHandler = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolInfoResult {
    String(String),
    List(Vec<String>),
}

/// Tool info with name, description, parameter schema, prompt guidelines, and source metadata.
// TS: Pick< ToolDefinition, "name" | "description" | "parameters" | "promptGuidelines" | "reachesTools" > & { exposure: ToolExposure; namespace?: ToolNamespace; annotations?: ToolAnnotations; sourceInfo: So
#[derive(Clone)]
pub struct ToolInfo {
    /// Tool name (used in LLM tool calls)
    pub name: String,
    /// Description for LLM
    pub description: String,
    /// Parameter schema (TypeBox)
    pub parameters: Value,
    /// Optional guideline bullets appended to the default system prompt Guidelines section when this tool is active.
    pub prompt_guidelines: Option<Vec<String>>,
    /// For tools that reach other tools (such as `codemode`, which calls them, and `tool_search`, which
    /// loads them): which tools a call with `input` may reach. `"all"`, or the words of the input that
    /// may name them, where a word refers to a tool or namespace whose name it contains, so words may
    /// over-match. Extensions that register tools late, such as MCP servers that are still connecting,
    /// use it from their `tool_call` handler to finish registering those tools before the call runs.
    pub reaches_tools: Option<Arc<dyn Fn(IndexMap<String, Value>) -> ToolInfoResult + Send + Sync>>,
    pub exposure: ToolExposure,
    pub namespace: Option<ToolNamespace>,
    pub annotations: Option<ToolAnnotations>,
    pub source_info: SourceInfo,
}

pub type GetAllToolsHandler = Arc<dyn Fn() -> Vec<ToolInfo> + Send + Sync>;

pub type GetSettingsHandler = Arc<dyn Fn() -> Settings + Send + Sync>;

pub type GetCommandsHandler = Arc<dyn Fn() -> Vec<SlashCommandInfo> + Send + Sync>;

pub type SetActiveToolsHandler = Arc<dyn Fn(Vec<String>) + Send + Sync>;

pub type RefreshToolsHandler = Arc<dyn Fn() + Send + Sync>;

pub type SetModelHandler =
    Arc<dyn Fn(Model) -> BoxFuture<'static, anyhow::Result<bool>> + Send + Sync>;

pub type GetThinkingLevelHandler = Arc<dyn Fn() -> ThinkingLevel + Send + Sync>;

pub type SetThinkingLevelHandler = Arc<dyn Fn(ThinkingLevel) + Send + Sync>;

pub type SetLabelHandler = Arc<dyn Fn(String, Option<String>) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtensionRuntimeStateFlagValues {
    Bool(bool),
    String(String),
}

#[derive(Clone)]
pub struct ExtensionRuntimeStatePendingProviderRegistrations {
    pub name: String,
    pub config: Arc<dyn ProviderConfig>,
    pub extension_path: String,
}

#[derive(Clone)]
pub struct ExtensionRuntimeStatePendingNativeProviderRegistrations {
    pub provider: Arc<dyn Provider>,
    pub extension_path: String,
}

#[derive(Clone)]
pub struct ExtensionRuntimeStatePendingVirtualModelRegistrations {
    pub definition: Arc<dyn VirtualModelDefinition>,
    pub extension_path: String,
}

/// Shared state created by loader, used during registration and runtime.
/// Contains flag values (defaults set during registration, CLI values set after).
#[derive(Clone)]
pub struct ExtensionRuntimeState {
    pub flag_values: IndexMap<String, ExtensionRuntimeStateFlagValues>,
    /// Legacy provider-config registrations queued during extension loading, processed when runner binds.
    pub pending_provider_registrations: Vec<ExtensionRuntimeStatePendingProviderRegistrations>,
    /// Native pi-ai provider registrations queued during extension loading, processed when runner binds.
    pub pending_native_provider_registrations:
        Vec<ExtensionRuntimeStatePendingNativeProviderRegistrations>,
    /// Virtual model registrations queued during extension loading, processed when runner binds.
    pub pending_virtual_model_registrations:
        Vec<ExtensionRuntimeStatePendingVirtualModelRegistrations>,
    /// Create an extension context. Throws before the runner binds.
    pub create_context: Arc<dyn Fn() -> Arc<dyn ExtensionContext> + Send + Sync>,
    /// Throws when this extension instance is stale after runtime replacement.
    pub assert_active: Arc<dyn Fn() + Send + Sync>,
    /// Marks this extension instance as stale after runtime replacement or reload.
    pub invalidate: Arc<dyn Fn(Option<String>) + Send + Sync>,
    /// Retain an event-bus subscription until this runtime is invalidated.
    pub track_event_bus_subscription:
        Arc<dyn Fn(Arc<dyn Fn() + Send + Sync>) -> Arc<dyn Fn() + Send + Sync> + Send + Sync>,
    /// Register or unregister a provider.
    ///
    /// Before bindCore(): queues registrations / removes from queue.
    /// After bindCore(): calls ModelRegistry directly for immediate effect.
    pub register_provider:
        Arc<dyn Fn(String, Arc<dyn ProviderConfig>, Option<String>) + Send + Sync>,
    pub register_native_provider: Arc<dyn Fn(Arc<dyn Provider>, Option<String>) + Send + Sync>,
    pub unregister_provider: Arc<dyn Fn(String, Option<String>) + Send + Sync>,
    /// Servers registered with `pi.registerMcpServer()`.
    pub mcp_servers: Arc<McpServerRegistry>,
    pub register_virtual_model:
        Arc<dyn Fn(Arc<dyn VirtualModelDefinition>, Option<String>) + Send + Sync>,
    pub unregister_virtual_model: Arc<dyn Fn(String, String) + Send + Sync>,
}

/// Action implementations for pi.* API methods.
/// Provided to runner.initialize(), copied into the shared runtime.
#[derive(Clone)]
pub struct ExtensionActions {
    pub send_message: SendMessageHandler,
    pub send_user_message: SendUserMessageHandler,
    pub append_entry: AppendEntryHandler,
    pub set_session_name: SetSessionNameHandler,
    pub get_session_name: GetSessionNameHandler,
    pub set_label: SetLabelHandler,
    pub get_active_tools: GetActiveToolsHandler,
    pub get_all_tools: GetAllToolsHandler,
    pub get_settings: GetSettingsHandler,
    pub set_active_tools: SetActiveToolsHandler,
    pub refresh_tools: RefreshToolsHandler,
    pub get_commands: GetCommandsHandler,
    pub set_model: SetModelHandler,
    pub get_thinking_level: GetThinkingLevelHandler,
    pub set_thinking_level: SetThinkingLevelHandler,
}

/// Actions for ExtensionContext (ctx.* in event handlers).
/// Required by all modes.
#[derive(Clone)]
pub struct ExtensionContextActions {
    pub get_model: Arc<dyn Fn() -> Option<Model> + Send + Sync>,
    pub get_scoped_models: Arc<dyn Fn() -> Vec<ScopedModel> + Send + Sync>,
    pub is_idle: Arc<dyn Fn() -> bool + Send + Sync>,
    pub is_project_trusted: Arc<dyn Fn() -> bool + Send + Sync>,
    pub get_signal: Arc<dyn Fn() -> Option<CancellationToken> + Send + Sync>,
    pub abort: Arc<dyn Fn() + Send + Sync>,
    pub has_pending_messages: Arc<dyn Fn() -> bool + Send + Sync>,
    pub shutdown: Arc<dyn Fn() + Send + Sync>,
    pub get_context_usage: Arc<dyn Fn() -> Option<ContextUsage> + Send + Sync>,
    pub compact: Arc<dyn Fn(Option<CompactOptions>) + Send + Sync>,
    pub get_system_prompt: Arc<dyn Fn() -> String + Send + Sync>,
    pub get_system_prompt_options: Option<Arc<dyn Fn() -> BuildSystemPromptOptions + Send + Sync>>,
    /// Backs `ExtensionToolContext.executeTool()`. Without it, nested calls fail.
    pub execute_tool: Option<
        Arc<
            dyn Fn(
                    String,
                    String,
                    Value,
                    ExecuteToolOptions,
                ) -> BoxFuture<'static, anyhow::Result<AgentToolCallOutcome>>
                + Send
                + Sync,
        >,
    >,
    /// Backs `ExtensionToolContext.tools`.
    pub get_callable_tools: Option<Arc<dyn Fn() -> Vec<AgentToolRef> + Send + Sync>>,
}

#[derive(Clone)]
pub struct ExtensionCommandContextActionsOptions {
    pub parent_session: Option<String>,
    pub setup: Option<
        Arc<dyn Fn(Arc<SessionManager>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
    pub with_session: Option<
        Arc<
            dyn Fn(Arc<dyn ReplacedSessionContext>) -> BoxFuture<'static, anyhow::Result<()>>
                + Send
                + Sync,
        >,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommandContextActionsResult {
    pub cancelled: bool,
}

#[derive(Clone)]
pub struct ExtensionCommandContextActionsOptions2 {
    pub position: Option<ExtensionCommandContextOptions2Position>,
    pub with_session: Option<
        Arc<
            dyn Fn(Arc<dyn ReplacedSessionContext>) -> BoxFuture<'static, anyhow::Result<()>>
                + Send
                + Sync,
        >,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommandContextActionsResult2 {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommandContextActionsOptions3 {
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
pub struct ExtensionCommandContextActionsResult3 {
    pub cancelled: bool,
}

#[derive(Clone)]
pub struct ExtensionCommandContextActionsOptions4 {
    pub with_session: Option<
        Arc<
            dyn Fn(Arc<dyn ReplacedSessionContext>) -> BoxFuture<'static, anyhow::Result<()>>
                + Send
                + Sync,
        >,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommandContextActionsResult4 {
    pub cancelled: bool,
}

/// Actions for ExtensionCommandContext (ctx.* in command handlers).
/// Only needed for interactive mode where extension commands are invokable.
#[derive(Clone)]
pub struct ExtensionCommandContextActions {
    pub wait_for_idle: Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    pub new_session: Arc<
        dyn Fn(
                Option<ExtensionCommandContextActionsOptions>,
            )
                -> BoxFuture<'static, anyhow::Result<ExtensionCommandContextActionsResult>>
            + Send
            + Sync,
    >,
    pub fork: Arc<
        dyn Fn(
                String,
                Option<ExtensionCommandContextActionsOptions2>,
            )
                -> BoxFuture<'static, anyhow::Result<ExtensionCommandContextActionsResult2>>
            + Send
            + Sync,
    >,
    pub navigate_tree: Arc<
        dyn Fn(
                String,
                Option<ExtensionCommandContextActionsOptions3>,
            )
                -> BoxFuture<'static, anyhow::Result<ExtensionCommandContextActionsResult3>>
            + Send
            + Sync,
    >,
    pub switch_session: Arc<
        dyn Fn(
                String,
                Option<ExtensionCommandContextActionsOptions4>,
            )
                -> BoxFuture<'static, anyhow::Result<ExtensionCommandContextActionsResult4>>
            + Send
            + Sync,
    >,
    pub reload: Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtensionRuntimeFlagValues {
    Bool(bool),
    String(String),
}

#[derive(Clone)]
pub struct ExtensionRuntimePendingProviderRegistrations {
    pub name: String,
    pub config: Arc<dyn ProviderConfig>,
    pub extension_path: String,
}

#[derive(Clone)]
pub struct ExtensionRuntimePendingNativeProviderRegistrations {
    pub provider: Arc<dyn Provider>,
    pub extension_path: String,
}

#[derive(Clone)]
pub struct ExtensionRuntimePendingVirtualModelRegistrations {
    pub definition: Arc<dyn VirtualModelDefinition>,
    pub extension_path: String,
}

/// Full runtime = state + actions.
/// Created by loader with throwing action stubs, completed by runner.initialize().
// TS: extends ExtensionRuntimeState, ExtensionActions (inherited fields inlined)
#[derive(Clone)]
pub struct ExtensionRuntime {
    pub flag_values: IndexMap<String, ExtensionRuntimeFlagValues>,
    /// Legacy provider-config registrations queued during extension loading, processed when runner binds.
    pub pending_provider_registrations: Vec<ExtensionRuntimePendingProviderRegistrations>,
    /// Native pi-ai provider registrations queued during extension loading, processed when runner binds.
    pub pending_native_provider_registrations:
        Vec<ExtensionRuntimePendingNativeProviderRegistrations>,
    /// Virtual model registrations queued during extension loading, processed when runner binds.
    pub pending_virtual_model_registrations: Vec<ExtensionRuntimePendingVirtualModelRegistrations>,
    /// Create an extension context. Throws before the runner binds.
    pub create_context: Arc<dyn Fn() -> Arc<dyn ExtensionContext> + Send + Sync>,
    /// Throws when this extension instance is stale after runtime replacement.
    pub assert_active: Arc<dyn Fn() + Send + Sync>,
    /// Marks this extension instance as stale after runtime replacement or reload.
    pub invalidate: Arc<dyn Fn(Option<String>) + Send + Sync>,
    /// Retain an event-bus subscription until this runtime is invalidated.
    pub track_event_bus_subscription:
        Arc<dyn Fn(Arc<dyn Fn() + Send + Sync>) -> Arc<dyn Fn() + Send + Sync> + Send + Sync>,
    /// Register or unregister a provider.
    ///
    /// Before bindCore(): queues registrations / removes from queue.
    /// After bindCore(): calls ModelRegistry directly for immediate effect.
    pub register_provider:
        Arc<dyn Fn(String, Arc<dyn ProviderConfig>, Option<String>) + Send + Sync>,
    pub register_native_provider: Arc<dyn Fn(Arc<dyn Provider>, Option<String>) + Send + Sync>,
    pub unregister_provider: Arc<dyn Fn(String, Option<String>) + Send + Sync>,
    /// Servers registered with `pi.registerMcpServer()`.
    pub mcp_servers: Arc<McpServerRegistry>,
    pub register_virtual_model:
        Arc<dyn Fn(Arc<dyn VirtualModelDefinition>, Option<String>) + Send + Sync>,
    pub unregister_virtual_model: Arc<dyn Fn(String, String) + Send + Sync>,
    pub send_message: SendMessageHandler,
    pub send_user_message: SendUserMessageHandler,
    pub append_entry: AppendEntryHandler,
    pub set_session_name: SetSessionNameHandler,
    pub get_session_name: GetSessionNameHandler,
    pub set_label: SetLabelHandler,
    pub get_active_tools: GetActiveToolsHandler,
    pub get_all_tools: GetAllToolsHandler,
    pub get_settings: GetSettingsHandler,
    pub set_active_tools: SetActiveToolsHandler,
    pub refresh_tools: RefreshToolsHandler,
    pub get_commands: GetCommandsHandler,
    pub set_model: SetModelHandler,
    pub get_thinking_level: GetThinkingLevelHandler,
    pub set_thinking_level: SetThinkingLevelHandler,
}

/// Loaded extension with all registered items.
#[derive(Clone)]
pub struct Extension {
    pub path: String,
    pub resolved_path: String,
    pub hidden: Option<bool>,
    /// See {@link InlineExtension}.
    pub replaceable: Option<bool>,
    pub source_info: SourceInfo,
    pub handlers: IndexMap<String, Vec<HandlerFn>>,
    pub tools: IndexMap<String, RegisteredTool>,
    pub message_renderers: IndexMap<String, MessageRenderer>,
    pub tool_renderers: Option<Vec<ToolRendererResolver>>,
    pub markdown_transformer: Option<MarkdownTransformer>,
    pub entry_renderers: Option<IndexMap<String, EntryRenderer>>,
    pub commands: IndexMap<String, RegisteredCommand>,
    pub flags: IndexMap<String, ExtensionFlag>,
    pub shortcuts: IndexMap<String, ExtensionShortcut>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadExtensionsResultErrors {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadExtensionsResultWarnings {
    pub path: String,
    pub warning: String,
}

/// Result of loading extensions.
#[derive(Clone)]
pub struct LoadExtensionsResult {
    pub extensions: Vec<Extension>,
    pub errors: Vec<LoadExtensionsResultErrors>,
    pub warnings: Option<Vec<LoadExtensionsResultWarnings>>,
    /// Shared runtime - actions are throwing stubs until runner.initialize()
    pub runtime: ExtensionRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionError {
    pub extension_path: String,
    pub event: String,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
}
