//! Port of `packages/coding-agent/src/modes/interactive/components/show-images-selector.ts`.

use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::theme::theme::get_select_list_theme;
use pi_tui::components::select_list::{SelectItem, SelectList, SelectListLayoutOptions};
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use std::sync::Arc;

fn show_images_select_list_layout() -> SelectListLayoutOptions {
    SelectListLayoutOptions {
        min_primary_column_width: Some(12),
        max_primary_column_width: Some(32),
        truncate_primary: None,
    }
}

/// Component that renders a show images selector with borders
/// TS: `extends Container`
pub struct ShowImagesSelectorComponent {
    container: Container,
    select_list: Arc<SelectList>,
}

impl ShowImagesSelectorComponent {
    pub fn new(
        current_value: bool,
        on_select: Arc<dyn Fn(bool) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let container = Container::new();
        let items = vec![
            SelectItem::new("yes", "Yes", Some("Show images inline in terminal".to_string())),
            SelectItem::new("no", "No", Some("Show text placeholder instead".to_string())),
        ];

        // Add top border
        container.add_child(Arc::new(DynamicBorder::new(None)));

        // Create selector
        let select_list = Arc::new(SelectList::new(
            items,
            5,
            (*get_select_list_theme()).clone(),
            show_images_select_list_layout(),
        ));

        // Preselect current value
        select_list.set_selected_index(if current_value { 0 } else { 1 });

        select_list.set_on_select(Some(Arc::new(move |item: SelectItem| on_select(item.value == "yes"))));
        select_list.set_on_cancel(Some(Arc::new(move || on_cancel())));

        container.add_child(select_list.clone());

        // Add bottom border
        container.add_child(Arc::new(DynamicBorder::new(None)));

        Self { container, select_list }
    }

    pub fn get_select_list(&self) -> Arc<SelectList> {
        self.select_list.clone()
    }
}

impl Component for ShowImagesSelectorComponent {
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
