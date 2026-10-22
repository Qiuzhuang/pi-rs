//! Port of `packages/coding-agent/src/modes/interactive/components/user-message-selector.ts`.

use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use parking_lot::Mutex;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use pi_tui::utils::{js_trim, truncate_to_width};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessageItem {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

type SelectCallback = Arc<dyn Fn(String) + Send + Sync>;
type CancelCallback = Arc<dyn Fn() + Send + Sync>;

/// Custom user message list component with selection
/// TS: `implements Component`
pub struct UserMessageList {
    messages: Vec<UserMessageItem>,
    selected_index: Mutex<usize>,
    on_select: Mutex<Option<SelectCallback>>,
    on_cancel: Mutex<Option<CancelCallback>>,
    /// Max messages visible
    max_visible: usize,
}

impl UserMessageList {
    pub fn new(messages: &[UserMessageItem], initial_selected_id: Option<&str>) -> Self {
        // Store messages in chronological order (oldest to newest)
        let initial_index = initial_selected_id
            .filter(|id| !id.is_empty())
            .and_then(|id| messages.iter().position(|m| m.id == id));
        // Start with selected message if provided, else default to the most recent
        let selected_index = initial_index.unwrap_or(messages.len().saturating_sub(1));
        Self {
            messages: messages.to_vec(),
            selected_index: Mutex::new(selected_index),
            on_select: Mutex::new(None),
            on_cancel: Mutex::new(None),
            max_visible: 10,
        }
    }

    pub fn on_select(&self) -> Option<Arc<dyn Fn(String) + Send + Sync>> {
        self.on_select.lock().clone()
    }

    pub fn set_on_select(&self, value: Option<Arc<dyn Fn(String) + Send + Sync>>) {
        *self.on_select.lock() = value;
    }

    pub fn on_cancel(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.on_cancel.lock().clone()
    }

    pub fn set_on_cancel(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.on_cancel.lock() = value;
    }
}

impl Component for UserMessageList {
    fn render(&self, width: usize) -> Vec<String> {
        let t = theme();
        let mut lines = Vec::new();
        if self.messages.is_empty() {
            lines.push(t.fg(&ThemeColor::Muted, "  No user messages found"));
            return lines;
        }

        let selected_index = *self.selected_index.lock();
        let len = self.messages.len() as i64;
        let max_visible = self.max_visible as i64;
        // Calculate visible range with scrolling
        let start_index = (selected_index as i64 - max_visible / 2).min(len - max_visible).max(0);
        let end_index = (start_index + max_visible).min(len);

        // Render visible messages (2 lines per message + blank line)
        for i in start_index..end_index {
            let message = &self.messages[i as usize];
            let is_selected = i as usize == selected_index;

            // Normalize message to single line
            let normalized_message = js_trim(&message.text.replace('\n', " ")).to_string();

            // First line: cursor + message
            let cursor = if is_selected { t.fg(&ThemeColor::Accent, "› ") } else { "  ".to_string() };
            let max_msg_width = width.saturating_sub(2); // Account for cursor (2 chars)
            let truncated_msg = truncate_to_width(&normalized_message, max_msg_width, "...", false);
            let message_line = format!("{cursor}{}", if is_selected { t.bold(&truncated_msg) } else { truncated_msg });
            lines.push(message_line);

            // Second line: metadata (position in history)
            let metadata = format!("  Message {} of {}", i + 1, len);
            lines.push(t.fg(&ThemeColor::Muted, &metadata));
            lines.push(String::new()); // Blank line between messages
        }

        // Add scroll indicator if needed
        if start_index > 0 || end_index < len {
            lines.push(t.fg(&ThemeColor::Muted, &format!("  ({}/{})", selected_index + 1, len)));
        }

        lines
    }

    fn handle_input(&self, data: &str) {
        let kb = get_keybindings();
        let len = self.messages.len();
        if kb.matches(data, "tui.select.up") {
            // Up arrow - go to previous (older) message, wrap to bottom when at top
            let mut idx = self.selected_index.lock();
            *idx = if *idx == 0 { len.saturating_sub(1) } else { *idx - 1 };
        } else if kb.matches(data, "tui.select.down") {
            // Down arrow - go to next (newer) message, wrap to top when at bottom
            let mut idx = self.selected_index.lock();
            *idx = if *idx + 1 >= len { 0 } else { *idx + 1 };
        } else if kb.matches(data, "tui.select.confirm") {
            // Enter - select message and branch
            let idx = *self.selected_index.lock();
            let callback = self.on_select.lock().clone();
            if let (Some(selected), Some(callback)) = (self.messages.get(idx), callback) {
                callback(selected.id.clone());
            }
        } else if kb.matches(data, "tui.select.cancel") {
            // Escape - cancel
            let callback = self.on_cancel.lock().clone();
            if let Some(callback) = callback {
                callback();
            }
        }
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        // No cached state to invalidate currently
    }
}

/// Component that renders a user message selector for branching
/// TS: `extends Container`
pub struct UserMessageSelectorComponent {
    container: Container,
    message_list: Arc<UserMessageList>,
}

impl UserMessageSelectorComponent {
    pub fn new(
        messages: &[UserMessageItem],
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        initial_selected_id: Option<&str>,
    ) -> Self {
        let t = theme();
        let container = Container::new();
        // Add header
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(Text::new(t.bold("Fork from Message"), 1, 0, None)));
        container.add_child(Arc::new(Text::new(
            t.fg(
                &ThemeColor::Muted,
                "Select a user message to copy the active path up to that point into a new session",
            ),
            1,
            0,
            None,
        )));
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(DynamicBorder::new(None)));
        container.add_child(Arc::new(Spacer::new(1)));

        // Create message list
        let message_list = Arc::new(UserMessageList::new(messages, initial_selected_id));
        message_list.set_on_select(Some(on_select));
        message_list.set_on_cancel(Some(on_cancel.clone()));
        container.add_child(message_list.clone());

        // Add bottom border
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(DynamicBorder::new(None)));

        // Auto-cancel if no messages
        if messages.is_empty() {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                on_cancel();
            });
        }

        Self { container, message_list }
    }

    pub fn get_message_list(&self) -> Arc<UserMessageList> {
        self.message_list.clone()
    }
}

impl Component for UserMessageSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
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
}
