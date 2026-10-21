//! Fullscreen layout engine: stacks, scroll views, clipping and scrollbars.
//! Port of `packages/tui/src/layout.ts`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::components::scroll_view::{ScrollView, ScrollViewScrollbar};
use crate::components::stack::{allocate_stack_sizes, visible_stack_entries};
use crate::layout_node::{LayoutNode, LayoutViewport, StackAlign, StackBasis, StackType};
use crate::terminal_image::{crop_kitty_image_line, get_kitty_image_metadata, is_image_line};
use crate::tui::{CURSOR_MARKER, ComponentRef, component_ptr, composite_tui_line};
use crate::utils::{extract_ansi_code, get_active_background_ansi, get_grapheme_cell_range, slice_by_column, visible_width};

/// Strip leading OSC 133 zone markers (`A`, `B`, `C`).
pub(crate) fn strip_osc133_zone_prefix(line: &str) -> &str {
    let mut rest = line;
    loop {
        let Some(after) = rest.strip_prefix("\x1b]133;") else { break };
        let mut chars = after.chars();
        match chars.next() {
            Some('A' | 'B' | 'C') => {}
            _ => break,
        }
        let tail = &after[1..];
        if let Some(t) = tail.strip_prefix('\x07') {
            rest = t;
        } else if let Some(t) = tail.strip_prefix("\x1b\\") {
            rest = t;
        } else {
            break;
        }
    }
    rest
}

