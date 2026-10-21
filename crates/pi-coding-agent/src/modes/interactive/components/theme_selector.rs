//! Port of `packages/coding-agent/src/modes/interactive/components/theme-selector.ts`.

use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::theme::theme::{get_available_themes, get_select_list_theme};
use pi_tui::components::select_list::{SelectItem, SelectList, SelectListLayoutOptions};
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use std::sync::Arc;

fn theme_select_list_layout() -> SelectListLayoutOptions {
    SelectListLayoutOptions {
        min_primary_column_width: Some(12),
        max_primary_column_width: Some(32),
        truncate_primary: None,
    }
}

/// Component that renders a theme selector
/// TS: `extends Container`
pub struct ThemeSelectorComponent {
    container: Container,
    select_list: Arc<SelectList>,
}

impl ThemeSelectorComponent {
    pub fn new(
        current_theme: &str,
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        on_preview: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Self {
        let container = Container::new();

        // Get available themes and create select items
        let themes = get_available_themes();
        let theme_items: Vec<SelectItem> = themes
            .iter()
            .map(|name| SelectItem {
                value: name.clone(),
                label: name.clone(),
                description: if name == current_theme { Some("(current)".to_string()) } else { None },
            })
            .collect();

        // Add top border
        container.add_child(Arc::new(DynamicBorder::new(None)));

        // Create selector
        let select_list = Arc::new(SelectList::new(
            theme_items,
            10,
            (*get_select_list_theme()).clone(),
            theme_select_list_layout(),
        ));

        // Preselect current theme
        if let Some(current_index) = themes.iter().position(|t| t == current_theme) {
            select_list.set_selected_index(current_index);
        }

        select_list.set_on_select(Some(Arc::new(move |item: SelectItem| on_select(item.value))));
        select_list.set_on_cancel(Some(Arc::new(move || on_cancel())));
        select_list.set_on_selection_change(Some(Arc::new(move |item: SelectItem| on_preview(item.value))));

        container.add_child(select_list.clone());

        // Add bottom border
        container.add_child(Arc::new(DynamicBorder::new(None)));

        Self { container, select_list }
    }

    pub fn get_select_list(&self) -> Arc<SelectList> {
        self.select_list.clone()
    }
}

impl Component for ThemeSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn invalidate(&self) {
        self.container.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.container.child_components()
    }
}
