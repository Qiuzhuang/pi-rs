//! Port of `packages/coding-agent/src/modes/interactive/components/trust-selector.ts`.

use crate::core::trust_manager::ProjectTrustOption;
use crate::core::trust_manager::ProjectTrustStoreEntry;
use crate::core::trust_manager::ProjectTrustUpdate;
use crate::core::trust_manager::get_project_trust_options;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::{key_hint, raw_key_hint};
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use parking_lot::Mutex;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// TS: Pick<ProjectTrustOption, "trusted" | "updates">
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustSelection {
    pub trusted: bool,
    pub updates: Vec<ProjectTrustUpdate>,
}

#[derive(Clone)]
pub struct TrustSelectorOptions {
    pub cwd: String,
    pub saved_decision: Option<ProjectTrustStoreEntry>,
    pub project_trusted: bool,
    pub on_select: Arc<dyn Fn(TrustSelection) + Send + Sync>,
    pub on_cancel: Arc<dyn Fn() + Send + Sync>,
}

fn format_decision(trust_path: Option<&str>, decision: Option<&ProjectTrustStoreEntry>) -> String {
    let Some(decision) = decision else {
        return "none".to_string();
    };
    let label = if decision.decision { "trusted" } else { "untrusted" };
    if let Some(trust_path) = trust_path {
        if decision.path != trust_path {
            return format!("{label} (inherited from {})", decision.path);
        }
    }
    format!("{label} ({})", decision.path)
}

/// TS: `extends Container`
pub struct TrustSelectorComponent {
    container: Container,
    selected_index: Mutex<usize>,
    list_container: Arc<Container>,
    trust_options: Vec<ProjectTrustOption>,
    saved_decision: Option<ProjectTrustStoreEntry>,
    on_select_callback: Arc<dyn Fn(TrustSelection) + Send + Sync>,
    on_cancel_callback: Arc<dyn Fn() + Send + Sync>,
}

impl TrustSelectorComponent {
    pub fn new(options: &TrustSelectorOptions) -> Self {
        let trust_options = get_project_trust_options(&options.cwd, None);
        let saved_decision = options.saved_decision.clone();
        let selected_index = trust_options
            .iter()
            .position(|option| is_saved_option(saved_decision.as_ref(), option))
            .unwrap_or(0);

        let t = theme();
        let container = Container::new();
        container.add_child(Arc::new(DynamicBorder::new(None)));
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(Text::new(
            t.fg(&ThemeColor::Accent, &t.bold("Project trust")),
            1,
            0,
            None,
        )));
        container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Muted, &options.cwd), 1, 0, None)));
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(Text::new(
            t.fg(
                &ThemeColor::Muted,
                &format!(
                    "Saved decision: {}",
                    format_decision(
                        trust_options.first().and_then(|o| o.saved_path.as_deref()),
                        options.saved_decision.as_ref()
                    )
                ),
            ),
            1,
            0,
            None,
        )));
        container.add_child(Arc::new(Text::new(
            t.fg(
                &ThemeColor::Muted,
                &format!(
                    "Current session: {}",
                    if options.project_trusted { "trusted" } else { "untrusted" }
                ),
            ),
            1,
            0,
            None,
        )));
        container.add_child(Arc::new(Spacer::new(1)));

        let list_container = Arc::new(Container::new());
        container.add_child(list_container.clone());
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(Text::new(
            format!(
                "{}  {}  {}",
                raw_key_hint("↑↓", "navigate"),
                key_hint("tui.select.confirm".into(), "save"),
                key_hint("tui.select.cancel".into(), "cancel")
            ),
            1,
            0,
            None,
        )));
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(DynamicBorder::new(None)));

        let this = Self {
            container,
            selected_index: Mutex::new(selected_index),
            list_container,
            trust_options,
            saved_decision,
            on_select_callback: options.on_select.clone(),
            on_cancel_callback: options.on_cancel.clone(),
        };
        this.update_list();
        this
    }

    fn update_list(&self) {
        self.list_container.clear();
        let t = theme();
        let selected_index = *self.selected_index.lock();
        for (i, option) in self.trust_options.iter().enumerate() {
            let is_selected = i == selected_index;
            let is_current = is_saved_option(self.saved_decision.as_ref(), option);
            let current_marker = if is_current { t.fg(&ThemeColor::Accent, "✓ ") } else { "  ".to_string() };
            let prefix = if is_selected { t.fg(&ThemeColor::Accent, "→ ") } else { "  ".to_string() };
            let label = if is_selected {
                t.fg(&ThemeColor::Accent, &option.label)
            } else {
                t.fg(&ThemeColor::Text, &option.label)
            };
            self.list_container
                .add_child(Arc::new(Text::new(format!("{prefix}{current_marker}{label}"), 1, 0, None)));
        }
    }
}

fn is_saved_option(saved: Option<&ProjectTrustStoreEntry>, option: &ProjectTrustOption) -> bool {
    match (&option.saved_path, saved) {
        (Some(saved_path), Some(saved)) => saved.decision == option.trusted && &saved.path == saved_path,
        _ => false,
    }
}

impl Component for TrustSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn handle_input(&self, data: &str) {
        let kb = get_keybindings();
        if kb.matches(data, "tui.select.up") || data == "k" {
            {
                let mut idx = self.selected_index.lock();
                *idx = idx.saturating_sub(1);
            }
            self.update_list();
        } else if kb.matches(data, "tui.select.down") || data == "j" {
            {
                let mut idx = self.selected_index.lock();
                let max = self.trust_options.len().saturating_sub(1);
                *idx = (*idx + 1).min(max);
            }
            self.update_list();
        } else if kb.matches(data, "tui.select.confirm") || data == "\n" {
            let idx = *self.selected_index.lock();
            if let Some(selected) = self.trust_options.get(idx) {
                (self.on_select_callback)(TrustSelection {
                    trusted: selected.trusted,
                    updates: selected.updates.clone(),
                });
            }
        } else if kb.matches(data, "tui.select.cancel") {
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
