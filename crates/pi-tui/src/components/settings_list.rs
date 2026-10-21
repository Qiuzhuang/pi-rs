//! A list of settings with cycling values and submenus. Port of `components/settings-list.ts`.

use std::sync::{Arc, Mutex, MutexGuard, Weak};

use crate::StyleFn;
use crate::components::input::Input;
use crate::fuzzy::fuzzy_filter;
use crate::keybindings::get_keybindings;
use crate::tui::{Component, ComponentRef, MouseButton, MouseEvent, MouseEventType, MouseResult};
use crate::utils::{truncate_to_width, visible_width, wrap_text_with_ansi};

/// Closes a submenu: `(selected_value, navigate_to)`.
pub type SubmenuDone = Arc<dyn Fn(Option<String>, Option<String>) + Send + Sync>;
/// Builds a submenu component from the current value and a done callback.
pub type SubmenuFactory = Arc<dyn Fn(String, SubmenuDone) -> ComponentRef + Send + Sync>;

#[derive(Clone, Default)]
pub struct SettingItem {
    /// Unique identifier.
    pub id: String,
    /// Display label (left side).
    pub label: String,
    /// Optional description shown when selected.
    pub description: Option<String>,
    /// Current value (right side).
    pub current_value: String,
    /// If provided, Enter/Space cycles through these values.
    pub values: Option<Vec<String>>,
    /// If provided, Enter opens this submenu.
    pub submenu: Option<SubmenuFactory>,
}

/// Selected-aware styling function.
pub type SelectedStyleFn = Arc<dyn Fn(&str, bool) -> String + Send + Sync>;

