//! Test hooks into `InteractiveMode` internals.
//!
//! The TypeScript interactive-mode tests call private methods on fake `this` objects. Rust tests drive
//! a real `InteractiveMode` instead and reach the private methods they exercise through these hooks.

use super::*;
use crate::core::agent_session::AgentSessionEvent;
use crate::modes::interactive::components::status_indicator::StatusIndicatorKind;

#[doc(hidden)]
#[derive(Clone)]
pub struct InteractiveModeTestHooks {
    pub(crate) inner: Arc<Inner>,
}

impl InteractiveMode {
    /// Hooks into private state for tests.
    #[doc(hidden)]
    pub fn test_hooks(&self) -> InteractiveModeTestHooks {
        InteractiveModeTestHooks { inner: self.inner.clone() }
    }
}

#[doc(hidden)]
impl InteractiveModeTestHooks {
    /// Text of the active editor.
    pub fn editor_text(&self) -> String {
        self.inner.editor().get_text()
    }

    pub fn set_editor_text(&self, text: &str) {
        self.inner.editor().set_text(text);
    }

    /// TS `handleEvent`.
    pub fn handle_event(&self, event: AgentSessionEvent) {
        self.inner.handle_event(event);
    }

    /// Number of components in the chat container.
    pub fn chat_children(&self) -> usize {
        self.inner.chat_container.children().len()
    }

    /// Render the chat container.
    pub fn render_chat(&self, width: usize) -> Vec<String> {
        self.inner.chat_container.render(width)
    }

    /// TS `addCompactionCostNotice`.
    pub fn add_compaction_cost_notice(&self, kind: CompactionCostNoticeKind, usage: Usage) {
        self.inner.add_compaction_cost_notice(&CompactionCostNotice { r#type: "compaction_cost".to_string(), kind, usage });
    }

    /// TS `renderSessionEntries` (without footer update or history).
    pub fn render_session_entries(&self, entries: &[SessionEntry]) {
        self.inner.render_session_entries(entries, false, false);
    }

    /// TS `restoreQueuedMessagesToEditor`.
    pub fn restore_queued_messages_to_editor(&self, abort: bool) -> usize {
        self.inner.restore_queued_messages_to_editor(abort, None)
    }

    /// TS `queueCompactionMessage`.
    pub fn queue_compaction_message(&self, text: &str, mode: CompactionQueuedMessageMode) {
        self.inner.queue_compaction_message(text, mode);
    }

    /// TS `compactionQueuedMessages`.
    pub fn compaction_queued_messages(&self) -> Vec<CompactionQueuedMessage> {
        self.inner.state.lock().compaction_queued_messages.clone()
    }

    /// TS `flushCompactionQueue`.
    pub async fn flush_compaction_queue(&self, will_retry: bool) {
        self.inner.clone().flush_compaction_queue(will_retry).await;
    }

    /// Replace the process signal operations used by suspend (Ctrl+Z).
    pub fn set_suspend_signals(&self, signals: Arc<dyn SuspendSignals>) {
        self.inner.state.lock().suspend_signals = Some(signals);
    }

    /// TS `handleCtrlZ` (suspend to background).
    pub fn suspend(&self) -> std::io::Result<()> {
        self.inner.handle_ctrl_z()
    }

    /// TS `setWorkingVisible`.
    pub fn set_working_visible(&self, visible: bool) {
        self.inner.set_working_visible(visible);
    }

    /// TS `switchTuiMode`.
    pub fn switch_tui_mode(&self, mode: TuiMode, restore_progress: bool) -> bool {
        self.inner.switch_tui_mode(mode, restore_progress, true)
    }

    /// TS `stopInteractiveTui`.
    pub fn stop_interactive_tui(&self, output: FullscreenExitOutput) {
        self.inner.stop_interactive_tui(output);
    }

    /// TS `handleRightClickPaste`.
    pub async fn handle_right_click_paste(&self) {
        self.inner.clone().handle_right_click_paste().await;
    }

    /// TS `handleCopyCommand`.
    pub async fn handle_copy_command(&self, flash_confirmation: bool, prefer_selection: bool) {
        self.inner.clone().handle_copy_command(flash_confirmation, prefer_selection).await;
    }

    /// TS `showTreeSelector`.
    pub fn show_tree_selector(&self) {
        self.inner.show_tree_selector(None);
    }

    /// The default editor's escape handler.
    pub fn default_editor_on_escape(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.inner.default_editor.on_escape()
    }

    /// Number of components in the status container (the row above the editor).
    pub fn status_container_children(&self) -> usize {
        self.inner.status_container.children().len()
    }

    pub fn active_status_indicator_kind(&self) -> Option<StatusIndicatorKind> {
        self.inner.active_indicator_kind()
    }

    /// TS `activeWorkingIndicatorEmbedded`.
    pub fn working_indicator_embedded(&self) -> bool {
        self.inner.state.lock().active_working_indicator_embedded
    }
}
