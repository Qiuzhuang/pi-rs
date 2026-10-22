//! Port of `packages/coding-agent/src/modes/interactive/components/scoped-models-selector.ts`.

use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::key_display_text;
use crate::modes::interactive::model_search::{ModelSearchItem, get_model_search_text};
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use futures::FutureExt;
use futures::future::BoxFuture;
use parking_lot::Mutex;
use pi_ai::types::Model;
use pi_tui::components::input::Input;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::fuzzy::fuzzy_filter;
use pi_tui::keybindings::get_keybindings;
use pi_tui::keys::{Key, matches_key};
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type EnabledIds = Option<Vec<String>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelItem {
    pub full_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsConfig {
    pub all_models: Vec<Model>,
    #[serde(default)]
    pub enabled_model_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_status: Option<String>,
}

#[derive(Clone)]
pub struct ModelsCallbacks {
    /// Called whenever the enabled model set or order changes (session-only, no persist)
    pub on_change:
        Arc<dyn Fn(Option<Vec<String>>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    /// Called when user wants to persist current selection to settings
    pub on_persist:
        Arc<dyn Fn(Option<Vec<String>>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    pub on_cancel: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScopedModelsSelectorComponentSetRefreshStatusKind {
    #[serde(rename = "muted")]
    Muted,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "warning")]
    Warning,
}

impl ScopedModelsSelectorComponentSetRefreshStatusKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Muted => "muted",
            Self::Success => "success",
            Self::Warning => "warning",
        }
    }
}


fn is_enabled(enabled_ids: &EnabledIds, id: &str) -> bool {
    match enabled_ids {
        None => true,
        Some(ids) => ids.iter().any(|x| x == id),
    }
}

/// Collapse an explicit list back to null (= all enabled) when it covers every available model.
fn normalize_enabled(result: Vec<String>, all_ids: &[String]) -> EnabledIds {
    if result.len() == all_ids.len() && result.iter().all(|id| all_ids.contains(id)) { None } else { Some(result) }
}

fn toggle(enabled_ids: &EnabledIds, all_ids: &[String], id: &str) -> EnabledIds {
    let Some(ids) = enabled_ids else {
        return Some(all_ids.iter().filter(|m| *m != id).cloned().collect());
    };
    if let Some(index) = ids.iter().position(|x| x == id) {
        let mut result = ids.clone();
        result.remove(index);
        return Some(result);
    }
    let mut result = ids.clone();
    result.push(id.to_string());
    normalize_enabled(result, all_ids)
}

fn enable_all(enabled_ids: &EnabledIds, all_ids: &[String], target_ids: Option<&[String]>) -> EnabledIds {
    let Some(ids) = enabled_ids else {
        return None; // Already all enabled
    };
    let targets = target_ids.unwrap_or(all_ids);
    let mut result = ids.clone();
    for id in targets {
        if !result.contains(id) {
            result.push(id.clone());
        }
    }
    normalize_enabled(result, all_ids)
}

fn clear_all(enabled_ids: &EnabledIds, all_ids: &[String], target_ids: Option<&[String]>) -> EnabledIds {
    let Some(ids) = enabled_ids else {
        return Some(match target_ids {
            Some(targets) => all_ids.iter().filter(|id| !targets.contains(id)).cloned().collect(),
            None => Vec::new(),
        });
    };
    let targets: HashSet<&String> = target_ids.unwrap_or(ids).iter().collect();
    Some(ids.iter().filter(|id| !targets.contains(id)).cloned().collect())
}

fn move_id(enabled_ids: &EnabledIds, id: &str, delta: i64) -> EnabledIds {
    let ids = enabled_ids.as_ref()?;
    let mut list = ids.clone();
    let Some(index) = list.iter().position(|x| x == id) else {
        return Some(list);
    };
    let new_index = index as i64 + delta;
    if new_index < 0 || new_index >= list.len() as i64 {
        return Some(list);
    }
    list.swap(index, new_index as usize);
    Some(list)
}

