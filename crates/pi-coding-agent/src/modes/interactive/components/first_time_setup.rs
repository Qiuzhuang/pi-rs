//! Port of `packages/coding-agent/src/modes/interactive/components/first-time-setup.ts`.

use crate::config::APP_NAME;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::{key_hint, raw_key_hint};
use crate::modes::interactive::theme::system_theme::SYSTEM_THEME_NAME;
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use parking_lot::Mutex;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstTimeSetupResult {
    pub theme: String,
    pub share_analytics: bool,
}

#[derive(Clone)]
pub struct FirstTimeSetupOptions {
    pub on_theme_preview: Arc<dyn Fn(String) + Send + Sync>,
    pub on_submit: Arc<dyn Fn(FirstTimeSetupResult) + Send + Sync>,
    pub on_cancel: Arc<dyn Fn() + Send + Sync>,
}

fn theme_options() -> [(&'static str, &'static str); 3] {
    [
        (SYSTEM_THEME_NAME, "System (matches your terminal colors)"),
        ("dark", "Dark"),
        ("light", "Light"),
    ]
}

const ANALYTICS_OPTIONS: [(bool, &str); 2] = [(true, "Share anonymous usage data"), (false, "Don't share")];

const SETUP_LOGO_LINES: [&str; 4] = ["██████", "██  ██", "████  ██", "██    ██"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Theme,
    Analytics,
}

struct State {
    step: Step,
    theme_index: usize,
    analytics_index: usize,
}

/// First-time setup dialog: theme choice and analytics opt-in.
/// TS: `extends Container`
pub struct FirstTimeSetupComponent {
    container: Container,
    state: Mutex<State>,
    options: FirstTimeSetupOptions,
}

impl FirstTimeSetupComponent {
    pub fn new(options: &FirstTimeSetupOptions) -> Self {
        let this = Self {
            container: Container::new(),
            state: Mutex::new(State { step: Step::Theme, theme_index: 0, analytics_index: 0 }),
            options: options.clone(),
        };
        this.update();
        this
    }

    // Rebuild the whole dialog on every change so theme previews recolor all text.
    fn update(&self) {
        let (step, theme_index, analytics_index) = {
            let s = self.state.lock();
            (s.step, s.theme_index, s.analytics_index)
        };
        let t = theme();
        let c = &self.container;
        c.clear();
        c.add_child(Arc::new(DynamicBorder::new(None)));
        c.add_child(Arc::new(Spacer::new(1)));
        c.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Accent, &SETUP_LOGO_LINES.join("\n")), 1, 0, None)));
        c.add_child(Arc::new(Spacer::new(1)));
        c.add_child(Arc::new(Text::new(
            t.fg(
                &ThemeColor::Accent,
                &t.bold(&format!("Welcome to {APP_NAME}, the minimal coding agent.")),
            ),
            1,
            0,
            None,
        )));
        c.add_child(Arc::new(Spacer::new(1)));

        if step == Step::Theme {
            c.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Text, "Pick a theme."), 1, 0, None)));
            c.add_child(Arc::new(Spacer::new(1)));
            let labels: Vec<&str> = theme_options().iter().map(|(_, label)| *label).collect();
            self.add_option_list(&labels, theme_index);
        } else {
            c.add_child(Arc::new(Text::new(
                t.fg(&ThemeColor::Text, "Opt-in to anonymous usage data sharing?"),
                1,
                0,
                None,
            )));
            c.add_child(Arc::new(Text::new(
                t.fg(
                    &ThemeColor::Muted,
                    "Opting in stores a tracking identifier in settings.json and enables anonymous\nusage analytics. This helps us to better debug, reproduce, and resolve issues\nand bugs within Pi. You can observe what is shared using /privacy and make\nchanges anytime in settings.json.",
                ),
                1,
                0,
                None,
            )));
            c.add_child(Arc::new(Spacer::new(1)));
            let labels: Vec<&str> = ANALYTICS_OPTIONS.iter().map(|(_, label)| *label).collect();
            self.add_option_list(&labels, analytics_index);
        }

        c.add_child(Arc::new(Spacer::new(1)));
        c.add_child(Arc::new(Text::new(
            format!(
                "{}  {}  {}",
                raw_key_hint("↑↓", "navigate"),
                key_hint(
                    "tui.select.confirm".into(),
                    if step == Step::Theme { "continue" } else { "finish" }
                ),
                key_hint("tui.select.cancel".into(), "skip setup")
            ),
            1,
            0,
            None,
        )));
        c.add_child(Arc::new(Spacer::new(1)));
        c.add_child(Arc::new(DynamicBorder::new(None)));
    }

    fn add_option_list(&self, labels: &[&str], selected_index: usize) {
        let t = theme();
        for (i, label) in labels.iter().enumerate() {
            let is_selected = i == selected_index;
            let prefix = if is_selected { t.fg(&ThemeColor::Accent, "→ ") } else { "  ".to_string() };
            let label = if is_selected { t.fg(&ThemeColor::Accent, label) } else { t.fg(&ThemeColor::Text, label) };
            self.container.add_child(Arc::new(Text::new(format!("{prefix}{label}"), 1, 0, None)));
        }
    }

    fn move_selection(&self, delta: i64) {
        let preview = {
            let mut s = self.state.lock();
            if s.step == Step::Theme {
                let max = theme_options().len() as i64 - 1;
                let next = (s.theme_index as i64 + delta).clamp(0, max) as usize;
                if next != s.theme_index {
                    s.theme_index = next;
                    Some(theme_options()[next].0.to_string())
                } else {
                    None
                }
            } else {
                let max = ANALYTICS_OPTIONS.len() as i64 - 1;
                s.analytics_index = (s.analytics_index as i64 + delta).clamp(0, max) as usize;
                None
            }
        };
        if let Some(name) = preview {
            (self.options.on_theme_preview)(name);
        }
        self.update();
    }
}

impl Component for FirstTimeSetupComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn handle_input(&self, data: &str) {
        let kb = get_keybindings();
        if kb.matches(data, "tui.select.up") || data == "k" {
            self.move_selection(-1);
        } else if kb.matches(data, "tui.select.down") || data == "j" {
            self.move_selection(1);
        } else if kb.matches(data, "tui.select.confirm") || data == "\n" {
            let submit = {
                let mut s = self.state.lock();
                if s.step == Step::Theme {
                    s.step = Step::Analytics;
                    None
                } else {
                    Some(FirstTimeSetupResult {
                        theme: theme_options()[s.theme_index].0.to_string(),
                        share_analytics: ANALYTICS_OPTIONS[s.analytics_index].0,
                    })
                }
            };
            match submit {
                None => self.update(),
                Some(result) => (self.options.on_submit)(result),
            }
        } else if kb.matches(data, "tui.select.cancel") {
            (self.options.on_cancel)();
        }
    }

    fn handles_input(&self) -> bool {
        true
    }

    /// Rebuild on theme changes, e.g. when the system theme receives the terminal's colors.
    fn invalidate(&self) {
        self.update();
        self.container.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.container.child_components()
    }
}
