//! Port of `packages/coding-agent/src/modes/interactive/components/custom-editor.ts`.

use crate::core::keybindings::AppKeybinding;
use crate::core::keybindings::KeybindingsManager;
use crate::modes::interactive::components::status_indicator::StatusIndicator;
use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_tui::StyleFn;
use pi_tui::autocomplete::AutocompleteProvider;
use pi_tui::components::editor::{Editor, EditorComponent, EditorOptions, EditorTheme, TextCallback};
use pi_tui::tui::{Component, MouseEvent, MouseResult, Tui};
use pi_tui::utils::visible_width;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::sync::{Arc, Weak};

// TS: EditorOptions & { /** Render working, compaction, summarization, and retry status in the editor's top border. */ embedWorkingStatus?: boolean; }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

type Handler = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Handlers {
    action_handlers: IndexMap<AppKeybinding, Handler>,
    on_escape: Option<Handler>,
    on_ctrl_d: Option<Handler>,
    on_paste_image: Option<Handler>,
    on_extension_shortcut: Option<Arc<dyn Fn(String) -> bool + Send + Sync>>,
}

/// Custom editor that handles app-level keybindings for coding-agent.
/// TS: `extends Editor`; derefs to the pi-tui [`Editor`].
pub struct CustomEditor {
    editor: Arc<Editor>,
    keybindings: Arc<KeybindingsManager>,
    embed_working_status: bool,
    working_status_indicator: Arc<Mutex<Option<Arc<StatusIndicator>>>>,
    handlers: Mutex<Handlers>,
}

/// `renderTopBorder` override: the working status embedded in the top border.
fn render_top_border(
    editor: &Editor,
    indicator: &StatusIndicator,
    width: usize,
    hidden_line_count: usize,
) -> Option<String> {
    let border_color = editor.border_color();
    let bc = |s: &str| border_color(s);
    let w = width as i64;
    let mut status = indicator.render_in_border((w - 5).max(1));
    let mut status_width = visible_width(&status) as i64;
    if status_width == 0 {
        return None;
    }

    let overflow_label = (hidden_line_count > 0).then(|| format!(" ↑ {hidden_line_count} more "));
    let overflow_label_width = overflow_label.as_deref().map_or(0, visible_width) as i64;
    let overflow_start = (w - overflow_label_width).div_euclid(2);
    let can_fit_overflow = |status_width: i64| {
        overflow_label.is_some() && overflow_label_width + 2 <= w && overflow_start - (3 + status_width + 1) >= 1
    };

    if overflow_label.is_some() && !can_fit_overflow(status_width) {
        status = indicator.render_spinner_in_border(w);
        status_width = visible_width(&status) as i64;
    }

    if can_fit_overflow(status_width) {
        let left_block_width = 3 + status_width + 1;
        let label = overflow_label.as_deref().unwrap_or_default();
        return Some(format!(
            "{}{status}{}",
            bc("── "),
            bc(&format!(
                " {}{label}{}",
                "─".repeat((overflow_start - left_block_width).max(0) as usize),
                "─".repeat((w - overflow_start - overflow_label_width).max(0) as usize)
            ))
        ));
    }

    if w >= status_width + 5 {
        return Some(format!("{}{status}{}", bc("── "), bc(&format!(" {}", "─".repeat((w - status_width - 4) as usize)))));
    }

    status = indicator.render_spinner_in_border(w);
    status_width = visible_width(&status) as i64;
    let prefix_width = 3.min((w - status_width).max(0));
    Some(format!(
        "{}{status}{}",
        bc(&"─".repeat(prefix_width as usize)),
        bc(&"─".repeat((w - prefix_width - status_width).max(0) as usize))
    ))
}

