//! A container applying padding and background to its children. Port of `components/box.ts`.

use std::sync::{Mutex, MutexGuard};

use crate::StyleFn;
use crate::tui::{Component, ComponentRef, MouseEvent, MouseResult, dispatch_mouse_event, same_component};
use crate::utils::visible_width;

struct Cache {
    child_lines: Vec<String>,
    width: usize,
    bg_sample: Option<String>,
    lines: Vec<String>,
}

struct State {
    children: Vec<ComponentRef>,
    padding_x: usize,
    padding_y: usize,
    bg_fn: Option<StyleFn>,
    cache: Option<Cache>,
    mouse_layout: Option<(usize, Vec<(ComponentRef, usize)>)>,
}

/// Box component - applies padding and an optional background to all children.
pub struct BoxComponent {
    st: Mutex<State>,
}

impl Default for BoxComponent {
    fn default() -> Self {
        Self::new(1, 1, None)
    }
}

impl BoxComponent {
    pub fn new(padding_x: usize, padding_y: usize, bg_fn: Option<StyleFn>) -> Self {
        Self {
            st: Mutex::new(State {
                children: Vec::new(),
                padding_x,
                padding_y,
                bg_fn,
                cache: None,
                mouse_layout: None,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn add_child(&self, component: ComponentRef) {
        let mut s = self.lock();
        s.children.push(component);
        s.cache = None;
    }

    pub fn remove_child(&self, component: &ComponentRef) {
        let mut s = self.lock();
        if let Some(i) = s.children.iter().position(|c| same_component(c, component)) {
            s.children.remove(i);
            s.cache = None;
        }
    }

    pub fn clear(&self) {
        let mut s = self.lock();
        s.children.clear();
        s.cache = None;
    }

    pub fn children(&self) -> Vec<ComponentRef> {
        self.lock().children.clone()
    }

    /// Change the background; detected on the next render by sampling its output.
    pub fn set_bg_fn(&self, bg_fn: Option<StyleFn>) {
        self.lock().bg_fn = bg_fn;
    }

    pub fn set_padding_x(&self, padding_x: usize) {
        let mut s = self.lock();
        s.padding_x = padding_x;
        s.cache = None;
    }

    fn apply_bg(bg: &Option<StyleFn>, line: &str, width: usize) -> String {
        let pad = width.saturating_sub(visible_width(line));
        let padded = format!("{line}{}", " ".repeat(pad));
        match bg {
            Some(f) => f(&padded),
            None => padded,
        }
    }
}

impl Component for BoxComponent {
    fn render(&self, width: usize) -> Vec<String> {
        let (children, padding_x, padding_y, bg_fn) = {
            let s = self.lock();
            (s.children.clone(), s.padding_x, s.padding_y, s.bg_fn.clone())
        };
        if children.is_empty() {
            return Vec::new();
        }
        let content_width = width.saturating_sub(padding_x * 2).max(1);
        let left_pad = " ".repeat(padding_x);
        let mut child_lines = Vec::new();
        let mut layout = Vec::new();
        for child in &children {
            let lines = child.render(content_width);
            layout.push((child.clone(), lines.len()));
            child_lines.extend(lines);
        }
        let mut s = self.lock();
        s.mouse_layout = Some((content_width, layout));
        if child_lines.is_empty() {
            return Vec::new();
        }
        let bg_sample = bg_fn.as_ref().map(|f| f("test"));
        if let Some(cache) = &s.cache
            && cache.width == width && cache.bg_sample == bg_sample && cache.child_lines == child_lines
        {
            return cache.lines.clone();
        }
        let mut result = Vec::new();
        for _ in 0..padding_y {
            result.push(Self::apply_bg(&bg_fn, "", width));
        }
        for line in &child_lines {
            result.push(Self::apply_bg(&bg_fn, &format!("{left_pad}{line}"), width));
        }
        for _ in 0..padding_y {
            result.push(Self::apply_bg(&bg_fn, "", width));
        }
        s.cache = Some(Cache { child_lines, width, bg_sample, lines: result.clone() });
        result
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        let (padding_x, padding_y, children, layout) = {
            let s = self.lock();
            (s.padding_x as i64, s.padding_y as i64, s.children.clone(), s.mouse_layout.clone())
        };
        let content_width = (event.width - padding_x * 2).max(1);
        let content_y = event.y - padding_y;
        let content_x = event.x - padding_x;
        if content_y < 0 || content_x < 0 || content_x >= content_width {
            return None;
        }
        let mouse_children = match layout {
            Some((w, l)) if w as i64 == content_width => l,
            _ => children.into_iter().map(|c| { let h = c.render(content_width as usize).len(); (c, h) }).collect(),
        };
        let mut child_y = 0i64;
        for (child, h) in mouse_children {
            let h = h as i64;
            if content_y >= child_y && content_y < child_y + h {
                return dispatch_mouse_event(
                    &child,
                    &MouseEvent { x: content_x, y: content_y - child_y, width: content_width, height: h, ..*event },
                );
            }
            child_y += h;
        }
        None
    }

    fn invalidate(&self) {
        let children = {
            let mut s = self.lock();
            s.cache = None;
            s.children.clone()
        };
        for c in children {
            c.invalidate();
        }
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        None
    }
}
