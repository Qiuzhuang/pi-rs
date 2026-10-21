//! Port of `packages/coding-agent/src/modes/interactive/interactive-mode.ts`.

use crate::core::agent_session_runtime::AgentSessionRuntime;
use crate::core::agent_session_services::AgentSessionRuntimeDiagnostic;
use crate::core::session_manager::SessionEntry;
use crate::core::session_manager::SessionManager;
use crate::core::settings_schema::TuiMode;
use crate::modes::interactive::components::status_indicator::StatusIndicator;
use crate::utils::version_check::LatestPiRelease;
use pi_ai::types::AssistantMessage;
use pi_ai::types::ImageContent;
use pi_ai::types::SystemMessage;
use pi_ai::types::ToolResultMessage;
use pi_ai::types::Usage;
use pi_ai::types::UserMessage;
use pi_tui::components::editor::EditorComponent;
use pi_tui::terminal::Terminal;
use pi_tui::tui::Component;
use pi_tui::tui::MouseEvent;
use pi_tui::tui::MouseResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub use crate::modes::interactive::tui_renderer::create_interactive_tui;
pub use crate::modes::interactive::tui_renderer::create_interactive_tui_reference;

/// Interface for components that can be expanded/collapsed
pub trait Expandable: Send + Sync {
    fn set_expanded(&self, expanded: bool);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveEditorGetCursorResult {
    pub line: i64,
    pub col: i64,
}

pub trait ActiveEditor: EditorComponent + Send + Sync {
    /// Optional in TS: the default implementation means "not provided".
    fn get_cursor(&self) -> ActiveEditorGetCursorResult {
        todo!("optional method not provided")
    }
}

pub trait WorkingStatusEditor: EditorComponent + Send + Sync {
    fn embed_working_status(&self) -> bool;
    fn set_working_status_indicator(&self, indicator: Option<Arc<StatusIndicator>>);
}

/// TS: `extends ThemedText implements Expandable`
pub struct ExpandableText {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ExpandableText {
    pub fn new(
        get_collapsed_text: Arc<dyn Fn() -> String + Send + Sync>,
        get_expanded_text: Arc<dyn Fn() -> String + Send + Sync>,
        expanded: Option<bool>,
        padding_x: Option<i64>,
        padding_y: Option<i64>,
    ) -> Self {
        todo!("port modes/interactive/interactive-mode.ts:ExpandableText.constructor")
    }
}

impl Expandable for ExpandableText {
    fn set_expanded(&self, expanded: bool) {
        todo!("port modes/interactive/interactive-mode.ts:ExpandableText.set_expanded")
    }
}

impl Component for ExpandableText {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/interactive-mode.ts:ExpandableText.render")
    }
}

/// The built-in header. Clicking its logo (the first two lines, after one column of padding) plays an easter egg.
/// TS: `extends ExpandableText`
pub struct BuiltInHeader {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl BuiltInHeader {
    pub fn new() -> Self {
        todo!("port modes/interactive/interactive-mode.ts:BuiltInHeader.constructor")
    }

    pub fn on_logo_click(&self) -> Option<Arc<dyn Fn(i64, i64) + Send + Sync>> {
        todo!("port modes/interactive/interactive-mode.ts:BuiltInHeader.onLogoClick")
    }