#[derive(Clone)]
pub struct SettingsListTheme {
    pub label: SelectedStyleFn,
    pub value: SelectedStyleFn,
    pub description: StyleFn,
    pub cursor: String,
    pub hint: StyleFn,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SettingsListOptions {
    pub enable_search: bool,
}

pub type ChangeFn = Arc<dyn Fn(String, String) + Send + Sync>;
pub type CancelFn = Arc<dyn Fn() + Send + Sync>;

struct State {
    items: Vec<SettingItem>,
    filtered: Vec<usize>,
    selected_index: i64,
    mouse_pressed_index: Option<i64>,
    submenu: Option<ComponentRef>,
    submenu_item_index: Option<i64>,
    navigate_after_close: Option<String>,
}

/// A list of settings.
pub struct SettingsList {
    weak_self: Weak<SettingsList>,
    st: Mutex<State>,
    max_visible: usize,
    theme: SettingsListTheme,
    on_change: ChangeFn,
    on_cancel: CancelFn,
    search_input: Option<Input>,
    search_enabled: bool,
}

impl SettingsList {
    pub fn new(
        items: Vec<SettingItem>,
        max_visible: usize,
        theme: SettingsListTheme,
        on_change: ChangeFn,
        on_cancel: CancelFn,
        options: SettingsListOptions,
    ) -> Arc<Self> {
        let filtered = (0..items.len()).collect();
        Arc::new_cyclic(|weak| SettingsList {
            weak_self: weak.clone(),
            st: Mutex::new(State {
                items,
                filtered,
                selected_index: 0,
                mouse_pressed_index: None,
                submenu: None,
                submenu_item_index: None,
                navigate_after_close: None,
            }),
            max_visible,
            theme,
            on_change,
            on_cancel,
            search_input: if options.enable_search { Some(Input::default()) } else { None },
            search_enabled: options.enable_search,
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Update an item's current value.
    pub fn update_value(&self, id: &str, new_value: &str) {
        let mut s = self.lock();
        if let Some(item) = s.items.iter_mut().find(|i| i.id == id) {
            item.current_value = new_value.to_string();
        }
    }

    /// Move selection to the item with the given id.
    pub fn select_item(&self, id: &str) {
        let mut s = self.lock();
        let display = self.display_indices(&s);
        if let Some(index) = display.iter().position(|&i| s.items[i].id == id) {
            s.selected_index = index as i64;
        }
    }

    fn display_indices(&self, s: &State) -> Vec<usize> {
        if self.search_enabled { s.filtered.clone() } else { (0..s.items.len()).collect() }
    }

    fn visible_range(&self, s: &State, len: usize) -> (i64, i64) {
        let len = len as i64;
        let mv = self.max_visible as i64;
        let start = (s.selected_index - mv / 2).min(len - mv).max(0);
        (start, (start + mv).min(len))
    }

    fn add_hint_line(&self, lines: &mut Vec<String>, width: usize) {
        lines.push(String::new());
        let hint = if self.search_enabled {
            "  Type to search · Enter/Space to change · Esc to cancel"
        } else {
            "  Enter/Space to change · Esc to cancel"
        };
        lines.push(truncate_to_width(&(self.theme.hint)(hint), width, "...", false));
    }

    fn render_main_list(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(input) = &self.search_input {
            lines.extend(input.render(width));
            lines.push(String::new());
        }
        let s = self.lock();
        if s.items.is_empty() {
            lines.push((self.theme.hint)("  No settings available"));
            if self.search_enabled {
                self.add_hint_line(&mut lines, width);
            }
            return lines;
        }
        let display = self.display_indices(&s);
        if display.is_empty() {
            lines.push(truncate_to_width(&(self.theme.hint)("  No matching settings"), width, "...", false));
            self.add_hint_line(&mut lines, width);
            return lines;
        }
        let (start, end) = self.visible_range(&s, display.len());
        let max_label_width = s.items.iter().map(|i| visible_width(&i.label)).max().unwrap_or(0).min(36);
        for i in start..end {
            let Some(&idx) = display.get(i as usize) else { continue };
            let item = &s.items[idx];
            let is_selected = i == s.selected_index;
            let prefix = if is_selected { self.theme.cursor.as_str() } else { "  " };
            let prefix_width = visible_width(prefix);
            let label_padded = format!("{}{}", item.label, " ".repeat(max_label_width.saturating_sub(visible_width(&item.label))));
            let label_text = (self.theme.label)(&label_padded, is_selected);
            let separator = "  ";
            let used = prefix_width + max_label_width + visible_width(separator);
            let value_max = width.saturating_sub(used + 2);
            let value_text = (self.theme.value)(&truncate_to_width(&item.current_value, value_max, "", false), is_selected);
            lines.push(truncate_to_width(&format!("{prefix}{label_text}{separator}{value_text}"), width, "...", false));
        }
        if start > 0 || end < display.len() as i64 {
            let scroll_text = format!("  ({}/{})", s.selected_index + 1, display.len());
            lines.push((self.theme.hint)(&truncate_to_width(&scroll_text, width.saturating_sub(2), "", false)));
        }
        if let Some(desc) = usize::try_from(s.selected_index)
            .ok()
            .and_then(|i| display.get(i))
            .and_then(|&idx| s.items[idx].description.clone())
            .filter(|d| !d.is_empty())
        {
            lines.push(String::new());
            for line in wrap_text_with_ansi(&desc, width.saturating_sub(4)) {
                lines.push((self.theme.description)(&format!("  {line}")));
            }
        }
        drop(s);
        self.add_hint_line(&mut lines, width);
        lines
    }

    fn activate_item(&self) {
        let (item, idx, selected) = {
            let s = self.lock();
            let display = self.display_indices(&s);
            let Some(&idx) = usize::try_from(s.selected_index).ok().and_then(|i| display.get(i)) else {
                return;
            };
            (s.items[idx].clone(), idx, s.selected_index)
        };
        if let Some(factory) = &item.submenu {
            self.lock().submenu_item_index = Some(selected);
            let weak = self.weak_self.clone();
            let id = item.id.clone();
            let done: SubmenuDone = Arc::new(move |selected_value: Option<String>, navigate_to: Option<String>| {
                let Some(me) = weak.upgrade() else { return };
                if let Some(v) = selected_value {
                    {
                        let mut s = me.lock();
                        if let Some(it) = s.items.get_mut(idx) {
                            it.current_value = v.clone();
                        }
                    }
                    (me.on_change)(id.clone(), v);
                }
                if let Some(nav) = navigate_to.filter(|n| !n.is_empty()) {
                    me.lock().navigate_after_close = Some(nav);
                }
                me.close_submenu();
            });
            let component = factory(item.current_value.clone(), done);
            self.lock().submenu = Some(component);
        } else if let Some(values) = item.values.as_ref().filter(|v| !v.is_empty()) {
            let current = values.iter().position(|v| *v == item.current_value).map_or(-1, |i| i as i64);
            let next = ((current + 1) as usize) % values.len();
            let new_value = values[next].clone();
            {
                let mut s = self.lock();
                s.items[idx].current_value = new_value.clone();
            }
            (self.on_change)(item.id.clone(), new_value);
        }
    }

    fn close_submenu(&self) {
        let nav = {
            let mut s = self.lock();
            s.submenu = None;
            match s.navigate_after_close.take() {
                Some(id) => {
                    s.submenu_item_index = None;
                    Some(id)
                }
                None => {
                    if let Some(i) = s.submenu_item_index.take() {
                        s.selected_index = i;
                    }
                    None
                }
            }
        };
        if let Some(id) = nav {
            self.select_item(&id);
            self.activate_item();
        }
    }

    fn apply_filter(&self, query: &str) {
        let mut s = self.lock();
        let indices: Vec<usize> = (0..s.items.len()).collect();
        let items = s.items.clone();
        s.filtered = fuzzy_filter(&indices, query, |i| items[*i].label.clone());
        s.selected_index = 0;
    }
}

impl Component for SettingsList {
    fn render(&self, width: usize) -> Vec<String> {
        let submenu = self.lock().submenu.clone();
        match submenu {
            Some(c) => c.render(width),
            None => self.render_main_list(width),
        }
    }

    fn invalidate(&self) {
        let submenu = self.lock().submenu.clone();
        if let Some(c) = submenu {
            c.invalidate();
        }
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        let submenu = self.lock().submenu.clone();
        if let Some(c) = submenu {
            let mut r = c.handle_mouse(event)?;
            r.focus = true;
            return Some(r);
        }
        if let Some(input) = &self.search_input {
            if event.y == 0 {
                let mut r = input.handle_mouse(event)?;
                r.focus = true;
                return Some(r);
            }
            if event.y == 1 {
                return None;
            }
        }
        let mut s = self.lock();
        let display = self.display_indices(&s);
        if display.is_empty() {
            return None;
        }
        if event.kind == MouseEventType::Wheel {
            if let Some(delta) = event.wheel_delta.filter(|d| *d != 0) {
                let d = if delta < 0 { -1 } else { 1 };
                let prev = s.selected_index;
                s.selected_index = (s.selected_index + d).clamp(0, display.len() as i64 - 1);
                return Some(MouseResult { handled: true, render: Some(s.selected_index != prev), ..Default::default() });
            }
        }
        if event.button != MouseButton::Left || (event.kind != MouseEventType::Press && event.kind != MouseEventType::Click) {
            return None;
        }
        let row_offset = if self.search_enabled { 2 } else { 0 };
        let (start, end) = self.visible_range(&s, display.len());
        let item_index = start + event.y - row_offset;
        if item_index < start || item_index >= end {
            return None;
        }
        if event.kind == MouseEventType::Press {
            s.mouse_pressed_index = Some(item_index);
            s.selected_index = item_index;
            return Some(MouseResult { handled: true, focus: true, ..Default::default() });
        }
        s.selected_index = s.mouse_pressed_index.take().unwrap_or(item_index);
        drop(s);
        self.activate_item();
        Some(MouseResult::handled())
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn handle_input(&self, data: &str) {
        let submenu = self.lock().submenu.clone();
        if let Some(c) = submenu {
            c.handle_input(data);
            return;
        }
        let kb = get_keybindings();
        let len = {
            let s = self.lock();
            self.display_indices(&s).len() as i64
        };
        if kb.matches(data, "tui.select.up") {
            if len == 0 {
                return;
            }
            let mut s = self.lock();
            s.selected_index = if s.selected_index == 0 { len - 1 } else { s.selected_index - 1 };
        } else if kb.matches(data, "tui.select.down") {
            if len == 0 {
                return;
            }
            let mut s = self.lock();
            s.selected_index = if s.selected_index == len - 1 { 0 } else { s.selected_index + 1 };
        } else if kb.matches(data, "tui.select.confirm")
            || (data == " " && (!self.search_enabled || self.search_input.as_ref().is_some_and(|i| i.get_value().is_empty())))
        {
            self.activate_item();
        } else if kb.matches(data, "tui.select.cancel") {
            (self.on_cancel)();
        } else if let Some(input) = &self.search_input {
            input.handle_input(data);
            self.apply_filter(&input.get_value());
        }
    }
}
