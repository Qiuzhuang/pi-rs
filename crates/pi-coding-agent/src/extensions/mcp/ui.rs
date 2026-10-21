//! Port of `packages/coding-agent/src/extensions/mcp/ui.ts`.

use crate::core::extensions::types::ExtensionCommandContext;
use crate::core::keybindings::KeybindingsManager;
use crate::modes::interactive::theme::theme::Theme;
use futures::future::BoxFuture;
use pi_tui::components::select_list::SelectItem;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct McpMenu {
    pub title: String,
    /// Shown below the title.
    pub details: Option<String>,
    /// Shown below the details in the error color.
    pub error: Option<String>,
    pub items: Vec<Arc<SelectItem>>,
    /// Shown when there are no items.
    pub empty: Option<String>,
    /// Value of the item selected when the menu opens.
    pub selected: Option<String>,
    /// What the confirm key does, for the key hint.
    pub confirm_label: String,
    /// What the cancel key does, for the key hint.
    pub cancel_label: String,
}

#[async_trait::async_trait]
pub trait McpUi: Send + Sync {
    /// Show a menu and resolve to the chosen item's value, or undefined when cancelled. `subscribe`
    /// rebuilds the menu on every change, keeping the selected item.
    async fn menu(
        &self,
        build: Arc<dyn Fn() -> McpMenu + Send + Sync>,
        subscribe: Option<
            Arc<dyn Fn(Arc<dyn Fn() + Send + Sync>) -> Arc<dyn Fn() + Send + Sync> + Send + Sync>,
        >,
    ) -> anyhow::Result<Option<String>>;
    /// Show a message while an operation runs. With `onCancel`, the cancel key calls it.
    fn status(&self, title: &str, message: &str, on_cancel: Option<Arc<dyn Fn() + Send + Sync>>);
    /// Show the authorization URL and wait for a pasted redirect URL. Resolves to undefined when
    /// cancelled or when `signal` aborts (the browser reached the callback).
    async fn redirect_url(
        &self,
        title: &str,
        authorization_url: &str,
        signal: CancellationToken,
    ) -> anyhow::Result<Option<String>>;
}

/// TS: `implements McpUi, Component, Focusable`
pub struct McpManagerView {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl McpManagerView {
    pub fn new(tui: Arc<Tui>, theme: Arc<Theme>, keybindings: Arc<KeybindingsManager>) -> Self {
        todo!("port extensions/mcp/ui.ts:McpManagerView.constructor")
    }
}

#[async_trait::async_trait]
impl McpUi for McpManagerView {
    async fn menu(
        &self,
        build: Arc<dyn Fn() -> McpMenu + Send + Sync>,
        subscribe: Option<
            Arc<dyn Fn(Arc<dyn Fn() + Send + Sync>) -> Arc<dyn Fn() + Send + Sync> + Send + Sync>,
        >,
    ) -> anyhow::Result<Option<String>> {
        todo!("port extensions/mcp/ui.ts:McpManagerView.menu")
    }
    fn status(&self, title: &str, message: &str, on_cancel: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port extensions/mcp/ui.ts:McpManagerView.status")
    }
    async fn redirect_url(
        &self,
        title: &str,
        authorization_url: &str,
        signal: CancellationToken,
    ) -> anyhow::Result<Option<String>> {
        todo!("port extensions/mcp/ui.ts:McpManagerView.redirect_url")
    }
}

impl Component for McpManagerView {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port extensions/mcp/ui.ts:McpManagerView.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port extensions/mcp/ui.ts:McpManagerView.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        todo!("port extensions/mcp/ui.ts:McpManagerView.invalidate")
    }
}

/// Run `manage` in the manager view until it returns.
pub async fn show_mcp_manager(
    ctx: &ExtensionCommandContext,
    manage: Arc<dyn Fn(Arc<dyn McpUi>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
) -> anyhow::Result<()> {
    todo!("port extensions/mcp/ui.ts:showMcpManager")
}