    pub fn set_on_logo_click(&self, value: Option<Arc<dyn Fn(i64, i64) + Send + Sync>>) {
        todo!("port modes/interactive/interactive-mode.ts:BuiltInHeader.onLogoClick")
    }
}

impl Component for BuiltInHeader {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/interactive-mode.ts:BuiltInHeader.render")
    }
}

// TS: Component & { dispose?(): void }
#[derive(Clone)]
pub struct ExtensionDialogComponent {
    /// Render the component to lines for the given viewport width
    /// @param width - Current viewport width
    /// @returns Array of strings, each representing a line
    pub render: Arc<dyn Fn(i64) -> Vec<String> + Send + Sync>,
    /// Optional handler for keyboard input when component has focus.
    pub handle_input: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// Optional normalized mouse handler.
    pub handle_mouse: Option<Arc<dyn Fn(MouseEvent) -> Option<Arc<MouseResult>> + Send + Sync>>,
    /// If true, component receives key release events (Kitty protocol).
    /// Default is false - release events are filtered out.
    pub wants_key_release: Option<bool>,
    /// Invalidate any cached rendering state.
    /// Called when theme changes or when component needs to re-render from scratch.
    pub invalidate: Arc<dyn Fn() + Send + Sync>,
    pub dispose: Option<Arc<dyn Fn() + Send + Sync>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompactionQueuedMessageMode {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

impl CompactionQueuedMessageMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::FollowUp => "followUp",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionQueuedMessage {
    pub text: String,
    pub mode: CompactionQueuedMessageMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompactionCostNoticeKind {
    #[serde(rename = "compaction")]
    Compaction,
    #[serde(rename = "branch_summary")]
    BranchSummary,
}

impl CompactionCostNoticeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Compaction => "compaction",
            Self::BranchSummary => "branch_summary",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionCostNotice {
    /// Always "compaction_cost".
    pub r#type: String,
    pub kind: CompactionCostNoticeKind,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RenderSessionItem {
    SystemMessage(SystemMessage),
    UserMessage(UserMessage),
    AssistantMessage(AssistantMessage),
    ToolResultMessage(ToolResultMessage),
    Value(Value),
    SessionEntry(SessionEntry),
    CompactionCostNotice(CompactionCostNotice),
}

pub fn format_crash_extension_hint(extension_matches: Option<&[String]>) -> Option<String> {
    todo!("port modes/interactive/interactive-mode.ts:formatCrashExtensionHint")
}

pub fn format_resume_command(session_manager: Arc<SessionManager>) -> Option<String> {
    todo!("port modes/interactive/interactive-mode.ts:formatResumeCommand")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LoginProviderCompletionOptionAuthTypes {
    #[serde(rename = "oauth")]
    Oauth,
    #[serde(rename = "api_key")]
    ApiKey,
}

impl LoginProviderCompletionOptionAuthTypes {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Oauth => "oauth",
            Self::ApiKey => "api_key",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginProviderCompletionOption {
    pub id: String,
    pub name: String,
    pub auth_types: Vec<LoginProviderCompletionOptionAuthTypes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription: Option<bool>,
}

/// Options for InteractiveMode initialization.
#[derive(Clone)]
pub struct InteractiveModeOptions {
    /// Providers that were migrated to auth.json (shows warning)
    pub migrated_providers: Option<Vec<String>>,
    /// Diagnostics collected before the interactive TUI was initialized.
    pub startup_diagnostics: Option<Vec<AgentSessionRuntimeDiagnostic>>,
    /// Warning message if session model couldn't be restored
    pub model_fallback_message: Option<String>,
    /// Cwd to trust after reload if it gained a .pi directory during this implicitly trusted session.
    pub auto_trust_on_reload_cwd: Option<String>,
    /// Initial message to send on startup (can include @file content)
    pub initial_message: Option<String>,
    /// Images to attach to the initial message
    pub initial_images: Option<Vec<ImageContent>>,
    /// Additional messages to send after the initial message
    pub initial_messages: Option<Vec<String>>,
    /// Force verbose startup (overrides quietStartup setting)
    pub verbose: Option<bool>,
    /// TUI layout mode.
    pub tui_mode: Option<TuiMode>,
    /// Initial interactive theme setting for this invocation.
    pub initial_theme_setting: Option<String>,
    /// Terminal implementation. Defaults to the current process terminal.
    pub terminal: Option<Arc<dyn Terminal>>,
}

pub struct InteractiveMode {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl InteractiveMode {
    pub fn new(
        runtime_host: Arc<AgentSessionRuntime>,
        options: Option<&InteractiveModeOptions>,
    ) -> Self {
        todo!("port modes/interactive/interactive-mode.ts:InteractiveMode.constructor")
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        todo!("port modes/interactive/interactive-mode.ts:InteractiveMode.init")
    }

    /// Run the interactive mode. This is the main entry point.
    /// Initializes the UI, shows warnings, processes initial messages, and starts the interactive loop.
    pub async fn run(&self) -> anyhow::Result<()> {
        todo!("port modes/interactive/interactive-mode.ts:InteractiveMode.run")
    }

    pub fn render_initial_messages(&self) {
        todo!("port modes/interactive/interactive-mode.ts:InteractiveMode.renderInitialMessages")
    }

    pub async fn get_user_input(&self) -> anyhow::Result<String> {
        todo!("port modes/interactive/interactive-mode.ts:InteractiveMode.getUserInput")
    }

    pub fn clear_editor(&self) {
        todo!("port modes/interactive/interactive-mode.ts:InteractiveMode.clearEditor")
    }

    pub fn show_error(&self, error_message: &str) {
        todo!("port modes/interactive/interactive-mode.ts:InteractiveMode.showError")
    }

    pub fn show_warning(&self, warning_message: &str) {
        todo!("port modes/interactive/interactive-mode.ts:InteractiveMode.showWarning")
    }

    pub fn show_new_version_notification(&self, release: &LatestPiRelease) {
        todo!(
            "port modes/interactive/interactive-mode.ts:InteractiveMode.showNewVersionNotification"
        )
    }

    pub fn show_package_update_notification(&self, packages: &[String]) {
        todo!(
            "port modes/interactive/interactive-mode.ts:InteractiveMode.showPackageUpdateNotification"
        )
    }

    pub fn stop(&self, fullscreen_exit_output: Option<Value>) {
        todo!("port modes/interactive/interactive-mode.ts:InteractiveMode.stop")
    }
}
