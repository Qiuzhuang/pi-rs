//! Port of `packages/coding-agent/src/modes/interactive/components/thinking-selector.ts`.

use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::key_display_text;
use crate::modes::interactive::theme::theme::{ThemeColor, get_select_list_theme, theme};
use parking_lot::Mutex;
use pi_agent::types::ThinkingLevel;
use pi_tui::components::input::Input;
use pi_tui::components::select_list::{SelectItem, SelectList, SelectListLayoutOptions};
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::fuzzy::fuzzy_filter;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use std::sync::{Arc, Weak};

fn thinking_select_list_layout() -> SelectListLayoutOptions {
    SelectListLayoutOptions {
        min_primary_column_width: Some(12),
        max_primary_column_width: Some(32),
        truncate_primary: None,
    }
}

fn level_description(level: ThinkingLevel) -> &'static str {
    match level {
        ThinkingLevel::Off => "No reasoning",
        ThinkingLevel::Minimal => "Very brief reasoning (~1k tokens)",
        ThinkingLevel::Low => "Light reasoning (~2k tokens)",
        ThinkingLevel::Medium => "Moderate reasoning (~8k tokens)",
        ThinkingLevel::High => "Deep reasoning (~16k tokens)",
        ThinkingLevel::Xhigh => "Extra-high reasoning (~32k tokens)",
        ThinkingLevel::Max => "Maximum reasoning",
    }
}

struct Inner {
    container: Container,
    search_input: Arc<Input>,
    select_list: Mutex<Arc<SelectList>>,
    select_list_child_index: Mutex<usize>,
    all_items: Vec<SelectItem>,
    on_select: Arc<dyn Fn(ThinkingLevel) + Send + Sync>,
    on_cancel: Arc<dyn Fn() + Send + Sync>,
    on_select_as_default: Option<Arc<dyn Fn(ThinkingLevel) + Send + Sync>>,
    focused: Mutex<bool>,
}

impl Inner {
    fn build_select_list(&self, items: Vec<SelectItem>, preselect: Option<&str>) -> Arc<SelectList> {
        build_select_list(items, preselect, self.on_select.clone(), self.on_cancel.clone())
    }

    fn apply_filter(&self, query: &str) {
        let filtered = if query.is_empty() {
            self.all_items.clone()
        } else {
            fuzzy_filter(&self.all_items, query, |item| {
                format!("{} {}", item.value, item.description.clone().unwrap_or_default())
            })
        };
        let selected_value = self.select_list.lock().get_selected_item().map(|i| i.value);
        let new_list = self.build_select_list(filtered, selected_value.as_deref());
        let idx = *self.select_list_child_index.lock();
        let mut children = self.container.children();
        if idx < children.len() {
            children[idx] = new_list.clone();
        } else {
            children.push(new_list.clone());
        }
        self.container.set_children(children);
        *self.select_list.lock() = new_list;
    }
}

fn build_select_list(
    items: Vec<SelectItem>,
    preselect: Option<&str>,
    on_select: Arc<dyn Fn(ThinkingLevel) + Send + Sync>,
    on_cancel: Arc<dyn Fn() + Send + Sync>,
) -> Arc<SelectList> {
    let current_index = preselect.and_then(|p| items.iter().position(|item| item.value == p));
    let max_visible = items.len().max(1);
    let list = Arc::new(SelectList::new(
        items,
        max_visible,
        (*get_select_list_theme()).clone(),
        thinking_select_list_layout(),
    ));
    if let Some(index) = current_index {
        list.set_selected_index(index);
    }
    list.set_on_select(Some(Arc::new(move |item: SelectItem| {
        if let Some(level) = ThinkingLevel::parse(&item.value) {
            on_select(level);
        }
    })));
    list.set_on_cancel(Some(Arc::new(move || on_cancel())));
    list
}

/// Component that renders a thinking level selector with borders
/// TS: `extends Container implements Focusable`
pub struct ThinkingSelectorComponent {
    inner: Arc<Inner>,
}

