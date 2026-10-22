//! Scrollable selection list. Port of `components/select-list.ts`.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::StyleFn;
use crate::keybindings::get_keybindings;
use crate::tui::{Component, MouseButton, MouseEvent, MouseEventType, MouseResult};
use crate::utils::{js_trim, truncate_to_width, visible_width};

const DEFAULT_PRIMARY_COLUMN_WIDTH: usize = 32;
const PRIMARY_COLUMN_GAP: usize = 2;
const MIN_DESCRIPTION_WIDTH: i64 = 10;

fn normalize_to_single_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_break = false;
    for c in text.chars() {
        if c == '\r' || c == '\n' {
            if !in_break {
                out.push(' ');
            }
            in_break = true;
        } else {
            out.push(c);
            in_break = false;
        }
    }
    js_trim(&out).to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectItem {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

impl SelectItem {
    pub fn new(value: impl Into<String>, label: impl Into<String>, description: Option<String>) -> Self {
        Self { value: value.into(), label: label.into(), description }
    }
}

/// Theme for [`SelectList`].
#[derive(Clone)]
pub struct SelectListTheme {
    pub selected_prefix: StyleFn,
    pub selected_text: StyleFn,
    pub description: StyleFn,
    pub scroll_info: StyleFn,
    pub no_match: StyleFn,
}

impl SelectListTheme {
    /// A theme that applies no styling.
    pub fn plain() -> Self {
        let id: StyleFn = Arc::new(|t: &str| t.to_string());
        Self {
            selected_prefix: id.clone(),
            selected_text: id.clone(),
            description: id.clone(),
            scroll_info: id.clone(),
            no_match: id,
        }
    }
}

/// Context passed to a custom primary-column truncation function.
pub struct SelectListTruncatePrimaryContext<'a> {
    pub text: &'a str,
    pub max_width: usize,
    pub column_width: usize,
    pub item: &'a SelectItem,
    pub is_selected: bool,
}

pub type TruncatePrimaryFn = Arc<dyn Fn(&SelectListTruncatePrimaryContext) -> String + Send + Sync>;

#[derive(Clone, Default)]
pub struct SelectListLayoutOptions {
    pub min_primary_column_width: Option<usize>,
    pub max_primary_column_width: Option<usize>,
    pub truncate_primary: Option<TruncatePrimaryFn>,
}

pub type SelectFn = Arc<dyn Fn(SelectItem) + Send + Sync>;
pub type CancelFn = Arc<dyn Fn() + Send + Sync>;

struct State {
    items: Vec<SelectItem>,
    filtered: Vec<SelectItem>,
    selected_index: i64,
    mouse_pressed_index: Option<i64>,
}

#[derive(Default)]
struct Callbacks {
    on_select: Option<SelectFn>,
    on_cancel: Option<CancelFn>,
    on_selection_change: Option<SelectFn>,
}

/// A scrollable list of selectable items.
pub struct SelectList {
    st: Mutex<State>,
    max_visible: usize,
    theme: SelectListTheme,
    layout: SelectListLayoutOptions,
    callbacks: Mutex<Callbacks>,
}

