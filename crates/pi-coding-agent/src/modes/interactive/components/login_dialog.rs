//! Port of `packages/coding-agent/src/modes/interactive/components/login-dialog.ts`.

use pi_ai::auth::extension_oauth_types::OAuthDeviceCodeInfo;
use pi_ai::auth::types::AuthInfoLink;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Login dialog component - replaces editor during OAuth login flow
/// TS: `extends Container implements Focusable`
pub struct LoginDialogComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl LoginDialogComponent {
    pub fn new(
        tui: Arc<Tui>,
        provider_id: &str,
        on_complete: Arc<dyn Fn(bool, Option<String>) + Send + Sync>,
        provider_name_override: Option<&str>,
        title_override: Option<&str>,
    ) -> Self {
        todo!("port modes/interactive/components/login-dialog.ts:LoginDialogComponent.constructor")
    }

    pub fn signal(&self) -> CancellationToken {
        todo!("port modes/interactive/components/login-dialog.ts:LoginDialogComponent.signal")
    }

    /// Called by onAuth callback - show URL and optional instructions
    pub fn show_auth(&self, url: &str, instructions: Option<&str>) {
        todo!("port modes/interactive/components/login-dialog.ts:LoginDialogComponent.showAuth")
    }

    /// Called by onDeviceCode callback - show URL and user code.
    pub fn show_device_code(&self, info: OAuthDeviceCodeInfo) {
        todo!(
            "port modes/interactive/components/login-dialog.ts:LoginDialogComponent.showDeviceCode"
        )
    }

    /// Show input for manual code/URL entry (for callback server providers)
    pub async fn show_manual_input(&self, prompt: &str) -> anyhow::Result<String> {
        todo!(
            "port modes/interactive/components/login-dialog.ts:LoginDialogComponent.showManualInput"
        )
    }

    /// Called by onPrompt callback - show prompt and wait for input
    /// Note: Does NOT clear content, appends to existing (preserves URL from showAuth)
    pub async fn show_prompt(
        &self,
        message: &str,
        placeholder: Option<&str>,
    ) -> anyhow::Result<String> {
        todo!("port modes/interactive/components/login-dialog.ts:LoginDialogComponent.showPrompt")
    }

    /// Show informational text before another login step.
    pub fn show_details(&self, lines: &[String]) {
        todo!("port modes/interactive/components/login-dialog.ts:LoginDialogComponent.showDetails")
    }

    /// Show provider-owned information and links without starting an auth callback flow.
    pub fn show_info(
        &self,
        message: &str,
        links: Option<&[AuthInfoLink]>,
        show_close_hint: Option<bool>,
    ) {
        todo!("port modes/interactive/components/login-dialog.ts:LoginDialogComponent.showInfo")
    }

    /// Show waiting message (for polling flows like GitHub Copilot)
    pub fn show_waiting(&self, message: &str) {
        todo!("port modes/interactive/components/login-dialog.ts:LoginDialogComponent.showWaiting")
    }

    /// Called by onProgress callback
    pub fn show_progress(&self, message: &str) {
        todo!("port modes/interactive/components/login-dialog.ts:LoginDialogComponent.showProgress")
    }
}

// TODO(port): impl pi-tui::Container for LoginDialogComponent

impl Component for LoginDialogComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/login-dialog.ts:LoginDialogComponent.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port modes/interactive/components/login-dialog.ts:LoginDialogComponent.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }
}
