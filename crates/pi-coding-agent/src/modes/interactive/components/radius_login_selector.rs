//! Port of `packages/coding-agent/src/modes/interactive/components/radius-login-selector.ts`.

use crate::modes::interactive::components::extension_selector::ExtensionSelectorComponent;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The "Sign in with Radius" option: `label` is the full option, starting with the animated `text`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadiusOption {
    pub label: String,
    pub text: String,
}

/// Top-level `/login` selector whose Radius option shimmers in the Radius logo colors while it is selected.
pub fn create_login_menu_selector(
    tui: Arc<Tui>,
    title: &str,
    options: &[String],
    radius_option: &RadiusOption,
    on_select: Arc<dyn Fn(String) + Send + Sync>,
    on_cancel: Arc<dyn Fn() + Send + Sync>,
) -> Arc<ExtensionSelectorComponent> {
    todo!("port modes/interactive/components/radius-login-selector.ts:createLoginMenuSelector")
}
