//! Port of `packages/coding-agent/src/modes/interactive/components/model-selector.ts`.

use crate::core::model_runtime::ModelRuntime;
use pi_ai::types::Model;
use pi_tui::components::input::Input;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelItem {
    pub provider: String,
    pub id: String,
    pub model: Model,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopedModelItem {
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultModelReference {
    pub provider: String,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelScope {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "scoped")]
    Scoped,
}

impl ModelScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Scoped => "scoped",
        }
    }
}

/// Component that renders a model selector with search
/// TS: `extends Container implements Focusable`
pub struct ModelSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ModelSelectorComponent {
    pub fn new(
        tui: Arc<Tui>,
        current_model: Option<Model>,
        model_runtime: Arc<ModelRuntime>,
        scoped_models: &[ScopedModelItem],
        on_select: Arc<dyn Fn(Model) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        initial_search_input: Option<&str>,
        on_select_as_default: Option<Arc<dyn Fn(Model) + Send + Sync>>,
        default_model: Option<&DefaultModelReference>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/model-selector.ts:ModelSelectorComponent.constructor"
        )
    }

    pub fn dispose(&self) {
        todo!("port modes/interactive/components/model-selector.ts:ModelSelectorComponent.dispose")
    }

    pub fn get_search_input(&self) -> Arc<Input> {
        todo!(
            "port modes/interactive/components/model-selector.ts:ModelSelectorComponent.getSearchInput"
        )
    }
}

// TODO(port): impl pi-tui::Container for ModelSelectorComponent

impl Component for ModelSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/model-selector.ts:ModelSelectorComponent.render")
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/model-selector.ts:ModelSelectorComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}