fn get_sorted_ids(enabled_ids: &EnabledIds, all_ids: &[String]) -> Vec<String> {
    let Some(ids) = enabled_ids else {
        return all_ids.to_vec();
    };
    let enabled_set: HashSet<&String> = ids.iter().collect();
    let mut result = ids.clone();
    result.extend(all_ids.iter().filter(|id| !enabled_set.contains(id)).cloned());
    result
}

/// Run a callback future without awaiting it (TS: fire-and-forget promise).
fn fire_and_forget(future: BoxFuture<'static, anyhow::Result<()>>) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        crate::utils::uncaught::spawn_on(&handle, future);
    } else {
        let _ = future.now_or_never();
    }
}

struct State {
    models_by_id: HashMap<String, Model>,
    all_ids: Vec<String>,
    enabled_ids: EnabledIds,
    filtered_items: Vec<ModelItem>,
    selected_index: usize,
    is_dirty: bool,
    focused: bool,
}

impl State {
    fn build_items(&self) -> Vec<ModelItem> {
        get_sorted_ids(&self.enabled_ids, &self.all_ids)
            .into_iter()
            .map(|id| ModelItem {
                model: self.models_by_id.get(&id).cloned(),
                enabled: is_enabled(&self.enabled_ids, &id),
                full_id: id,
            })
            .collect()
    }

    fn footer_text(&self) -> String {
        let t = theme();
        let enabled_count = match &self.enabled_ids {
            Some(ids) => ids.iter().filter(|id| self.models_by_id.contains_key(*id)).count(),
            None => self.all_ids.len(),
        };
        let unavailable_count = match &self.enabled_ids {
            Some(ids) => ids.iter().filter(|id| !self.models_by_id.contains_key(*id)).count(),
            None => 0,
        };
        let count_text = if self.enabled_ids.is_none() {
            "all enabled".to_string()
        } else {
            format!(
                "{enabled_count}/{} enabled{}",
                self.all_ids.len(),
                if unavailable_count > 0 { format!(" · {unavailable_count} unavailable") } else { String::new() }
            )
        };
        let parts = [
            format!("{} toggle", key_display_text("tui.select.confirm".into())),
            format!("{} all", key_display_text("app.models.enableAll".into())),
            format!("{} clear", key_display_text("app.models.clearAll".into())),
            format!("{} provider", key_display_text("app.models.toggleProvider".into())),
            format!(
                "{}/{} reorder",
                key_display_text("app.models.reorderUp".into()),
                key_display_text("app.models.reorderDown".into())
            ),
            format!("{} save", key_display_text("app.models.save".into())),
            count_text,
        ];
        if self.is_dirty {
            t.fg(&ThemeColor::Dim, &format!("  {} ", parts.join(" · "))) + &t.fg(&ThemeColor::Warning, "(unsaved)")
        } else {
            t.fg(&ThemeColor::Dim, &format!("  {}", parts.join(" · ")))
        }
    }
}

/// Component for enabling/disabling models for Ctrl+P cycling.
/// Changes are session-only until explicitly persisted with Ctrl+S.
/// TS: `extends Container implements Focusable`
pub struct ScopedModelsSelectorComponent {
    container: Container,
    search_input: Arc<Input>,
    list_container: Arc<Container>,
    footer_text: Arc<Text>,
    callbacks: ModelsCallbacks,
    max_visible: usize,
    refresh_status_text: Option<Arc<Text>>,
    state: Mutex<State>,
}

