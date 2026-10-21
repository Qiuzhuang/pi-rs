//! Port of `packages/coding-agent/src/modes/interactive/components/scoped-models-selector.ts`.

use futures::future::BoxFuture;
use pi_ai::types::Model;
use pi_tui::components::input::Input;
use pi_tui::tui::Component;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type EnabledIds = Option<Vec<String>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelItem {
    pub full_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsConfig {
    pub all_models: Vec<Model>,
    #[serde(default)]
    pub enabled_model_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_status: Option<String>,
}

#[derive(Clone)]
pub struct ModelsCallbacks {
    /// Called whenever the enabled model set or order changes (session-only, no persist)
    pub on_change:
        Arc<dyn Fn(Option<Vec<String>>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    /// Called when user wants to persist current selection to settings
    pub on_persist:
        Arc<dyn Fn(Option<Vec<String>>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    pub on_cancel: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScopedModelsSelectorComponentSetRefreshStatusKind {
    #[serde(rename = "muted")]
    Muted,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "warning")]
    Warning,
}

impl ScopedModelsSelectorComponentSetRefreshStatusKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Muted => "muted",
            Self::Success => "success",
            Self::Warning => "warning",
        }
    }
}

/// Component for enabling/disabling models for Ctrl+P cycling.
/// Changes are session-only until explicitly persisted with Ctrl+S.
/// TS: `extends Container implements Focusable`
pub struct ScopedModelsSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ScopedModelsSelectorComponent {
    pub fn new(config: &ModelsConfig, callbacks: &ModelsCallbacks) -> Self {
        todo!(
            "port modes/interactive/components/scoped-models-selector.ts:ScopedModelsSelectorComponent.constructor"
        )
    }

    pub fn update_models(&self, models: &[Model], enabled_model_ids: Option<&[String]>) {
        todo!(
            "port modes/interactive/components/scoped-models-selector.ts:ScopedModelsSelectorComponent.updateModels"
        )
    }

    pub fn set_refresh_status(
        &self,
        message: &str,
        kind: ScopedModelsSelectorComponentSetRefreshStatusKind,
    ) {
        todo!(
            "port modes/interactive/components/scoped-models-selector.ts:ScopedModelsSelectorComponent.setRefreshStatus"
        )
    }

    pub fn get_search_input(&self) -> Arc<Input> {
        todo!(
            "port modes/interactive/components/scoped-models-selector.ts:ScopedModelsSelectorComponent.getSearchInput"
        )
    }
}

// TODO(port): impl pi-tui::Container for ScopedModelsSelectorComponent

impl Component for ScopedModelsSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/scoped-models-selector.ts:ScopedModelsSelectorComponent.render"
        )
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/scoped-models-selector.ts:ScopedModelsSelectorComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}
