//! Port of `packages/coding-agent/src/modes/interactive/components/settings-submenu.ts`.

use crate::modes::interactive::theme::theme::{ThemeColor, get_select_list_theme, theme};
use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_tui::components::input::Input;
use pi_tui::components::select_list::SelectItem;
use pi_tui::components::select_list::SelectList;
use pi_tui::components::select_list::SelectListLayoutOptions;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::fuzzy::fuzzy_filter;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Weak};

fn submenu_select_list_layout() -> SelectListLayoutOptions {
    SelectListLayoutOptions {
        min_primary_column_width: Some(12),
        max_primary_column_width: Some(32),
        truncate_primary: None,
    }
}

#[derive(Clone, Default)]
pub struct SelectSubmenuOptions {
    /// Enable type-to-search fuzzy filtering.
    pub searchable: Option<bool>,
    /// Override the select list layout (column widths).
    pub layout: Option<SelectListLayoutOptions>,
}

struct SelectSubmenuInner {
    container: Container,
    select_list: Mutex<Arc<SelectList>>,
    list_child_index: usize,
    all_options: Vec<SelectItem>,
    list_layout: SelectListLayoutOptions,
    search_input: Option<Arc<Input>>,
    on_select_cb: Arc<dyn Fn(String) + Send + Sync>,
    on_cancel_cb: Arc<dyn Fn() + Send + Sync>,
    on_selection_change_cb: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

fn build_select_list(
    options: Vec<SelectItem>,
    preselect: &str,
    layout: &SelectListLayoutOptions,
    on_select: Arc<dyn Fn(String) + Send + Sync>,
    on_cancel: Arc<dyn Fn() + Send + Sync>,
    on_selection_change: Option<Arc<dyn Fn(String) + Send + Sync>>,
) -> Arc<SelectList> {
    let idx = options.iter().position(|o| o.value == preselect);
    let max_visible = options.len().min(10);
    let list = Arc::new(SelectList::new(options, max_visible, (*get_select_list_theme()).clone(), layout.clone()));
    if let Some(idx) = idx {
        list.set_selected_index(idx);
    }
    list.set_on_select(Some(Arc::new(move |item: SelectItem| on_select(item.value))));
    list.set_on_cancel(Some(on_cancel));
    if let Some(cb) = on_selection_change {
        list.set_on_selection_change(Some(Arc::new(move |item: SelectItem| cb(item.value))));
    }
    list
}

impl SelectSubmenuInner {
    fn build_select_list(&self, options: Vec<SelectItem>, preselect: &str) -> Arc<SelectList> {
        build_select_list(
            options,
            preselect,
            &self.list_layout,
            self.on_select_cb.clone(),
            self.on_cancel_cb.clone(),
            self.on_selection_change_cb.clone(),
        )
    }