impl ScopedModelsSelectorComponent {
    pub fn new(config: &ModelsConfig, callbacks: &ModelsCallbacks) -> Self {
        let mut models_by_id = HashMap::new();
        let mut all_ids = Vec::new();
        for model in &config.all_models {
            let full_id = format!("{}/{}", model.provider, model.id);
            models_by_id.insert(full_id.clone(), model.clone());
            all_ids.push(full_id);
        }
        let mut state = State {
            models_by_id,
            all_ids,
            enabled_ids: config.enabled_model_ids.clone(),
            filtered_items: Vec::new(),
            selected_index: 0,
            is_dirty: false,
            focused: false,
        };
        state.filtered_items = state.build_items();

        let t = theme();
        let container = Container::new();
        // Header
        container.add_child(Arc::new(DynamicBorder::new(None)));
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Accent, &t.bold("Model Configuration")), 0, 0, None)));
        container.add_child(Arc::new(Text::new(
            t.fg(
                &ThemeColor::Muted,
                &format!("Session-only. {} to save to settings.", key_display_text("app.models.save".into())),
            ),
            0,
            0,
            None,
        )));
        container.add_child(Arc::new(Spacer::new(1)));

        // Search input
        let search_input = Arc::new(Input::default());
        container.add_child(search_input.clone());
        container.add_child(Arc::new(Spacer::new(1)));

        // List container
        let list_container = Arc::new(Container::new());
        container.add_child(list_container.clone());

        // Footer hint
        container.add_child(Arc::new(Spacer::new(1)));
        let refresh_status_text = config.refresh_status.as_deref().filter(|s| !s.is_empty()).map(|status| {
            let text = Arc::new(Text::new(t.fg(&ThemeColor::Muted, &format!("  {status}")), 0, 0, None));
            container.add_child(text.clone());
            text
        });
        let footer_text = Arc::new(Text::new(state.footer_text(), 0, 0, None));
        container.add_child(footer_text.clone());
        container.add_child(Arc::new(DynamicBorder::new(None)));

        let this = Self {
            container,
            search_input,
            list_container,
            footer_text,
            callbacks: callbacks.clone(),
            max_visible: 8,
            refresh_status_text,
            state: Mutex::new(state),
        };
        this.update_list();
        this
    }

    /// `enabled_model_ids`: `None` keeps the current set; `Some(None)` enables all; `Some(Some(ids))` sets them.
    pub fn update_models(&self, models: &[Model], enabled_model_ids: Option<EnabledIds>) {
        let selected_id = {
            let mut s = self.state.lock();
            let selected_id = s.filtered_items.get(s.selected_index).map(|i| i.full_id.clone());
            if let Some(enabled) = enabled_model_ids {
                s.enabled_ids = enabled;
            }
            s.models_by_id.clear();
            s.all_ids.clear();
            for model in models {
                let full_id = format!("{}/{}", model.provider, model.id);
                s.models_by_id.insert(full_id.clone(), model.clone());
                s.all_ids.push(full_id);
            }
            selected_id
        };
        self.refresh();
        if let Some(selected_id) = selected_id {
            let found = {
                let mut s = self.state.lock();
                match s.filtered_items.iter().position(|item| item.full_id == selected_id) {
                    Some(i) => {
                        s.selected_index = i;
                        true
                    }
                    None => false,
                }
            };
            if found {
                self.update_list();
            }
        }
    }

    pub fn set_refresh_status(&self, message: &str, kind: ScopedModelsSelectorComponentSetRefreshStatusKind) {
        if let Some(text) = &self.refresh_status_text {
            let color = match kind {
                ScopedModelsSelectorComponentSetRefreshStatusKind::Muted => ThemeColor::Muted,
                ScopedModelsSelectorComponentSetRefreshStatusKind::Success => ThemeColor::Success,
                ScopedModelsSelectorComponentSetRefreshStatusKind::Warning => ThemeColor::Warning,
            };
            text.set_text(theme().fg(&color, &format!("  {message}")));
        }
    }

    pub fn get_search_input(&self) -> Arc<Input> {
        self.search_input.clone()
    }

    fn refresh(&self) {
        let query = self.search_input.get_value();
        let footer = {
            let mut s = self.state.lock();
            let items = s.build_items();
            s.filtered_items = if query.is_empty() {
                items
            } else {
                fuzzy_filter(&items, &query, |item| match &item.model {
                    Some(model) => get_model_search_text(&ModelSearchItem {
                        id: model.id.clone(),
                        provider: model.provider.clone(),
                        name: Some(model.name.clone()),
                    }),
                    None => item.full_id.clone(),
                })
            };
            s.selected_index = s.selected_index.min(s.filtered_items.len().saturating_sub(1));
            s.footer_text()
        };
        self.update_list();
        self.footer_text.set_text(footer);
    }

    fn notify_change(&self) {
        let enabled = self.state.lock().enabled_ids.clone();
        fire_and_forget((self.callbacks.on_change)(enabled));
    }

    fn update_list(&self) {
        self.list_container.clear();
        let t = theme();
        let s = self.state.lock();

        if s.filtered_items.is_empty() {
            self.list_container
                .add_child(Arc::new(Text::new(t.fg(&ThemeColor::Muted, "  No matching models"), 0, 0, None)));
            return;
        }

        let len = s.filtered_items.len() as i64;
        let max_visible = self.max_visible as i64;
        let selected = s.selected_index as i64;
        let start_index = (selected - max_visible / 2).min(len - max_visible).max(0);
        let end_index = (start_index + max_visible).min(len);
        for i in start_index..end_index {
            let item = &s.filtered_items[i as usize];
            let is_selected = i == selected;
            let prefix = if is_selected { t.fg(&ThemeColor::Accent, "→ ") } else { "  ".to_string() };
            let id = item.model.as_ref().map(|m| m.id.clone()).unwrap_or_else(|| item.full_id.clone());
            let styled_id = if item.model.is_some() { id } else { t.strikethrough(&id) };
            let model_text = if is_selected { t.fg(&ThemeColor::Accent, &styled_id) } else { styled_id };
            let provider_badge = t.fg(
                &ThemeColor::Muted,
                &match &item.model {
                    Some(model) => format!(" [{}]", model.provider),
                    None => " [unavailable]".to_string(),
                },
            );
            let status =
                if item.model.is_some() && item.enabled { t.fg(&ThemeColor::Accent, "✓ ") } else { "  ".to_string() };
            self.list_container
                .add_child(Arc::new(Text::new(format!("{prefix}{status}{model_text}{provider_badge}"), 0, 0, None)));
        }

        // Add scroll indicator if needed
        if start_index > 0 || end_index < len {
            self.list_container.add_child(Arc::new(Text::new(
                t.fg(&ThemeColor::Muted, &format!("  ({}/{})", selected + 1, len)),
                0,
                0,
                None,
            )));
        }

        if let Some(selected) = s.filtered_items.get(s.selected_index) {
            self.list_container.add_child(Arc::new(Spacer::new(1)));
            let text = match &selected.model {
                Some(model) => format!("Model Name: {}", model.name),
                None => "Model unavailable".to_string(),
            };
            self.list_container
                .add_child(Arc::new(Text::new(t.fg(&ThemeColor::Muted, &format!("  {text}")), 0, 0, None)));
        }
    }

    fn filtered_target_ids(&self) -> Option<Vec<String>> {
        if self.search_input.get_value().is_empty() {
            None
        } else {
            Some(self.state.lock().filtered_items.iter().map(|i| i.full_id.clone()).collect())
        }
    }

    fn apply_change(&self, f: impl FnOnce(&mut State) -> bool) {
        let changed = {
            let mut s = self.state.lock();
            f(&mut s)
        };
        if changed {
            self.refresh();
            self.notify_change();
        }
    }
}

