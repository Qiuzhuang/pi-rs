//! Port of `packages/coding-agent/src/modes/interactive/components/session-selector.ts`.

use crate::core::keybindings::KeybindingsManager;
use crate::core::session_manager::SessionInfo;
use crate::core::session_manager::SessionListProgress;
use crate::modes::interactive::components::session_selector_search::NameFilter;
use crate::modes::interactive::components::session_selector_search::SortMode;
use futures::future::BoxFuture;
use pi_tui::tui::Component;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionSelectorHeaderMsgType {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "error")]
    Error,
}

impl SessionSelectorHeaderMsgType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSelectorHeaderMsg {
    pub r#type: SessionSelectorHeaderMsgType,
    pub message: String,
}

/// TS: `implements Component`
pub struct SessionSelectorHeader {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl SessionSelectorHeader {
    pub fn new(
        scope: &SessionScope,
        sort_mode: &SortMode,
        name_filter: &NameFilter,
        request_render: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.constructor"
        )
    }

    pub fn set_scope(&self, scope: &SessionScope) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.setScope"
        )
    }

    pub fn set_sort_mode(&self, sort_mode: &SortMode) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.setSortMode"
        )
    }

    pub fn set_name_filter(&self, name_filter: &NameFilter) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.setNameFilter"
        )
    }

    pub fn set_loading(&self, loading: bool) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.setLoading"
        )
    }

    pub fn set_progress(&self, loaded: f64, total: i64) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.setProgress"
        )
    }

    pub fn set_show_path(&self, show_path: bool) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.setShowPath"
        )
    }

    pub fn set_show_rename_hint(&self, show: bool) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.setShowRenameHint"
        )
    }

    pub fn set_confirming_delete_path(&self, path: Option<&str>) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.setConfirmingDeletePath"
        )
    }

    pub fn set_status_message(
        &self,
        msg: Option<&SessionSelectorHeaderMsg>,
        auto_hide_ms: Option<f64>,
    ) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.setStatusMessage"
        )
    }
}

impl Component for SessionSelectorHeader {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/session-selector.ts:SessionSelectorHeader.render")
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionSelectorHeader.invalidate"
        )
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

/// Custom session list component with multi-line items and search
/// TS: `implements Component, Focusable`
pub struct SessionList {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl SessionList {
    pub fn new(
        sessions: &[SessionInfo],
        show_cwd: bool,
        sort_mode: &SortMode,
        name_filter: &NameFilter,
        keybindings: Arc<KeybindingsManager>,
        current_session_file_path: Option<&str>,
    ) -> Self {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.constructor")
    }

    pub fn get_selected_session_path(&self) -> Option<String> {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionList.getSelectedSessionPath"
        )
    }

    pub fn on_select(&self) -> Option<Arc<dyn Fn(String) + Send + Sync>> {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onSelect")
    }

    pub fn set_on_select(&self, value: Option<Arc<dyn Fn(String) + Send + Sync>>) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onSelect")
    }

    pub fn on_cancel(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onCancel")
    }

    pub fn set_on_cancel(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onCancel")
    }

    pub fn on_exit(&self) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onExit")
    }

    pub fn on_toggle_scope(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onToggleScope")
    }

    pub fn set_on_toggle_scope(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onToggleScope")
    }

    pub fn on_toggle_sort(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onToggleSort")
    }

    pub fn set_on_toggle_sort(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onToggleSort")
    }

    pub fn on_toggle_name_filter(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionList.onToggleNameFilter"
        )
    }

    pub fn set_on_toggle_name_filter(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionList.onToggleNameFilter"
        )
    }

    pub fn on_toggle_path(&self) -> Option<Arc<dyn Fn(bool) + Send + Sync>> {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onTogglePath")
    }

    pub fn set_on_toggle_path(&self, value: Option<Arc<dyn Fn(bool) + Send + Sync>>) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onTogglePath")
    }

    pub fn on_delete_confirmation_change(
        &self,
    ) -> Option<Arc<dyn Fn(Option<String>) + Send + Sync>> {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionList.onDeleteConfirmationChange"
        )
    }

    pub fn set_on_delete_confirmation_change(
        &self,
        value: Option<Arc<dyn Fn(Option<String>) + Send + Sync>>,
    ) {
        todo!(
            "port modes/interactive/components/session-selector.ts:SessionList.onDeleteConfirmationChange"
        )
    }

    pub fn on_delete_session(
        &self,
    ) -> Option<Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>> {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onDeleteSession")
    }

    pub fn set_on_delete_session(
        &self,
        value: Option<Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>>,
    ) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onDeleteSession")
    }

    pub fn on_rename_session(&self) -> Option<Arc<dyn Fn(String) + Send + Sync>> {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onRenameSession")
    }

    pub fn set_on_rename_session(&self, value: Option<Arc<dyn Fn(String) + Send + Sync>>) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onRenameSession")
    }

    pub fn on_error(&self) -> Option<Arc<dyn Fn(String) + Send + Sync>> {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onError")
    }

    pub fn set_on_error(&self, value: Option<Arc<dyn Fn(String) + Send + Sync>>) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.onError")
    }

    pub fn set_sort_mode(&self, sort_mode: &SortMode) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.setSortMode")
    }

    pub fn set_name_filter(&self, name_filter: &NameFilter) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.setNameFilter")
    }

    pub fn set_sessions(&self, sessions: &[SessionInfo], show_cwd: bool) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.setSessions")
    }
}

impl Component for SessionList {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/session-selector.ts:SessionList.invalidate")
    }
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

    pub fn get_session_list(&self) -> Arc<SessionList> {
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