impl SelectList {
    pub fn new(items: Vec<SelectItem>, max_visible: usize, theme: SelectListTheme, layout: SelectListLayoutOptions) -> Self {
        Self {
            st: Mutex::new(State { filtered: items.clone(), items, selected_index: 0, mouse_pressed_index: None }),
            max_visible,
            theme,
            layout,
            callbacks: Mutex::new(Callbacks::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cbs(&self) -> MutexGuard<'_, Callbacks> {
        self.callbacks.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_on_select(&self, f: Option<SelectFn>) {
        self.cbs().on_select = f;
    }

    pub fn set_on_cancel(&self, f: Option<CancelFn>) {
        self.cbs().on_cancel = f;
    }

    pub fn set_on_selection_change(&self, f: Option<SelectFn>) {
        self.cbs().on_selection_change = f;
    }

    /// Filter items whose value starts with `filter` (case-insensitive); resets selection.
    pub fn set_filter(&self, filter: &str) {
        let mut s = self.lock();
        let f = filter.to_lowercase();
        s.filtered = s.items.iter().filter(|i| i.value.to_lowercase().starts_with(&f)).cloned().collect();
        s.selected_index = 0;
    }

    pub fn set_selected_index(&self, index: usize) {
        let mut s = self.lock();
        let max = s.filtered.len() as i64 - 1;
        s.selected_index = (index as i64).min(max).max(0);
    }

    pub fn selected_index(&self) -> usize {
        self.lock().selected_index.max(0) as usize
    }

    pub fn get_selected_item(&self) -> Option<SelectItem> {
        let s = self.lock();
        usize::try_from(s.selected_index).ok().and_then(|i| s.filtered.get(i).cloned())
    }

    fn visible_range(&self, s: &State) -> (i64, i64) {
        let len = s.filtered.len() as i64;
        let mv = self.max_visible as i64;
        let start = (s.selected_index - mv / 2).min(len - mv).max(0);
        (start, (start + mv).min(len))
    }

    fn display_value(item: &SelectItem) -> &str {
        if item.label.is_empty() { &item.value } else { &item.label }
    }

    fn primary_column_bounds(&self) -> (usize, usize) {
        let raw_min = self
            .layout
            .min_primary_column_width
            .or(self.layout.max_primary_column_width)
            .unwrap_or(DEFAULT_PRIMARY_COLUMN_WIDTH);
        let raw_max = self
            .layout
            .max_primary_column_width
            .or(self.layout.min_primary_column_width)
            .unwrap_or(DEFAULT_PRIMARY_COLUMN_WIDTH);
        (raw_min.min(raw_max).max(1), raw_min.max(raw_max).max(1))
    }

    fn primary_column_width(&self, s: &State) -> usize {
        let (min, max) = self.primary_column_bounds();
        let widest = s.filtered.iter().map(|i| visible_width(Self::display_value(i)) + PRIMARY_COLUMN_GAP).max().unwrap_or(0);
        widest.clamp(min, max)
    }

    fn truncate_primary(&self, item: &SelectItem, is_selected: bool, max_width: usize, column_width: usize) -> String {
        let display = Self::display_value(item);
        let truncated = match &self.layout.truncate_primary {
            Some(f) => f(&SelectListTruncatePrimaryContext { text: display, max_width, column_width, item, is_selected }),
            None => truncate_to_width(display, max_width, "", false),
        };
        truncate_to_width(&truncated, max_width, "", false)
    }

    fn render_item(&self, item: &SelectItem, is_selected: bool, width: usize, desc: Option<&str>, primary_width: usize) -> String {
        let prefix = if is_selected { "→ " } else { "  " };
        let prefix_width = visible_width(prefix);
        if let Some(desc) = desc.filter(|d| !d.is_empty())
            && width > 40
        {
            let effective = primary_width.min(width.saturating_sub(prefix_width + 4)).max(1);
            let max_primary = effective.saturating_sub(PRIMARY_COLUMN_GAP).max(1);
            let value = self.truncate_primary(item, is_selected, max_primary, effective);
            let value_width = visible_width(&value);
            let spacing = " ".repeat(effective.saturating_sub(value_width).max(1));
            let description_start = prefix_width + value_width + spacing.len();
            let remaining = width as i64 - description_start as i64 - 2;
            if remaining > MIN_DESCRIPTION_WIDTH {
                let truncated_desc = truncate_to_width(desc, remaining as usize, "", false);
                if is_selected {
                    return (self.theme.selected_text)(&format!("{prefix}{value}{spacing}{truncated_desc}"));
                }
                let desc_text = (self.theme.description)(&format!("{spacing}{truncated_desc}"));
                return format!("{prefix}{value}{desc_text}");
            }
        }
        let max_width = width.saturating_sub(prefix_width + 2);
        let value = self.truncate_primary(item, is_selected, max_width, max_width);
        if is_selected {
            return (self.theme.selected_text)(&format!("{prefix}{value}"));
        }
        format!("{prefix}{value}")
    }

    fn notify_selection_change(&self) {
        let item = self.get_selected_item();
        let cb = self.cbs().on_selection_change.clone();
        if let (Some(item), Some(cb)) = (item, cb) {
            cb(item);
        }
    }
}

impl Component for SelectList {
    fn render(&self, width: usize) -> Vec<String> {
        let s = self.lock();
        let mut lines = Vec::new();
        if s.filtered.is_empty() {
            lines.push((self.theme.no_match)("  No matching commands"));
            return lines;
        }
        let primary_width = self.primary_column_width(&s);
        let (start, end) = self.visible_range(&s);
        for i in start..end {
            let Some(item) = s.filtered.get(i as usize) else { continue };
            let is_selected = i == s.selected_index;
            let desc = item.description.as_deref().filter(|d| !d.is_empty()).map(normalize_to_single_line);
            lines.push(self.render_item(item, is_selected, width, desc.as_deref(), primary_width));
        }
        if start > 0 || end < s.filtered.len() as i64 {
            let scroll_text = format!("  ({}/{})", s.selected_index + 1, s.filtered.len());
            lines.push((self.theme.scroll_info)(&truncate_to_width(&scroll_text, width.saturating_sub(2), "", false)));
        }
        lines
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        let mut s = self.lock();
        if s.filtered.is_empty() {
            return None;
        }
        if event.kind == MouseEventType::Wheel
            && let Some(delta) = event.wheel_delta.filter(|d| *d != 0)
        {
            let d = if delta < 0 { -1 } else { 1 };
            let prev = s.selected_index;
            s.selected_index = (s.selected_index + d).clamp(0, s.filtered.len() as i64 - 1);
            let changed = s.selected_index != prev;
            drop(s);
            if changed {
                self.notify_selection_change();
            }
            return Some(MouseResult { handled: true, render: Some(changed), ..Default::default() });
        }
        if event.button != MouseButton::Left || (event.kind != MouseEventType::Press && event.kind != MouseEventType::Click) {
            return None;
        }
        let (start, end) = self.visible_range(&s);
        let item_index = start + event.y;
        if item_index < start || item_index >= end {
            return None;
        }
        if event.kind == MouseEventType::Press {
            s.mouse_pressed_index = Some(item_index);
            let changed = s.selected_index != item_index;
            s.selected_index = item_index;
            drop(s);
            if changed {
                self.notify_selection_change();
            }
            return Some(MouseResult { handled: true, focus: true, ..Default::default() });
        }
        let clicked = s.mouse_pressed_index.take().unwrap_or(item_index);
        let changed = s.selected_index != clicked;
        s.selected_index = clicked;
        drop(s);
        if changed {
            self.notify_selection_change();
        }
        let item = self.get_selected_item();
        let cb = self.cbs().on_select.clone();
        if let (Some(item), Some(cb)) = (item, cb) {
            cb(item);
        }
        Some(MouseResult::handled())
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn handle_input(&self, data: &str) {
        let kb = get_keybindings();
        if kb.matches(data, "tui.select.up") {
            {
                let mut s = self.lock();
                let len = s.filtered.len() as i64;
                s.selected_index = if s.selected_index == 0 { len - 1 } else { s.selected_index - 1 };
            }
            self.notify_selection_change();
        } else if kb.matches(data, "tui.select.down") {
            {
                let mut s = self.lock();
                let len = s.filtered.len() as i64;
                s.selected_index = if s.selected_index == len - 1 { 0 } else { s.selected_index + 1 };
            }
            self.notify_selection_change();
        } else if kb.matches(data, "tui.select.confirm") {
            let item = self.get_selected_item();
            let cb = self.cbs().on_select.clone();
            if let (Some(item), Some(cb)) = (item, cb) {
                cb(item);
            }
        } else if kb.matches(data, "tui.select.cancel") {
            let cb = self.cbs().on_cancel.clone();
            if let Some(cb) = cb {
                cb();
            }
        }
    }
}
