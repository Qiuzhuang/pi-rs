//! Vertical and horizontal stacks with flex-like sizing. Port of `components/stack.ts`,
//! `components/v-stack.ts` and `components/h-stack.ts`.

use std::sync::{Mutex, MutexGuard};

use crate::layout_node::{
    LayoutNode, LayoutViewport, StackAlign, StackBasis, StackLayoutEntry, StackLayoutNode, StackType, StackVisibleFn,
};
use crate::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult, composite_tui_line, same_component};
use crate::utils::visible_width;

/// Per-entry stack options.
#[derive(Clone, Default)]
pub struct StackEntryOptions {
    pub basis: Option<StackBasis>,
    pub grow: Option<usize>,
    pub shrink: Option<usize>,
    pub min_size: Option<usize>,
    pub max_size: Option<usize>,
    pub visible: Option<StackVisibleFn>,
}

/// A stack child: a component with options.
#[derive(Clone)]
pub struct StackChild {
    pub component: ComponentRef,
    pub options: StackEntryOptions,
}

impl From<ComponentRef> for StackChild {
    fn from(component: ComponentRef) -> Self {
        StackChild { component, options: StackEntryOptions::default() }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StackOptions {
    pub gap: Option<usize>,
    pub align: Option<StackAlign>,
}

pub fn visible_stack_entries(entries: &[StackLayoutEntry], viewport: LayoutViewport) -> Vec<StackLayoutEntry> {
    entries.iter().filter(|e| e.visible.as_ref().is_none_or(|v| v(viewport))).cloned().collect()
}

const MAX_SAFE: usize = 9007199254740991;

fn clamp_size(size: usize, entry: &StackLayoutEntry) -> usize {
    let min = entry.min_size.unwrap_or(0);
    let max = entry.max_size.unwrap_or(MAX_SAFE).max(min);
    size.clamp(min, max)
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Mode {
    Grow,
    Shrink,
}

fn distribute(sizes: &mut [usize], entries: &[StackLayoutEntry], amount: usize, mode: Mode) {
    let mut remaining = amount;
    while remaining > 0 {
        let candidates: Vec<usize> = (0..entries.len())
            .filter(|&i| {
                let e = &entries[i];
                match mode {
                    Mode::Grow => e.grow.unwrap_or(0) > 0 && sizes[i] < e.max_size.unwrap_or(MAX_SAFE),
                    Mode::Shrink => e.shrink.unwrap_or(1) > 0 && sizes[i] > e.min_size.unwrap_or(0),
                }
            })
            .collect();
        if candidates.is_empty() {
            return;
        }
        let weight = |sizes: &[usize], i: usize| -> f64 {
            let e = &entries[i];
            match mode {
                Mode::Grow => e.grow.unwrap_or(0) as f64,
                Mode::Shrink => e.shrink.unwrap_or(1) as f64 * sizes[i].max(1) as f64,
            }
        };
        let total_weight: f64 = candidates.iter().map(|&i| weight(sizes, i)).sum();
        let mut distributed = 0;
        for &i in &candidates {
            if remaining == 0 {
                break;
            }
            let w = weight(sizes, i);
            let proposed = (((remaining as f64) * w) / total_weight).floor().max(1.0) as usize;
            let e = &entries[i];
            let capacity = match mode {
                Mode::Grow => e.max_size.unwrap_or(MAX_SAFE).saturating_sub(sizes[i]),
                Mode::Shrink => sizes[i].saturating_sub(e.min_size.unwrap_or(0)),
            };
            let delta = remaining.min(proposed).min(capacity);
            if delta == 0 {
                continue;
            }
            match mode {
                Mode::Grow => sizes[i] += delta,
                Mode::Shrink => sizes[i] -= delta,
            }
            remaining -= delta;
            distributed += delta;
        }
        if distributed == 0 {
            return;
        }
    }
}

/// Allocate main-axis sizes for stack entries.
pub fn allocate_stack_sizes(
    entries: &[StackLayoutEntry],
    intrinsic_sizes: &[usize],
    available_size: Option<usize>,
    gap: usize,
) -> Vec<usize> {
    let mut sizes: Vec<usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let base = match e.basis {
                None | Some(StackBasis::Auto) => intrinsic_sizes.get(i).copied().unwrap_or(0),
                Some(StackBasis::Size(s)) => s,
            };
            clamp_size(base, e)
        })
        .collect();
    let Some(available) = available_size else { return sizes };
    let content = available.saturating_sub(entries.len().saturating_sub(1) * gap);
    let total: usize = sizes.iter().sum();
    if total < content {
        distribute(&mut sizes, entries, content - total, Mode::Grow);
    } else if total > content {
        distribute(&mut sizes, entries, total - content, Mode::Shrink);
    }
    sizes
}

/// Shared implementation of [`VStack`] and [`HStack`].
pub struct Stack {
    container: Container,
    entries: Mutex<Vec<StackLayoutEntry>>,
    gap: usize,
    align: StackAlign,
    kind: StackType,
}

impl Stack {
    fn new(kind: StackType, children: Vec<StackChild>, options: StackOptions) -> Self {
        let stack = Stack {
            container: Container::new(),
            entries: Mutex::new(Vec::new()),
            gap: options.gap.unwrap_or(0),
            align: options.align.unwrap_or_default(),
            kind,
        };
        for child in children {
            stack.add_child_with(child.component, child.options);
        }
        stack
    }

