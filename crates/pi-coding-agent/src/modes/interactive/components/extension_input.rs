//! Port of `packages/coding-agent/src/modes/interactive/components/extension-input.ts`.
//!
//! Simple text input component for extensions.

use crate::modes::interactive::components::countdown_timer::CountdownTimer;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::key_hint;
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use parking_lot::Mutex;
use pi_tui::components::input::{Input, InputOptions};
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult, Tui};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Default)]
pub struct ExtensionInputOptions {
    pub tui: Option<Arc<Tui>>,
    pub timeout: Option<i64>,
    pub initial_value: Option<String>,
    pub description: Option<String>,
}

/// TS: `extends Container implements Focusable`
pub struct ExtensionInputComponent {
    container: Container,
    input: Arc<Input>,
    on_submit_callback: Arc<dyn Fn(String) + Send + Sync>,
    on_cancel_callback: Arc<dyn Fn() + Send + Sync>,
    countdown: Mutex<Option<CountdownTimer>>,
    focused: AtomicBool,
}

impl ExtensionInputComponent {
    pub fn new(
        title: &str,
        _placeholder: Option<&str>,
        on_submit: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        opts: Option<&ExtensionInputOptions>,
    ) -> Self {
        let t = theme();
        let container = Container::new();
        container.add_child(Arc::new(DynamicBorder::new(None)));
        container.add_child(Arc::new(Spacer::new(1)));

        let title_text = Arc::new(Text::new(t.fg(&ThemeColor::Accent, title), 1, 0, None));
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
                Arc::new(move |s| title_text.set_text(theme().fg(&ThemeColor::Accent, &format!("{base_title} ({s}s)")))),
                Arc::new(move || on_cancel()),
            ));
        }

        let input = Arc::new(Input::new(InputOptions::default()));
        if let Some(initial) = opts.and_then(|o| o.initial_value.clone()).filter(|v| !v.is_empty()) {
            input.set_value(initial);
        }
        container.add_child(input.clone() as ComponentRef);
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(Text::new(
            format!(
                "{}  {}",
                key_hint("tui.select.confirm".to_string(), "submit"),
                key_hint("tui.select.cancel".to_string(), "cancel")
            ),
            1,
            0,
            None,
        )));
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(DynamicBorder::new(None)));

        Self {
            container,
            input,
            on_submit_callback: on_submit,
            on_cancel_callback: on_cancel,
            countdown: Mutex::new(countdown),
            focused: AtomicBool::new(false),
        }
    }

    pub fn dispose(&self) {
        if let Some(countdown) = self.countdown.lock().as_ref() {
            countdown.dispose();
        }
    }
}

impl Component for ExtensionInputComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn handle_input(&self, key_data: &str) {
        let kb = get_keybindings();
        if kb.matches(key_data, "tui.select.confirm") || key_data == "\n" {
            (self.on_submit_callback)(self.input.get_value());
        } else if kb.matches(key_data, "tui.select.cancel") {
            (self.on_cancel_callback)();
        } else {
            self.input.handle_input(key_data);
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

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::SeqCst);
        self.input.set_focused(focused);
    }

    fn focused(&self) -> bool {
        self.focused.load(Ordering::SeqCst)
    }
}