impl Component for ScopedModelsSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn handle_input(&self, data: &str) {
        let kb = get_keybindings();

        // Navigation
        if kb.matches(data, "tui.select.up") {
            {
                let mut s = self.state.lock();
                let len = s.filtered_items.len();
                if len == 0 {
                    return;
                }
                s.selected_index = if s.selected_index == 0 { len - 1 } else { s.selected_index - 1 };
            }
            self.update_list();
            return;
        }
        if kb.matches(data, "tui.select.down") {
            {
                let mut s = self.state.lock();
                let len = s.filtered_items.len();
                if len == 0 {
                    return;
                }
                s.selected_index = if s.selected_index == len - 1 { 0 } else { s.selected_index + 1 };
            }
            self.update_list();
            return;
        }

        // Reorder enabled models
        let reorder_up = kb.matches(data, "app.models.reorderUp");
        let reorder_down = kb.matches(data, "app.models.reorderDown");
        if reorder_up || reorder_down {
            self.apply_change(|s| {
                let Some(enabled) = &s.enabled_ids else {
                    return false;
                };
                let Some(item) = s.filtered_items.get(s.selected_index) else {
                    return false;
                };
                if !is_enabled(&s.enabled_ids, &item.full_id) {
                    return false;
                }
                let delta: i64 = if reorder_up { -1 } else { 1 };
                let Some(current_index) = enabled.iter().position(|x| *x == item.full_id) else {
                    return false;
                };
                let new_index = current_index as i64 + delta;
                // Only move if within bounds
                if new_index >= 0 && new_index < enabled.len() as i64 {
                    let full_id = item.full_id.clone();
                    s.enabled_ids = move_id(&s.enabled_ids, &full_id, delta);
                    s.is_dirty = true;
                    s.selected_index = (s.selected_index as i64 + delta).max(0) as usize;
                    true
                } else {
                    false
                }
            });
            return;
        }

