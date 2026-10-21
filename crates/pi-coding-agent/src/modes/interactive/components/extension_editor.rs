//! Port of `packages/coding-agent/src/modes/interactive/components/extension-editor.ts`.

use crate::core::keybindings::KeybindingsManager;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// TS: extends EditorOptions (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionEditorOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_x: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autocomplete_max_visible: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// TS: `extends Container implements Focusable`
pub struct ExtensionEditorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ExtensionEditorComponent {
    pub fn new(
        tui: Arc<Tui>,
        keybindings: Arc<KeybindingsManager>,
        title: &str,
        prefill: Option<&str>,
        on_submit: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        options: Option<&ExtensionEditorOptions>,
        external_editor_command: Option<&str>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/extension-editor.ts:ExtensionEditorComponent.constructor"
        )
    }
}

// TODO(port): impl pi-tui::Container for ExtensionEditorComponent

impl Component for ExtensionEditorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/extension-editor.ts:ExtensionEditorComponent.render"
        )
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/extension-editor.ts:ExtensionEditorComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}