impl CustomEditor {
    pub fn new(
        tui: Arc<Tui>,
        theme: Arc<EditorTheme>,
        keybindings: Arc<KeybindingsManager>,
        options: Option<&CustomEditorOptions>,
    ) -> Self {
        let editor_options = EditorOptions {
            padding_x: options.and_then(|o| o.padding_x).map(|p| p.max(0) as usize),
            autocomplete_max_visible: options.and_then(|o| o.autocomplete_max_visible).map(|p| p.max(0) as usize),
        };
        let editor = Editor::new(&tui, (*theme).clone(), editor_options);
        let embed_working_status = options.and_then(|o| o.embed_working_status).unwrap_or(false);
        let working_status_indicator: Arc<Mutex<Option<Arc<StatusIndicator>>>> = Arc::new(Mutex::new(None));
        if embed_working_status {
            let weak_editor: Weak<Editor> = Arc::downgrade(&editor);
            let indicator_slot = working_status_indicator.clone();
            editor.set_top_border_renderer(Some(Arc::new(move |width, hidden| {
                let indicator = indicator_slot.lock().clone()?;
                if width == 0 {
                    return None;
                }
                let editor = weak_editor.upgrade()?;
                render_top_border(&editor, &indicator, width, hidden)
            })));
        }
        Self { editor, keybindings, embed_working_status, working_status_indicator, handlers: Mutex::new(Handlers::default()) }
    }

    /// The underlying pi-tui editor.
    pub fn editor(&self) -> &Arc<Editor> {
        &self.editor
    }

    pub fn embed_working_status(&self) -> bool {
        self.embed_working_status
    }

    pub fn action_handlers(&self) -> IndexMap<AppKeybinding, Arc<dyn Fn() + Send + Sync>> {
        self.handlers.lock().action_handlers.clone()
    }

    pub fn set_action_handlers(&self, value: IndexMap<AppKeybinding, Arc<dyn Fn() + Send + Sync>>) {
        self.handlers.lock().action_handlers = value;
    }

    pub fn on_escape(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.handlers.lock().on_escape.clone()
    }

    pub fn set_on_escape(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.handlers.lock().on_escape = value;
    }

    pub fn on_ctrl_d(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.handlers.lock().on_ctrl_d.clone()
    }

    pub fn set_on_ctrl_d(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.handlers.lock().on_ctrl_d = value;
    }

    pub fn on_paste_image(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.handlers.lock().on_paste_image.clone()
    }

    pub fn set_on_paste_image(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.handlers.lock().on_paste_image = value;
    }

    /// Handler for extension-registered shortcuts. Returns true if handled.
    pub fn on_extension_shortcut(&self) -> Option<Arc<dyn Fn(String) -> bool + Send + Sync>> {
        self.handlers.lock().on_extension_shortcut.clone()
    }

    pub fn set_on_extension_shortcut(
        &self,
        value: Option<Arc<dyn Fn(String) -> bool + Send + Sync>>,
    ) {
        self.handlers.lock().on_extension_shortcut = value;
    }

    pub fn set_working_status_indicator(&self, indicator: Option<Arc<StatusIndicator>>) {
        *self.working_status_indicator.lock() = indicator;
    }

    /// Register a handler for an app action.
    pub fn on_action(&self, action: &AppKeybinding, handler: Arc<dyn Fn() + Send + Sync>) {
        self.handlers.lock().action_handlers.insert(action.clone(), handler);
    }
}

/// The `CustomEditor` behind an editor component: a Rust `CustomEditor`, or the Rust editor
/// backing a JavaScript `CustomEditor` subclass from an extension.
pub fn as_custom_editor(component: &pi_tui::tui::ComponentRef) -> Option<Arc<CustomEditor>> {
    if let Some(custom) = pi_tui::tui::downcast_component::<CustomEditor>(component) {
        return Some(custom);
    }
    pi_tui::tui::downcast_component::<crate::core::extensions::js::JsEditorComponent>(component)?.rust_custom_editor()
}

impl Deref for CustomEditor {
    type Target = Editor;
    fn deref(&self) -> &Editor {
        &self.editor
    }
}