    fn lock(&self) -> MutexGuard<'_, Vec<StackLayoutEntry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn add_child(&self, component: ComponentRef) {
        self.add_child_with(component, StackEntryOptions::default());
    }

    pub fn add_child_with(&self, component: ComponentRef, options: StackEntryOptions) {
        self.container.add_child(component.clone());
        self.lock().push(StackLayoutEntry {
            component,
            basis: options.basis,
            grow: options.grow,
            shrink: options.shrink,
            min_size: options.min_size,
            max_size: options.max_size,
            visible: options.visible,
        });
    }

    pub fn remove_child(&self, component: &ComponentRef) {
        self.container.remove_child(component);
        let mut entries = self.lock();
        if let Some(i) = entries.iter().position(|e| same_component(&e.component, component)) {
            entries.remove(i);
        }
    }

    pub fn clear(&self) {
        self.container.clear();
        self.lock().clear();
    }

    pub fn children(&self) -> Vec<ComponentRef> {
        self.container.children()
    }

    fn node(&self) -> StackLayoutNode {
        StackLayoutNode { kind: self.kind, entries: self.lock().clone(), gap: self.gap, align: self.align }
    }

    fn render_vstack(&self, width: usize) -> Vec<String> {
        let viewport = LayoutViewport { width: width.max(1), height: MAX_SAFE };
        let entries = visible_stack_entries(&self.lock(), viewport);
        let rendered: Vec<Vec<String>> = entries.iter().map(|e| e.component.render(viewport.width)).collect();
        let sizes = allocate_stack_sizes(&entries, &rendered.iter().map(Vec::len).collect::<Vec<_>>(), None, self.gap);
        let mut lines = Vec::new();
        for (index, child_lines) in rendered.into_iter().enumerate() {
            if index > 0 {
                for _ in 0..self.gap {
                    lines.push(String::new());
                }
            }
            let size = sizes[index];
            let n = child_lines.len().min(size);
            lines.extend(child_lines.into_iter().take(size));
            for _ in n..size {
                lines.push(String::new());
            }
        }
        lines
    }

    fn render_hstack(&self, width: usize) -> Vec<String> {
        let safe_width = width.max(1);
        let viewport = LayoutViewport { width: safe_width, height: MAX_SAFE };
        let entries = visible_stack_entries(&self.lock(), viewport);
        if entries.is_empty() {
            return Vec::new();
        }
        let intrinsic: Vec<usize> = entries
            .iter()
            .map(|e| e.component.render(safe_width).iter().map(|l| visible_width(l)).max().unwrap_or(0))
            .collect();
        let widths = allocate_stack_sizes(&entries, &intrinsic, Some(safe_width), self.gap);
        let rendered: Vec<Vec<String>> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| if widths[i] == 0 { Vec::new() } else { e.component.render(widths[i]) })
            .collect();
        let height = rendered.iter().map(Vec::len).max().unwrap_or(0);
        let mut result = vec![String::new(); height];
        let mut x = 0;
        for (index, lines) in rendered.iter().enumerate() {
            let child_width = widths[index];
            let offset: i64 = match self.align {
                StackAlign::Center => (height as i64 - lines.len() as i64).div_euclid(2),
                StackAlign::End => height as i64 - lines.len() as i64,
                _ => 0,
            };
            for (row, line) in lines.iter().enumerate() {
                let target = row as i64 + offset;
                if target < 0 || target as usize >= result.len() {
                    continue;
                }
                let t = target as usize;
                result[t] = composite_tui_line(&result[t], line, x, child_width, safe_width);
            }
            x += child_width + self.gap;
        }
        result
    }
}

impl Component for Stack {
    fn render(&self, width: usize) -> Vec<String> {
        match self.kind {
            StackType::VStack => self.render_vstack(width),
            StackType::HStack => self.render_hstack(width),
        }
    }
    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.container.handle_mouse_with_owner(event, None)
    }
    fn invalidate(&self) {
        self.container.invalidate_children();
    }
    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        Some(self.container.children())
    }
    fn layout_node(&self) -> Option<LayoutNode> {
        Some(LayoutNode::Stack(self.node()))
    }
    fn uses_container_mouse(&self) -> bool {
        true
    }
}

/// Vertical stack.
pub struct VStack;

impl VStack {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(children: Vec<StackChild>, options: StackOptions) -> Stack {
        Stack::new(StackType::VStack, children, options)
    }
}

/// Horizontal stack.
pub struct HStack;

impl HStack {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(children: Vec<StackChild>, options: StackOptions) -> Stack {
        Stack::new(StackType::HStack, children, options)
    }
}