    fn apply_filter(&self, query: &str) {
        let filtered = if query.is_empty() {
            self.all_options.clone()
        } else {
            fuzzy_filter(&self.all_options, query, |item| {
                format!("{} {}", item.label, item.description.clone().unwrap_or_default())
            })
        };
        let new_list = self.build_select_list(filtered, "");
        let mut children = self.container.children();
        if self.list_child_index < children.len() {
            children[self.list_child_index] = new_list.clone();
        }
        self.container.set_children(children);
        *self.select_list.lock() = new_list;
    }
}

/// Single-step submenu that shows a titled select list.
/// With `searchable: true`, typing filters the list using fuzzy matching.
/// TS: `extends Container`
pub struct SelectSubmenu {
    inner: Arc<SelectSubmenuInner>,
}

impl SelectSubmenu {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        title: &str,
        description: &str,
        options: Vec<SelectItem>,
        current_value: &str,
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        on_selection_change: Option<Arc<dyn Fn(String) + Send + Sync>>,
        submenu_options: Option<&SelectSubmenuOptions>,
    ) -> Self {
        let searchable = submenu_options.and_then(|o| o.searchable).unwrap_or(false);
        let list_layout =
            submenu_options.and_then(|o| o.layout.clone()).unwrap_or_else(submenu_select_list_layout);
        let inner = Arc::new_cyclic(|weak: &Weak<SelectSubmenuInner>| {
            let t = theme();
            let container = Container::new();
            // Title
            container.add_child(Arc::new(Text::new(t.bold(&t.fg(&ThemeColor::Accent, title)), 0, 0, None)));

            // Description
            if !description.is_empty() {
                container.add_child(Arc::new(Spacer::new(1)));
                container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Muted, description), 0, 0, None)));
            }

            // Search input
            let search_input = if searchable {
                container.add_child(Arc::new(Spacer::new(1)));
                let input = Arc::new(Input::default());
                let w = weak.clone();
                input.set_on_submit(Some(Arc::new(move |_v: String| {
                    if let Some(inner) = w.upgrade() {
                        let list = inner.select_list.lock().clone();
                        list.handle_input("\r");
                    }
                })));
                container.add_child(input.clone());
                Some(input)
            } else {
                None
            };

            // Spacer
            container.add_child(Arc::new(Spacer::new(1)));

            // Select list
            let select_list = build_select_list(
                options.clone(),
                current_value,
                &list_layout,
                on_select.clone(),
                on_cancel.clone(),
                on_selection_change.clone(),
            );
            let list_child_index = container.len();
            container.add_child(select_list.clone());

            // Hint
            container.add_child(Arc::new(Spacer::new(1)));
            let hint = if searchable {
                "  Type to filter \u{00b7} Enter to select \u{00b7} Esc to go back"
            } else {
                "  Enter to select \u{00b7} Esc to go back"
            };
            container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Dim, hint), 0, 0, None)));

            SelectSubmenuInner {
                container,
                select_list: Mutex::new(select_list),
                list_child_index,
                all_options: options,
                list_layout,
                search_input,
                on_select_cb: on_select,
                on_cancel_cb: on_cancel,
                on_selection_change_cb: on_selection_change,
            }
        });
        Self { inner }
    }

    /// The current select list.
    pub fn select_list(&self) -> Arc<SelectList> {
        self.inner.select_list.lock().clone()
    }
}

impl Component for SelectSubmenu {
    fn render(&self, width: usize) -> Vec<String> {
        self.inner.container.render(width)
    }