impl Component for CustomEditor {
    fn render(&self, width: usize) -> Vec<String> {
        self.editor.render(width)
    }

    fn handle_input(&self, data: &str) {
        // Check extension-registered shortcuts first
        let on_extension_shortcut = self.handlers.lock().on_extension_shortcut.clone();
        if on_extension_shortcut.is_some_and(|f| f(data.to_string())) {
            return;
        }

        // Check for clipboard paste keybinding
        if self.keybindings.matches(data, "app.clipboard.pasteImage") {
            let handler = self.handlers.lock().on_paste_image.clone();
            if let Some(handler) = handler {
                handler();
            }
            return;
        }

        // Check app keybindings first

        // Escape/interrupt - only if autocomplete is NOT active
        if self.keybindings.matches(data, "app.interrupt") {
            if !self.editor.is_showing_autocomplete() {
                // Use dynamic onEscape if set, otherwise registered handler
                let handler = {
                    let h = self.handlers.lock();
                    h.on_escape.clone().or_else(|| h.action_handlers.get("app.interrupt").cloned())
                };
                if let Some(handler) = handler {
                    handler();
                    return;
                }
            }
            // Let parent handle escape for autocomplete cancellation
            self.editor.handle_input(data);
            return;
        }

        // Exit (Ctrl+D) - only when editor is empty
        if self.keybindings.matches(data, "app.exit") && self.editor.get_text().is_empty() {
            let handler = {
                let h = self.handlers.lock();
                h.on_ctrl_d.clone().or_else(|| h.action_handlers.get("app.exit").cloned())
            };
            if let Some(handler) = handler {
                handler();
            }
            return;
        }
        // Fall through to editor handling for delete-char-forward when not empty

        // Explicit history bindings take precedence over app actions while the editor is focused.
        // This lets users bind Ctrl+P even though it cycles models by default.
        if self.keybindings.matches(data, "tui.editor.historyPrevious")
            || self.keybindings.matches(data, "tui.editor.historyNext")
        {
            self.editor.handle_input(data);
            return;
        }

        // Check all other app actions
        let handlers: Vec<(AppKeybinding, Handler)> =
            self.handlers.lock().action_handlers.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (action, handler) in handlers {
            if action != "app.interrupt" && action != "app.exit" && self.keybindings.matches(data, &action) {
                handler();
                return;
            }
        }

        // Pass to parent for editor handling
        self.editor.handle_input(data);
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        self.editor.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.editor.handle_mouse(event)
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        self.editor.set_focused(focused);
    }

    fn focused(&self) -> bool {
        Component::focused(&*self.editor)
    }
}

impl EditorComponent for CustomEditor {
    fn get_text(&self) -> String {
        self.editor.get_text()
    }
    fn set_text(&self, text: &str) {
        self.editor.set_text(text)
    }
    fn set_on_submit(&self, f: Option<TextCallback>) {
        self.editor.set_on_submit(f)
    }
    fn set_on_change(&self, f: Option<TextCallback>) {
        self.editor.set_on_change(f)
    }
    fn add_to_history(&self, text: &str) {
        self.editor.add_to_history(text)
    }
    fn insert_text_at_cursor(&self, text: &str) {
        self.editor.insert_text_at_cursor(text)
    }
    fn get_expanded_text(&self) -> String {
        self.editor.get_expanded_text()
    }
    fn set_autocomplete_provider(&self, provider: Arc<dyn AutocompleteProvider>) {
        self.editor.set_autocomplete_provider(provider)
    }
    fn set_border_color(&self, f: StyleFn) {
        self.editor.set_border_color(f)
    }
    fn set_padding_x(&self, padding: usize) {
        self.editor.set_padding_x(padding)
    }
    fn set_autocomplete_max_visible(&self, max_visible: usize) {
        self.editor.set_autocomplete_max_visible(max_visible)
    }
}