/// Whether the line starts with an OSC 133 prompt-start marker.
pub(crate) fn starts_with_osc133_prompt(line: &str) -> bool {
    line.starts_with("\x1b]133;A\x07") || line.starts_with("\x1b]133;A\x1b\\")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayoutRect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

/// A laid-out component.
#[derive(Clone)]
pub struct LayoutBox {
    pub component: ComponentRef,
    pub rect: LayoutRect,
    pub clip: LayoutRect,
    pub children: Vec<LayoutBox>,
    pub lines: Option<Arc<Vec<String>>>,
    pub line_offset: usize,
    pub scroll_view: Option<Arc<ScrollView>>,
    pub scroll_content_lines: Option<Arc<Vec<String>>>,
    pub layer: i64,
}

/// A rendered fullscreen frame.
#[derive(Clone)]
pub struct LayoutFrame {
    pub root: LayoutBox,
    pub width: usize,
    pub height: usize,
    pub lines: Vec<String>,
    pub primary_scroll_view: Option<Arc<ScrollView>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarGeometry {
    pub column: i64,
    pub track_top: i64,
    pub track_height: i64,
    pub thumb_top: i64,
    pub thumb_height: i64,
    pub max_scroll_top: i64,
}

type RenderRequest = Arc<dyn Fn() + Send + Sync>;

struct LayoutContext {
    viewport: LayoutViewport,
    render_cache: HashMap<*const (), HashMap<usize, Arc<Vec<String>>>>,
    request_render: RenderRequest,
    primary_scroll_view: Option<Arc<ScrollView>>,
}

fn intersect(a: LayoutRect, b: LayoutRect) -> LayoutRect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    LayoutRect { x, y, width: (right - x).max(0), height: (bottom - y).max(0) }
}

fn render_cached(ctx: &mut LayoutContext, component: &ComponentRef, width: i64) -> Arc<Vec<String>> {
    let safe = width.max(1) as usize;
    let key = component_ptr(component);
    if let Some(lines) = ctx.render_cache.get(&key).and_then(|m| m.get(&safe)) {
        return lines.clone();
    }
    let lines = Arc::new(component.render(safe));
    ctx.render_cache.entry(key).or_default().insert(safe, lines.clone());
    lines
}

fn measure_height(ctx: &mut LayoutContext, component: &ComponentRef, width: i64) -> usize {
    render_cached(ctx, component, width).len()
}

fn measure_width(ctx: &mut LayoutContext, component: &ComponentRef, width: i64) -> usize {
    render_cached(ctx, component, width).iter().map(|l| visible_width(l)).max().unwrap_or(0)
}

fn translate_box(b: &mut LayoutBox, dy: i64) {
    b.rect.y += dy;
    for c in b.children.iter_mut() {
        translate_box(c, dy);
    }
}

fn update_clips(b: &mut LayoutBox, parent_clip: LayoutRect) {
    b.clip = intersect(parent_clip, b.rect);
    let clip = b.clip;
    for c in b.children.iter_mut() {
        update_clips(c, clip);
    }
}

fn layout_component(
    ctx: &mut LayoutContext,
    component: &ComponentRef,
    x: i64,
    y: i64,
    width: i64,
    height: Option<i64>,
    clip: LayoutRect,
) -> LayoutBox {
    let safe_width = width.max(1);
    let Some(node) = component.layout_node() else {
        let lines = render_cached(ctx, component, safe_width);
        let allocated = match height {
            None => lines.len() as i64,
            Some(h) => h.max(0),
        };
        let mut line_offset = 0;
        if lines.len() as i64 > allocated && allocated > 0 {
            if let Some(cursor_line) = lines.iter().position(|l| l.contains(CURSOR_MARKER)) {
                if cursor_line as i64 >= allocated {
                    line_offset = (cursor_line as i64 - allocated + 1) as usize;
                }
            }
        }
        let rect = LayoutRect { x, y, width: safe_width, height: allocated };
        return LayoutBox {
            component: component.clone(),
            rect,
            clip: intersect(clip, rect),
            children: Vec::new(),
            lines: Some(lines),
            line_offset,
            scroll_view: None,
            scroll_content_lines: None,
            layer: 0,
        };
    };

    match node {
        LayoutNode::Scroll(scroll) => {
            let state = scroll.state.clone();
            let previous_scroll_top = state.scroll_top();
            let content_width = state.get_content_width(safe_width as usize) as i64;
            let mut child_box =
                layout_component(ctx, &scroll.component, x, y - previous_scroll_top, content_width, None, clip);
            let content_height = child_box.rect.height;
            let viewport_height = match height {
                None => content_height,
                Some(h) => h.max(0),
            };
            state.update_layout(content_height, viewport_height, ctx.request_render.clone());
            translate_box(&mut child_box, previous_scroll_top - state.scroll_top());
            if state.primary || ctx.primary_scroll_view.is_none() {
                ctx.primary_scroll_view = Some(state.clone());
            }
            let rect = LayoutRect { x, y, width: safe_width, height: viewport_height };
            let child_clip = intersect(clip, rect);
            update_clips(&mut child_box, child_clip);
            let scroll_content_lines = render_cached(ctx, &scroll.component, content_width);
            LayoutBox {
                component: component.clone(),
                rect,
                clip: child_clip,
                children: vec![child_box],
                lines: None,
                line_offset: 0,
                scroll_view: Some(state),
                scroll_content_lines: Some(scroll_content_lines),
                layer: 0,
            }
        }
        LayoutNode::Stack(stack) => {
            let entries = visible_stack_entries(&stack.entries, ctx.viewport);
            let gap_total = entries.len().saturating_sub(1) as i64 * stack.gap as i64;
            if stack.kind == StackType::VStack {
                let intrinsic: Vec<usize> = entries
                    .iter()
                    .map(|e| match e.basis {
                        Some(StackBasis::Size(s)) => s,
                        _ => measure_height(ctx, &e.component, safe_width),
                    })
                    .collect();
                let sizes = allocate_stack_sizes(&entries, &intrinsic, height.map(|h| h.max(0) as usize), stack.gap);
                let natural: i64 = sizes.iter().map(|s| *s as i64).sum::<i64>() + gap_total;
                let allocated = match height {
                    None => natural,
                    Some(h) => h.max(0),
                };
                let rect = LayoutRect { x, y, width: safe_width, height: allocated };
                let box_clip = intersect(clip, rect);
                let mut children = Vec::new();
                let mut child_y = y;
                for (i, e) in entries.iter().enumerate() {
                    children.push(layout_component(
                        ctx,
                        &e.component,
                        x,
                        child_y,
                        safe_width,
                        Some(sizes[i] as i64),
                        box_clip,
                    ));
                    child_y += sizes[i] as i64 + stack.gap as i64;
                }
                return LayoutBox {
                    component: component.clone(),
                    rect,
                    clip: box_clip,
                    children,
                    lines: None,
                    line_offset: 0,
                    scroll_view: None,
                    scroll_content_lines: None,
                    layer: 0,
                };
            }
            let intrinsic_widths: Vec<usize> = entries
                .iter()
                .map(|e| match e.basis {
                    Some(StackBasis::Size(s)) => s,
                    _ => measure_width(ctx, &e.component, safe_width),
                })
                .collect();
            let widths = allocate_stack_sizes(&entries, &intrinsic_widths, Some(safe_width as usize), stack.gap);
            let intrinsic_heights: Vec<i64> = entries
                .iter()
                .enumerate()
                .map(|(i, e)| measure_height(ctx, &e.component, (widths[i] as i64).max(1)) as i64)
                .collect();
            let allocated = match height {
                None => intrinsic_heights.iter().copied().max().unwrap_or(0),
                Some(h) => h.max(0),
            };
            let rect = LayoutRect { x, y, width: safe_width, height: allocated };
            let box_clip = intersect(clip, rect);
            let mut children = Vec::new();
            let mut child_x = x;
            for (i, e) in entries.iter().enumerate() {
                let natural = intrinsic_heights[i];
                let child_height = if stack.align == StackAlign::Stretch { allocated } else { allocated.min(natural) };
                let mut child_y = y;
                match stack.align {
                    StackAlign::Center => child_y += (allocated - child_height).div_euclid(2),
                    StackAlign::End => child_y += allocated - child_height,
                    _ => {}
                }
                let child_width = widths[i] as i64;
                if child_width == 0 {
                    children.push(LayoutBox {
                        component: e.component.clone(),
                        rect: LayoutRect { x: child_x, y: child_y, width: 0, height: child_height },
                        clip: LayoutRect { x: child_x, y: child_y, width: 0, height: 0 },
                        children: Vec::new(),
                        lines: None,
                        line_offset: 0,
                        scroll_view: None,
                        scroll_content_lines: None,
                        layer: 0,
                    });
                } else {
                    children.push(layout_component(
                        ctx,
                        &e.component,
                        child_x,
                        child_y,
                        child_width,
                        Some(child_height),
                        box_clip,
                    ));
                }
                child_x += child_width + stack.gap as i64;
            }
            LayoutBox {
                component: component.clone(),
                rect,
                clip: box_clip,
                children,
                lines: None,
                line_offset: 0,
                scroll_view: None,
                scroll_content_lines: None,
                layer: 0,
            }
        }
    }
}

fn replace_scrollbar_cell(
    line: &str,
    column: i64,
    total_width: usize,
    replacement: &str,
    preserve_target_background: bool,
) -> String {
    if is_image_line(line) {
        return line.to_string();
    }
    let column_u = column.max(0) as usize;
    let range = get_grapheme_cell_range(line, column_u);
    let start = range.map_or(column_u, |r| r.start);
    let end = range.map_or(column_u + 1, |r| r.end);
    let before = slice_by_column(line, 0, start, true);
    let target = slice_by_column(line, start, end - start, true);
    let after = slice_by_column(line, end, total_width.saturating_sub(end), true);
    let mut target_prefix = String::new();
    let mut idx = 0;
    while idx < target.len() {
        let Some((code, len)) = extract_ansi_code(&target, idx) else { break };
        target_prefix.push_str(code);
        idx += len;
    }
    let before_padding = " ".repeat(start.saturating_sub(visible_width(&before)));
    let cell_padding_before = " ".repeat(column_u.saturating_sub(start));
    let cell_padding_after = " ".repeat(end.saturating_sub(column_u + 1));
    let target_style = format!(
        "\x1b[0m\x1b]8;;\x07{}",
        if preserve_target_background { get_active_background_ansi(&target_prefix) } else { String::new() }
    );
    format!("{before}{before_padding}{target_style}{cell_padding_before}{replacement}{cell_padding_after}{after}")
}

/// Scrollbar geometry of a scroll view box.
pub fn get_scrollbar_geometry(b: &LayoutBox, include_hidden_auto: bool) -> Option<ScrollbarGeometry> {
    let sv = b.scroll_view.as_ref()?;
    if b.rect.width <= 0 || b.rect.height <= 0 {
        return None;
    }
    let content_height = b
        .children
        .first()
        .map(|c| c.rect.height)
        .or_else(|| b.scroll_content_lines.as_ref().map(|l| l.len() as i64))
        .unwrap_or(0);
    let track_height = b.rect.height;
    let can_reveal = include_hidden_auto && sv.scrollbar() == ScrollViewScrollbar::Auto && content_height > track_height;
    if !sv.is_scrollbar_visible() && !can_reveal {
        return None;
    }
    let min_thumb = 2.min(track_height);
    let ratio = if content_height == 0 {
        f64::INFINITY
    } else {
        (track_height as f64 * track_height as f64) / content_height as f64
    };
    let thumb_height = min_thumb.max(track_height.min(js_round(ratio)));
    let max_scroll_top = (content_height - track_height).max(0);
    let max_thumb_top = track_height - thumb_height;
    let thumb_offset = if max_scroll_top == 0 {
        0
    } else {
        js_round((sv.scroll_top() as f64 / max_scroll_top as f64) * max_thumb_top as f64)
    };
    let column = b.rect.x + b.rect.width - 1;
    if column < b.clip.x || column >= b.clip.x + b.clip.width {
        return None;
    }
    Some(ScrollbarGeometry {
        column,
        track_top: b.rect.y,
        track_height,
        thumb_top: b.rect.y + thumb_offset,
        thumb_height,
        max_scroll_top,
    })
}

fn js_round(v: f64) -> i64 {
    if v.is_infinite() {
        return if v > 0.0 { i64::MAX } else { i64::MIN };
    }
    (v + 0.5).floor() as i64
}

fn paint_scrollbar(b: &LayoutBox, screen: &mut [String], total_width: usize) {
    let Some(geometry) = get_scrollbar_geometry(b, false) else { return };
    let Some(sv) = &b.scroll_view else { return };
    for offset in 0..geometry.track_height {
        let row = geometry.track_top + offset;
        if row < b.clip.y || row >= b.clip.y + b.clip.height || row < 0 || row as usize >= screen.len() {
            continue;
        }
        let is_thumb = row >= geometry.thumb_top && row < geometry.thumb_top + geometry.thumb_height;
        let replacement = if is_thumb {
            sv.scrollbar_thumb_style(if sv.is_scrollbar_active() { "█" } else { "┃" })
        } else {
            sv.scrollbar_track_style("│")
        };
        let r = row as usize;
        screen[r] = replace_scrollbar_cell(
            &screen[r],
            geometry.column,
            total_width,
            &replacement,
            sv.scrollbar() != ScrollViewScrollbar::Always,
        );
    }
}

fn paint_box(b: &LayoutBox, screen: &mut [String], total_width: usize) {
    if let Some(lines) = &b.lines {
        let offset = b.line_offset as i64;
        let first_row = b.rect.y.max(b.clip.y).max(0);
        let last_row = (b.rect.y + b.rect.height).min(b.clip.y + b.clip.height).min(screen.len() as i64);
        let mut row = first_row;
        while row < last_row {
            let src_index = offset + row - b.rect.y;
            if src_index < 0 || src_index as usize >= lines.len() {
                row += 1;
                continue;
            }
            let source = &lines[src_index as usize];
            let mut line = strip_osc133_zone_prefix(source).to_string();
            if let Some(meta) = get_kitty_image_metadata(&line) {
                let clip_bottom = (screen.len() as i64).min(b.clip.y + b.clip.height);
                let visible_rows = (meta.rows as i64).min(clip_bottom - row);
                if visible_rows < meta.rows as i64 {
                    line = crop_kitty_image_line(&line, 0, visible_rows.max(0) as usize);
                }
            }
            let r = row as usize;
            if b.rect.x == 0 && b.rect.width >= total_width as i64 && (is_image_line(&line) || screen[r].is_empty()) {
                screen[r] = line;
            } else {
                screen[r] = composite_tui_line(
                    &screen[r],
                    &line,
                    b.rect.x.max(0) as usize,
                    b.rect.width.max(0) as usize,
                    total_width,
                );
            }
            row += 1;
        }
    }
    for child in &b.children {
        paint_box(child, screen, total_width);
    }
    if let (Some(sv), Some(content)) = (&b.scroll_view, &b.scroll_content_lines) {
        let scroll_top = sv.scroll_top();
        if scroll_top > 0 && b.rect.height > 0 {
            let mut image_row = scroll_top - 1;
            while image_row >= 0 {
                let image_line = content.get(image_row as usize).map(String::as_str).unwrap_or("");
                if let Some(meta) = get_kitty_image_metadata(image_line) {
                    let hidden_rows = scroll_top - image_row;
                    if hidden_rows < meta.rows as i64 {
                        let visible_rows = b.rect.height.min(meta.rows as i64 - hidden_rows);
                        let cropped = crop_kitty_image_line(image_line, hidden_rows as usize, visible_rows as usize);
                        if b.rect.x == 0 && b.rect.width >= total_width as i64 && b.rect.y >= 0 && (b.rect.y as usize) < screen.len() {
                            screen[b.rect.y as usize] = cropped;
                        }
                    }
                    break;
                }
                if !image_line.is_empty() {
                    break;
                }
                image_row -= 1;
            }
        }
    }
    paint_scrollbar(b, screen, total_width);
}

/// Lay out and paint a component tree into a frame of `width` x `height` cells.
pub fn render_layout_frame(root: &ComponentRef, width: usize, height: usize, request_render: RenderRequest) -> LayoutFrame {
    let safe_width = width.max(1);
    let safe_height = height.max(1);
    let mut ctx = LayoutContext {
        viewport: LayoutViewport { width: safe_width, height: safe_height },
        render_cache: HashMap::new(),
        request_render,
        primary_scroll_view: None,
    };
    let full = LayoutRect { x: 0, y: 0, width: safe_width as i64, height: safe_height as i64 };
    let root_box = layout_component(&mut ctx, root, 0, 0, safe_width as i64, Some(safe_height as i64), full);
    let mut lines = vec![String::new(); safe_height];
    paint_box(&root_box, &mut lines, safe_width);
    LayoutFrame { root: root_box, width: safe_width, height: safe_height, lines, primary_scroll_view: ctx.primary_scroll_view }
}

fn contains_point(r: &LayoutRect, x: i64, y: i64) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// The visual hit path from the deepest component to the layout root.
pub fn get_layout_boxes_at(frame: &LayoutFrame, x: i64, y: i64) -> Vec<LayoutBox> {
    let mut result: Vec<(LayoutBox, usize)> = Vec::new();
    fn visit(b: &LayoutBox, depth: usize, x: i64, y: i64, out: &mut Vec<(LayoutBox, usize)>) {
        if !contains_point(&b.clip, x, y) {
            return;
        }
        out.push((b.clone(), depth));
        for c in &b.children {
            visit(c, depth + 1, x, y, out);
        }
    }
    visit(&frame.root, 0, x, y, &mut result);
    result.sort_by(|a, b| b.0.layer.cmp(&a.0.layer).then(b.1.cmp(&a.1)));
    result.into_iter().map(|(b, _)| b).collect()
}

/// The layout box of a scroll view.
pub fn get_scroll_view_box<'a>(frame: &'a LayoutFrame, scroll_view: &Arc<ScrollView>) -> Option<&'a LayoutBox> {
    fn visit<'a>(b: &'a LayoutBox, sv: &Arc<ScrollView>) -> Option<&'a LayoutBox> {
        if b.scroll_view.as_ref().is_some_and(|s| Arc::ptr_eq(s, sv)) {
            return Some(b);
        }
        b.children.iter().find_map(|c| visit(c, sv))
    }
    visit(&frame.root, scroll_view)
}

/// Scroll views under a point, deepest first.
pub fn get_scroll_views_at(frame: &LayoutFrame, x: i64, y: i64) -> Vec<Arc<ScrollView>> {
    let mut result: Vec<(Arc<ScrollView>, usize)> = Vec::new();
    fn visit(b: &LayoutBox, depth: usize, x: i64, y: i64, out: &mut Vec<(Arc<ScrollView>, usize)>) {
        if !contains_point(&b.clip, x, y) {
            return;
        }
        if let Some(sv) = &b.scroll_view {
            if contains_point(&b.rect, x, y) {
                out.push((sv.clone(), depth));
            }
        }
        for c in &b.children {
            visit(c, depth + 1, x, y, out);
        }
    }
    visit(&frame.root, 0, x, y, &mut result);
    result.sort_by(|a, b| b.1.cmp(&a.1));
    result.into_iter().map(|(s, _)| s).collect()
}
