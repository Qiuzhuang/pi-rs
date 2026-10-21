//! Port of `packages/coding-agent/src/modes/interactive/components/custom-editor.ts`.

use crate::core::keybindings::AppKeybinding;
use crate::core::keybindings::KeybindingsManager;
use crate::modes::interactive::components::status_indicator::StatusIndicator;
use indexmap::IndexMap;
use pi_tui::components::editor::EditorTheme;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// TS: EditorOptions & { /** Render working, compaction, summarization, and retry status in the editor's top border. */ embedWorkingStatus?: boolean; }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEditorOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_x: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autocomplete_max_visible: Option<i64>,
    /// Render working, compaction, summarization, and retry status in the editor's top border.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed_working_status: Option<bool>,
}

/// Custom editor that handles app-level keybindings for coding-agent.
/// TS: `extends Editor`
pub struct CustomEditor {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl CustomEditor {
    pub fn new(
        tui: Arc<Tui>,
        theme: Arc<EditorTheme>,
        keybindings: Arc<KeybindingsManager>,
        options: Option<&CustomEditorOptions>,
    ) -> Self {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.constructor")
    }

    pub fn embed_working_status(&self) -> bool {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.embedWorkingStatus")
    }

    pub fn action_handlers(&self) -> IndexMap<AppKeybinding, Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.actionHandlers")
    }

    pub fn set_action_handlers(&self, value: IndexMap<AppKeybinding, Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.actionHandlers")
    }

    pub fn on_escape(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.onEscape")
    }

    pub fn set_on_escape(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.onEscape")
    }

    pub fn on_ctrl_d(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.onCtrlD")
    }

    pub fn set_on_ctrl_d(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.onCtrlD")
    }

    pub fn on_paste_image(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.onPasteImage")
    }

    pub fn set_on_paste_image(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.onPasteImage")
    }

    /// Handler for extension-registered shortcuts. Returns true if handled.
    pub fn on_extension_shortcut(&self) -> Option<Arc<dyn Fn(String) -> bool + Send + Sync>> {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.onExtensionShortcut")
    }

    pub fn set_on_extension_shortcut(
        &self,
        value: Option<Arc<dyn Fn(String) -> bool + Send + Sync>>,
    ) {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.onExtensionShortcut")
    }

    pub fn set_working_status_indicator(&self, indicator: Option<Arc<StatusIndicator>>) {
        todo!(
            "port modes/interactive/components/custom-editor.ts:CustomEditor.setWorkingStatusIndicator"
        )
    }

    /// Register a handler for an app action.
    pub fn on_action(&self, action: &AppKeybinding, handler: Arc<dyn Fn() + Send + Sync>) {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.onAction")
    }
}

// TODO(port): impl pi-tui::Editor for CustomEditor

impl Component for CustomEditor {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port modes/interactive/components/custom-editor.ts:CustomEditor.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }
}