    fn handle_input(&self, data: &str) {
        let inner = &self.inner;
        if let Some(search_input) = &inner.search_input {
            let kb = get_keybindings();
            let is_nav = kb.matches(data, "tui.select.up")
                || kb.matches(data, "tui.select.down")
                || kb.matches(data, "tui.select.confirm")
                || kb.matches(data, "tui.select.cancel");
            if is_nav {
                let list = inner.select_list.lock().clone();
                list.handle_input(data);
            } else {
                search_input.handle_input(data);
                inner.apply_filter(&search_input.get_value());
            }
        } else {
            let list = inner.select_list.lock().clone();
            list.handle_input(data);
        }
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        self.inner.container.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.inner.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.inner.container.child_components()
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
    /// Unique key — the selected value is stored in the result context under this key.
    pub key: String,
    /// Title shown at the top of the step. Receives prior selections.
    pub title: SteppedSubmenuStepTitle,
    /// Description shown below the title. Receives prior selections.
    pub description: SteppedSubmenuStepDescription,
    /// Build the option list for this step. Called fresh each time the step is shown.
    pub options: Arc<dyn Fn(IndexMap<String, String>) -> Vec<SelectItem> + Send + Sync>,
    /// Optionally pre-select a value when entering this step.
    pub preselect: Option<Arc<dyn Fn(IndexMap<String, String>) -> Option<String> + Send + Sync>>,
    /// Enable type-to-search fuzzy filtering for this step.
    pub searchable: Option<bool>,
    /// Override the select list layout (column widths) for this step.
    pub layout: Option<SelectListLayoutOptions>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

struct SteppedInner {
    steps: Vec<SteppedSubmenuStep>,
    on_complete: Arc<dyn Fn(IndexMap<String, String>) + Send + Sync>,
    on_cancel: Arc<dyn Fn() + Send + Sync>,
    opts: SteppedSubmenuOptions,
    active_component: Mutex<Option<ComponentRef>>,
    context: Mutex<IndexMap<String, String>>,
    weak_self: Weak<SteppedInner>,
}

impl SteppedInner {
    fn build_step(&self, step_index: usize) -> ComponentRef {
        let step = self.steps[step_index].clone();
        let total = self.steps.len();
        let step_label = if total > 1 { format!("Step {}/{} \u{00b7} ", step_index + 1, total) } else { String::new() };

        let context = self.context.lock().clone();
        let title = match &step.title {
            SteppedSubmenuStepTitle::String(s) => s.clone(),
            SteppedSubmenuStepTitle::Fn(f) => f(context.clone()),
        };
        let desc = match &step.description {
            SteppedSubmenuStepDescription::String(s) => s.clone(),
            SteppedSubmenuStepDescription::Fn(f) => f(context.clone()),
        };
        let items = (step.options)(context.clone());
        let preselect = step.preselect.as_ref().and_then(|f| f(context.clone())).unwrap_or_default();

        let weak = self.weak_self.clone();
        let key = step.key.clone();
        let on_select: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |value: String| {
            let Some(this) = weak.upgrade() else { return };
            this.context.lock().insert(key.clone(), value);
            if step_index < total - 1 {
                // Advance to next step
                let next = this.build_step(step_index + 1);
                *this.active_component.lock() = Some(next);
            } else {
                // Final step — deliver result
                let result = this.context.lock().clone();
                (this.on_complete)(result);
                if this.opts.r#loop.unwrap_or(false) {
                    this.context.lock().clear();
                    let first = this.build_step(0);
                    *this.active_component.lock() = Some(first);
                } else {
                    (this.on_cancel)();
                }
            }
        });
        let weak = self.weak_self.clone();
        let key = step.key.clone();
        let on_cancel: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let Some(this) = weak.upgrade() else { return };
            if step_index > 0 {
                this.context.lock().shift_remove(&key);
                let prev = this.build_step(step_index - 1);
                *this.active_component.lock() = Some(prev);
            } else {
                (this.on_cancel)();
            }
        });

        let submenu_options = if step.searchable.unwrap_or(false) || step.layout.is_some() {
            Some(SelectSubmenuOptions { searchable: step.searchable, layout: step.layout.clone() })
        } else {
            None
        };

        Arc::new(SelectSubmenu::new(
            &title,
            &format!("{step_label}{desc}"),
            items,
            &preselect,
            on_select,
            on_cancel,
            None,
            submenu_options.as_ref(),
        ))
    }

    fn active(&self) -> Option<ComponentRef> {
        self.active_component.lock().clone()
    }
}

/// Generic N-step submenu built on top of {@link SelectSubmenu}.
///
/// Each step's options can depend on prior selections via the shared context.
/// Esc goes back one step; Esc at step 0 cancels.
/// With `loop: true`, completing the final step invokes `onComplete` then returns to step 0.
/// TS: `extends Container`
pub struct SteppedSubmenu {
    inner: Arc<SteppedInner>,
}

impl SteppedSubmenu {
    pub fn new(
        steps: &[SteppedSubmenuStep],
        on_complete: Arc<dyn Fn(IndexMap<String, String>) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        opts: Option<&SteppedSubmenuOptions>,
    ) -> Self {
        let opts = opts.cloned().unwrap_or_default();
        let context = opts.initial_context.clone().unwrap_or_default();
        let start = opts.start_at_step.unwrap_or(0).max(0) as usize;
        let inner = Arc::new_cyclic(|weak: &Weak<SteppedInner>| SteppedInner {
            steps: steps.to_vec(),
            on_complete,
            on_cancel,
            opts,
            active_component: Mutex::new(None),
            context: Mutex::new(context),
            weak_self: weak.clone(),
        });
        let first = inner.build_step(start);
        *inner.active_component.lock() = Some(first);
        Self { inner }
    }
}

impl Component for SteppedSubmenu {
    fn render(&self, width: usize) -> Vec<String> {
        self.inner.active().map(|c| c.render(width)).unwrap_or_default()
    }

    fn handle_input(&self, data: &str) {
        if let Some(active) = self.inner.active() {
            active.handle_input(data);
        }
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        if let Some(active) = self.inner.active() {
            active.invalidate();
        }
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        // TS: a Container without children (the active step renders through `render`).
        Some(Vec::new())
    }
}
