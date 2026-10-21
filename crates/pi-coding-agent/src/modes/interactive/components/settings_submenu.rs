//! Port of `packages/coding-agent/src/modes/interactive/components/settings-submenu.ts`.

use indexmap::IndexMap;
use pi_tui::components::select_list::SelectItem;
use pi_tui::components::select_list::SelectListLayoutOptions;
use pi_tui::tui::Component;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct SelectSubmenuOptions {
    /// Enable type-to-search fuzzy filtering.
    pub searchable: Option<bool>,
    /// Override the select list layout (column widths).
    pub layout: Option<Arc<SelectListLayoutOptions>>,
}

/// Single-step submenu that shows a titled select list.
/// With `searchable: true`, typing filters the list using fuzzy matching.
/// TS: `extends Container`
pub struct SelectSubmenu {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl SelectSubmenu {
    pub fn new(
        title: &str,
        description: &str,
        options: Vec<Arc<SelectItem>>,
        current_value: &str,
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        on_selection_change: Option<Arc<dyn Fn(String) + Send + Sync>>,
        submenu_options: Option<&SelectSubmenuOptions>,
    ) -> Self {
        todo!("port modes/interactive/components/settings-submenu.ts:SelectSubmenu.constructor")
    }
}

// TODO(port): impl pi-tui::Container for SelectSubmenu

impl Component for SelectSubmenu {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/settings-submenu.ts:SelectSubmenu.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port modes/interactive/components/settings-submenu.ts:SelectSubmenu.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }
}

#[derive(Clone)]
pub enum SteppedSubmenuStepTitle {
    String(String),
    Fn(Arc<dyn Fn(IndexMap<String, String>) -> String + Send + Sync>),
}

#[derive(Clone)]
pub enum SteppedSubmenuStepDescription {
    String(String),
    Fn(Arc<dyn Fn(IndexMap<String, String>) -> String + Send + Sync>),
}

/// One step in a {@link SteppedSubmenu}.
#[derive(Clone)]
pub struct SteppedSubmenuStep {
    /// Unique key \u2014 the selected value is stored in the result context under this key.
    pub key: String,
    /// Title shown at the top of the step. Receives prior selections.
    pub title: SteppedSubmenuStepTitle,
    /// Description shown below the title. Receives prior selections.
    pub description: SteppedSubmenuStepDescription,
    /// Build the option list for this step. Called fresh each time the step is shown.
    pub options: Arc<dyn Fn(IndexMap<String, String>) -> Vec<Arc<SelectItem>> + Send + Sync>,
    /// Optionally pre-select a value when entering this step.
    pub preselect: Option<Arc<dyn Fn(IndexMap<String, String>) -> Option<String> + Send + Sync>>,
    /// Enable type-to-search fuzzy filtering for this step.
    pub searchable: Option<bool>,
    /// Override the select list layout (column widths) for this step.
    pub layout: Option<Arc<SelectListLayoutOptions>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteppedSubmenuOptions {
    /// Start at this step index (0-based), skipping earlier steps. Requires initialContext for skipped keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_at_step: Option<i64>,
    /// Pre-fill selections for skipped steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_context: Option<IndexMap<String, String>>,
    /// After completing the last step, loop back to step 0 instead of closing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#loop: Option<bool>,
}

/// Generic N-step submenu built on top of {@link SelectSubmenu}.
///
/// Each step's options can depend on prior selections via the shared context.
/// Esc goes back one step; Esc at step 0 cancels.
/// With `loop: true`, completing the final step invokes `onComplete` then returns to step 0.
/// TS: `extends Container`
pub struct SteppedSubmenu {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl SteppedSubmenu {
    pub fn new(
        steps: &[SteppedSubmenuStep],
        on_complete: Arc<dyn Fn(IndexMap<String, String>) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        opts: Option<&SteppedSubmenuOptions>,
    ) -> Self {
        todo!("port modes/interactive/components/settings-submenu.ts:SteppedSubmenu.constructor")
    }
}

// TODO(port): impl pi-tui::Container for SteppedSubmenu

impl Component for SteppedSubmenu {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/settings-submenu.ts:SteppedSubmenu.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port modes/interactive/components/settings-submenu.ts:SteppedSubmenu.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/settings-submenu.ts:SteppedSubmenu.invalidate")
    }
}
