//! Port of `packages/coding-agent/src/modes/interactive/components/user-message-selector.ts`.

use pi_tui::tui::Component;
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

/// Custom user message list component with selection
/// TS: `implements Component`
pub struct UserMessageList {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl UserMessageList {
    pub fn new(messages: &[UserMessageItem], initial_selected_id: Option<&str>) -> Self {
        todo!(
            "port modes/interactive/components/user-message-selector.ts:UserMessageList.constructor"
        )
    }

    pub fn on_select(&self) -> Option<Arc<dyn Fn(String) + Send + Sync>> {
        todo!("port modes/interactive/components/user-message-selector.ts:UserMessageList.onSelect")
    }

    pub fn set_on_select(&self, value: Option<Arc<dyn Fn(String) + Send + Sync>>) {
        todo!("port modes/interactive/components/user-message-selector.ts:UserMessageList.onSelect")
    }

    pub fn on_cancel(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/user-message-selector.ts:UserMessageList.onCancel")
    }

    pub fn set_on_cancel(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/user-message-selector.ts:UserMessageList.onCancel")
    }
}

impl Component for UserMessageList {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/user-message-selector.ts:UserMessageList.render")
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/user-message-selector.ts:UserMessageList.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/user-message-selector.ts:UserMessageList.invalidate"
        )
    }
}

/// Component that renders a user message selector for branching
/// TS: `extends Container`
pub struct UserMessageSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl UserMessageSelectorComponent {
    pub fn new(
        messages: &[UserMessageItem],
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        initial_selected_id: Option<&str>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/user-message-selector.ts:UserMessageSelectorComponent.constructor"
        )
    }

    pub fn get_message_list(&self) -> Arc<UserMessageList> {
        todo!(
            "port modes/interactive/components/user-message-selector.ts:UserMessageSelectorComponent.getMessageList"
        )
    }
}

// TODO(port): impl pi-tui::Container for UserMessageSelectorComponent

impl Component for UserMessageSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/user-message-selector.ts:UserMessageSelectorComponent.render"
        )
    }
}