impl ThinkingSelectorComponent {
    pub fn new(
        current_level: ThinkingLevel,
        available_levels: &[ThinkingLevel],
        on_select: Arc<dyn Fn(ThinkingLevel) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        on_select_as_default: Option<Arc<dyn Fn(ThinkingLevel) + Send + Sync>>,
        default_thinking_level: Option<ThinkingLevel>,
    ) -> Self {
        let all_items: Vec<SelectItem> = available_levels
            .iter()
            .map(|&level| SelectItem {
                value: level.as_str().to_string(),
                label: format!("{}{}", if level == current_level { "✓ " } else { "  " }, level.as_str()),
                description: Some(if Some(level) == default_thinking_level {
                    format!("{} · default", level_description(level))
                } else {
                    level_description(level).to_string()
                }),
            })
            .collect();

        let inner = Arc::new_cyclic(|weak: &Weak<Inner>| {
            let container = Container::new();
            // Add top border
            container.add_child(Arc::new(DynamicBorder::new(None)));
            container.add_child(Arc::new(Spacer::new(1)));
            container.add_child(Arc::new(Text::new("Thinking Level", 0, 0, None)));
            container.add_child(Arc::new(Spacer::new(1)));
            container.add_child(Arc::new(Text::new(
                format!("{} cycles thinking levels in-session", key_display_text("app.thinking.cycle".into())),
                0,
                0,
                None,
            )));
            container.add_child(Arc::new(Spacer::new(1)));

            let search_input = Arc::new(Input::default());
            let w = weak.clone();
            search_input.set_on_submit(Some(Arc::new(move |_value: String| {
                if let Some(inner) = w.upgrade() {
                    let list = inner.select_list.lock().clone();
                    list.handle_input("\r");
                }
            })));
            container.add_child(search_input.clone());
            container.add_child(Arc::new(Spacer::new(1)));

            // Create selector
            let select_list = build_select_list(
                all_items.clone(),
                Some(current_level.as_str()),
                on_select.clone(),
                on_cancel.clone(),
            );
            let select_list_child_index = container.len();
            container.add_child(select_list.clone());
            container.add_child(Arc::new(Spacer::new(1)));
            container.add_child(Arc::new(Text::new(
                theme().fg(
                    &ThemeColor::Dim,
                    &format!(
                        "  {} to select · {} to set as default · {} to cancel",
                        key_display_text("tui.select.confirm".into()),
                        key_display_text("app.thinking.save".into()),
                        key_display_text("tui.select.cancel".into())
                    ),
                ),
                0,
                0,
                None,
            )));

            // Add bottom border
            container.add_child(Arc::new(DynamicBorder::new(None)));

            Inner {
                container,
                search_input,
                select_list: Mutex::new(select_list),
                select_list_child_index: Mutex::new(select_list_child_index),
                all_items,
                on_select,
                on_cancel,
                on_select_as_default,
                focused: Mutex::new(false),
            }
        });
        Self { inner }
    }

    pub fn get_select_list(&self) -> Arc<SelectList> {
        self.inner.select_list.lock().clone()
    }
}

impl Component for ThinkingSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.inner.container.render(width)
    }

    fn handle_input(&self, data: &str) {
        let inner = &self.inner;
        let kb = get_keybindings();
        if kb.matches(data, "app.thinking.save")
            && let Some(on_default) = &inner.on_select_as_default
        {
            let item = inner.select_list.lock().get_selected_item();
            if let Some(item) = item
                && let Some(level) = ThinkingLevel::parse(&item.value)
            {
                on_default(level);
            }
            return;
        }

        let is_nav = kb.matches(data, "tui.select.up")
            || kb.matches(data, "tui.select.down")
            || kb.matches(data, "tui.select.confirm")
            || kb.matches(data, "tui.select.cancel");
        if is_nav {
            let list = inner.select_list.lock().clone();
            list.handle_input(data);
            return;
        }

        inner.search_input.handle_input(data);
        inner.apply_filter(&inner.search_input.get_value());
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

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        *self.inner.focused.lock() = focused;
        self.inner.search_input.set_focused(focused);
    }

    fn focused(&self) -> bool {
        *self.inner.focused.lock()
    }
}