        // Toggle on Enter
        if kb.matches(data, "tui.select.confirm") {
            self.apply_change(|s| {
                let Some(item) = s.filtered_items.get(s.selected_index) else {
                    return false;
                };
                let full_id = item.full_id.clone();
                s.enabled_ids = toggle(&s.enabled_ids, &s.all_ids, &full_id);
                s.is_dirty = true;
                true
            });
            return;
        }

        // Enable all (filtered if search active, otherwise all)
        if kb.matches(data, "app.models.enableAll") {
            let target_ids = self.filtered_target_ids();
            self.apply_change(|s| {
                s.enabled_ids = enable_all(&s.enabled_ids, &s.all_ids, target_ids.as_deref());
                s.is_dirty = true;
                true
            });
            return;
        }

        // Clear all (filtered if search active, otherwise all)
        if kb.matches(data, "app.models.clearAll") {
            let target_ids = self.filtered_target_ids();
            self.apply_change(|s| {
                s.enabled_ids = clear_all(&s.enabled_ids, &s.all_ids, target_ids.as_deref());
                s.is_dirty = true;
                true
            });
            return;
        }

        // Toggle provider of current item
        if kb.matches(data, "app.models.toggleProvider") {
            self.apply_change(|s| {
                let Some(provider) =
                    s.filtered_items.get(s.selected_index).and_then(|i| i.model.as_ref()).map(|m| m.provider.clone())
                else {
                    return false;
                };
                let provider_ids: Vec<String> = s
                    .all_ids
                    .iter()
                    .filter(|id| s.models_by_id.get(*id).is_some_and(|m| m.provider == provider))
                    .cloned()
                    .collect();
                let all_enabled = provider_ids.iter().all(|id| is_enabled(&s.enabled_ids, id));
                s.enabled_ids = if all_enabled {
                    clear_all(&s.enabled_ids, &s.all_ids, Some(&provider_ids))
                } else {
                    enable_all(&s.enabled_ids, &s.all_ids, Some(&provider_ids))
                };
                s.is_dirty = true;
                true
            });
            return;
        }

        // Save/persist to settings
        if kb.matches(data, "app.models.save") {
            let enabled = self.state.lock().enabled_ids.clone();
            fire_and_forget((self.callbacks.on_persist)(enabled));
            let footer = {
                let mut s = self.state.lock();
                s.is_dirty = false;
                s.footer_text()
            };
            self.footer_text.set_text(footer);
            return;
        }

        // Ctrl+C - clear search or cancel if empty
        if matches_key(data, &Key::ctrl("c")) {
            if !self.search_input.get_value().is_empty() {
                self.search_input.set_value("");
                self.refresh();
            } else {
                (self.callbacks.on_cancel)();
            }
            return;
        }

        // Escape - cancel
        if matches_key(data, Key::escape) {
            (self.callbacks.on_cancel)();
            return;
        }

        // Pass everything else to search input
        self.search_input.handle_input(data);
        self.refresh();
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
        self.state.lock().focused = focused;
        self.search_input.set_focused(focused);
    }

    fn focused(&self) -> bool {
        self.state.lock().focused
    }
}
