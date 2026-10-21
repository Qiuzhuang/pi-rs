//! Port of `packages/coding-agent/src/modes/interactive/components/session-selector.ts`.

use crate::core::keybindings::KeybindingsManager;
use crate::core::session_manager::SessionInfo;
use crate::core::session_manager::SessionListProgress;
use futures::future::BoxFuture;
use pi_tui::tui::Component;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionScope {
    #[serde(rename = "current")]
    Current,
    #[serde(rename = "all")]
    All,
}

impl SessionScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::All => "all",
        }
    }
}

/// A session tree node for hierarchical display
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTreeNode {
    pub session: SessionInfo,
    pub children: Vec<SessionTreeNode>,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub latest_activity: f64,
}

/// Flattened node for display with tree structure info
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlatSessionNode {
    pub session: SessionInfo,
    pub depth: i64,
    pub is_last: bool,
    /// For each ancestor level, whether there are more siblings after it
    pub ancestor_continues: Vec<bool>,
}

pub type SessionsLoader = Arc<
    dyn Fn(
            Option<SessionListProgress>,
            Option<CancellationToken>,
        ) -> BoxFuture<'static, anyhow::Result<Vec<SessionInfo>>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct SessionSelectorComponentOptions {
    pub rename_session: Option<
        Arc<dyn Fn(String, Option<String>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
    pub show_rename_hint: Option<bool>,
    pub keybindings: Option<Arc<KeybindingsManager>>,
}

/// Component that renders a session selector
/// TS: `extends Container implements Focusable`
pub struct SessionSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl SessionSelectorComponent {
    pub fn new(
        current_sessions_loader: &SessionsLoader,
        all_sessions_loader: &SessionsLoader,
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        on_exit: Arc<dyn Fn() + Send + Sync>,
        request_render: Arc<dyn Fn() + Send + Sync>,
        options: Option<&SessionSelectorComponentOptions>,
        current_session_file_path: Option<&str>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorComponent.constructor"
        )
    }

    pub fn get_session_list(&self) -> Value /* TS: SessionList */ {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorComponent.getSessionList"
        )
    }
}

// TODO(port): impl pi-tui::Container for SessionSelectorComponent

impl Component for SessionSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorComponent.render"
        )
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}
