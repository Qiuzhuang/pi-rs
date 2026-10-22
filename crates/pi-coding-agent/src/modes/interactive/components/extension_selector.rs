//! Port of `packages/coding-agent/src/modes/interactive/components/extension-selector.ts`.
//!
//! Generic selector component for extensions.
//! Displays a list of string options with keyboard navigation.

use crate::modes::interactive::components::countdown_timer::CountdownTimer;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::{key_hint, raw_key_hint};
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use parking_lot::Mutex;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult, Tui};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Default)]
pub struct ExtensionSelectorOptions {
    pub tui: Option<Arc<Tui>>,
    pub timeout: Option<i64>,
    pub on_toggle_tools_expanded: Option<Arc<dyn Fn() + Send + Sync>>,
    pub description: Option<String>,
}

/// TS: `extends Container`
pub struct ExtensionSelectorComponent {
    container: Container,
    options: Vec<String>,
    selected_index: AtomicUsize,
    list_container: Arc<Container>,
    on_select_callback: Arc<dyn Fn(String) + Send + Sync>,
    on_cancel_callback: Arc<dyn Fn() + Send + Sync>,
    countdown: Mutex<Option<CountdownTimer>>,
    on_toggle_tools_expanded: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ExtensionSelectorComponent {
    pub fn new(
        title: &str,
        options: &[String],
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        opts: Option<&ExtensionSelectorOptions>,
    ) -> Self {
        let t = theme();
        let container = Container::new();
        container.add_child(Arc::new(DynamicBorder::new(None)));
        container.add_child(Arc::new(Spacer::new(1)));

        let title_text = Arc::new(Text::new(t.fg(&ThemeColor::Accent, &t.bold(title)), 1, 0, None));
        container.add_child(title_text.clone() as ComponentRef);
        if let Some(description) = opts.and_then(|o| o.description.clone()).filter(|d| !d.is_empty()) {
            container.add_child(Arc::new(Spacer::new(1)));
            container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Text, &description), 1, 0, None)));
        }
        container.add_child(Arc::new(Spacer::new(1)));

        let mut countdown = None;
        if let Some(opts) = opts
            && let (Some(timeout), Some(tui)) = (opts.timeout, opts.tui.clone())
            && timeout > 0
        {
            let base_title = title.to_string();
            let title_text = title_text.clone();
            let on_cancel = on_cancel.clone();
            countdown = Some(CountdownTimer::new(
                timeout,
                Some(tui),
                Arc::new(move |s| {
                    let t = theme();
                    title_text.set_text(t.fg(&ThemeColor::Accent, &t.bold(&format!("{base_title} ({s}s)"))));
                }),
                Arc::new(move || on_cancel()),
            ));
        }

        let list_container = Arc::new(Container::new());
        container.add_child(list_container.clone() as ComponentRef);
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(Text::new(
            raw_key_hint("↑↓", "navigate")
                + "  "
                + &key_hint("tui.select.confirm".to_string(), "select")
                + "  "
                + &key_hint("tui.select.cancel".to_string(), "cancel"),
            1,
            0,
            None,
        )));
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(DynamicBorder::new(None)));

        let component = Self {
            container,
            options: options.to_vec(),
            selected_index: AtomicUsize::new(0),
            list_container,
            on_select_callback: on_select,
            on_cancel_callback: on_cancel,
            countdown: Mutex::new(countdown),
            on_toggle_tools_expanded: opts.and_then(|o| o.on_toggle_tools_expanded.clone()),
        };
        component.update_list();
        component
    }

    fn update_list(&self) {
        let t = theme();
        self.list_container.clear();
        let selected = self.selected_index.load(Ordering::SeqCst);
        for (i, option) in self.options.iter().enumerate() {
            let text = if i == selected {
                t.fg(&ThemeColor::Accent, "→ ") + &t.fg(&ThemeColor::Accent, option)
            } else {
                format!("  {}", t.fg(&ThemeColor::Text, option))
            };
            self.list_container.add_child(Arc::new(Text::new(text, 1, 0, None)));
        }
    }

    pub fn dispose(&self) {
        if let Some(countdown) = self.countdown.lock().as_ref() {
            countdown.dispose();
        }
    }
}

impl Component for ExtensionSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn handle_input(&self, key_data: &str) {
        let kb = get_keybindings();
        if kb.matches(key_data, "app.tools.expand") {
            if let Some(f) = &self.on_toggle_tools_expanded {
                f();
            }
        } else if kb.matches(key_data, "tui.select.up") || key_data == "k" {
            let current = self.selected_index.load(Ordering::SeqCst);
            self.selected_index.store(current.saturating_sub(1), Ordering::SeqCst);
            self.update_list();
        } else if kb.matches(key_data, "tui.select.down") || key_data == "j" {
            let current = self.selected_index.load(Ordering::SeqCst) as i64;
            let next = (current + 1).min(self.options.len() as i64 - 1).max(0);
            self.selected_index.store(next as usize, Ordering::SeqCst);
            self.update_list();
        } else if kb.matches(key_data, "tui.select.confirm") || key_data == "\n" {
            let selected = self.options.get(self.selected_index.load(Ordering::SeqCst)).cloned();
            if let Some(selected) = selected.filter(|s| !s.is_empty()) {
                (self.on_select_callback)(selected);
            }
        } else if kb.matches(key_data, "tui.select.cancel") {
            (self.on_cancel_callback)();
        }
    }

    fn handles_input(&self) -> bool {
        true
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
