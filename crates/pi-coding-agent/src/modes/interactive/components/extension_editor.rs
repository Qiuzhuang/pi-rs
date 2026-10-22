//! Port of `packages/coding-agent/src/modes/interactive/components/extension-editor.ts`.
//!
//! Multi-line editor component for extensions.
//! Supports Ctrl+G for external editor.

use crate::core::keybindings::KeybindingsManager;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::key_hint;
use crate::modes::interactive::external_editor::{
    ExternalEditorOptions, ExternalEditorResult, edit_in_external_editor,
};
use crate::modes::interactive::theme::theme::{ThemeColor, get_editor_theme, theme};
use pi_tui::components::editor::{Editor, EditorOptions};
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult, Tui, TuiStopOptions};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// TS: extends EditorOptions (inherited fields inlined)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionEditorOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_x: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autocomplete_max_visible: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Run a future to completion in the background: on the current tokio runtime, or on a thread
/// with its own runtime when called outside one.
pub(crate) fn spawn_background<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            crate::utils::uncaught::spawn_on(&handle, future);
        }
        Err(_) => {
            crate::utils::uncaught::spawn_thread(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    rt.block_on(future);
                }
            });
        }
    }
}

/// TS: `extends Container implements Focusable`
pub struct ExtensionEditorComponent {
    container: Container,
    editor: Arc<Editor>,
    on_cancel_callback: Arc<dyn Fn() + Send + Sync>,
    tui: Arc<Tui>,
    keybindings: Arc<KeybindingsManager>,
    external_editor_command: String,
    focused: AtomicBool,
}

impl ExtensionEditorComponent {
    #[allow(clippy::too_many_arguments)]
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
        let non_empty = |v: Option<String>| v.filter(|s| !s.is_empty());
        let external_editor_command = non_empty(external_editor_command.map(str::to_string))
            .or_else(|| non_empty(std::env::var("VISUAL").ok()))
            .or_else(|| non_empty(std::env::var("EDITOR").ok()))
            .unwrap_or_else(|| if cfg!(windows) { "notepad".to_string() } else { "nano".to_string() });
        let description = options.and_then(|o| o.description.clone());
        let editor_options = EditorOptions {
            padding_x: options.and_then(|o| o.padding_x).map(|p| p.max(0) as usize),
            autocomplete_max_visible: options.and_then(|o| o.autocomplete_max_visible).map(|p| p.max(0) as usize),
        };
        let t = theme();
        let container = Container::new();

        // Add top border
        container.add_child(Arc::new(DynamicBorder::new(None)));
        container.add_child(Arc::new(Spacer::new(1)));

        // Add title and optional description
        container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Accent, title), 1, 0, None)));
        if let Some(description) = description.filter(|d| !d.is_empty()) {
            container.add_child(Arc::new(Spacer::new(1)));
            container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Text, &description), 1, 0, None)));
        }
        container.add_child(Arc::new(Spacer::new(1)));

        // Create editor
        let editor = Editor::new(&tui, (*get_editor_theme()).clone(), editor_options);
        if let Some(prefill) = prefill.filter(|p| !p.is_empty()) {
            editor.set_text(prefill);
        }
        // Wire up Enter to submit (Shift+Enter for newlines, like the main editor)
        editor.set_on_submit(Some(Arc::new(move |text: String| on_submit(text))));
        container.add_child(editor.clone() as ComponentRef);

        container.add_child(Arc::new(Spacer::new(1)));

        // Add hint
        let hint = key_hint("tui.select.confirm".to_string(), "submit")
            + "  "
            + &key_hint("tui.input.newLine".to_string(), "newline")
            + "  "
            + &key_hint("tui.select.cancel".to_string(), "cancel")
            + &format!("  {}", key_hint("app.editor.external".to_string(), "external editor"));
        container.add_child(Arc::new(Text::new(hint, 1, 0, None)));

        container.add_child(Arc::new(Spacer::new(1)));

        // Add bottom border
        container.add_child(Arc::new(DynamicBorder::new(None)));

        Self {
            container,
            editor,
            on_cancel_callback: on_cancel,
            tui,
            keybindings,
            external_editor_command,
            focused: AtomicBool::new(false),
        }
    }

    fn handle_open_external_editor(&self) {
        let content = self.editor.get_text();
        self.tui.stop(TuiStopOptions::default());
        let tui = self.tui.clone();
        let editor = self.editor.clone();
        let command = self.external_editor_command.clone();
        spawn_background(async move {
            let result = edit_in_external_editor(&ExternalEditorOptions { command, content }).await;
            if let Ok(ExternalEditorResult::Complete { content }) = result {
                editor.set_text(&content);
            }
            tui.start();
            tui.request_render(true);
        });
    }
}

impl Component for ExtensionEditorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn handle_input(&self, key_data: &str) {
        let kb = get_keybindings();
        // Escape or Ctrl+C to cancel
        if kb.matches(key_data, "tui.select.cancel") {
            (self.on_cancel_callback)();
            return;
        }

        // External editor (app keybinding)
        if self.keybindings.matches(key_data, "app.editor.external") {
            self.handle_open_external_editor();
            return;
        }

        // Forward to editor
        self.editor.handle_input(key_data);
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        self.container.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.container.child_components()
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::SeqCst);
        self.editor.set_focused(focused);
    }

    fn focused(&self) -> bool {
        self.focused.load(Ordering::SeqCst)
    }
}
