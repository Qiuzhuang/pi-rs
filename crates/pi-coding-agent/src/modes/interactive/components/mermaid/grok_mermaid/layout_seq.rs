//! Sequence diagram layout (grok-mermaid `layout-seq.ts`).
//!
//! Participants get one column each, with lifelines running the full height and
//! a box repeated at top and bottom. Column gaps are solved from the widest
//! thing that has to fit between any two columns, then items stack down the
//! canvas in source order.

use super::Cls;
use super::canvas::{Canvas, D, L, R, U, draw_text_over_edges};
use super::graph::Shape;
use super::labels::{WRAP_WIDTH, fit_label};
use super::layout::{Placed, draw_box, half, sat};
use super::parse::{NoteAnchor, SeqHead, SeqItem, Sequence};
use super::width::string_width;

const PAD: i64 = 1;
/// Minimum columns between adjacent lifelines.
const SEQ_GAP: i64 = 5;
const MAX_CANVAS_CELLS: i64 = 1 << 21;

fn sw(s: &str) -> i64 {
    string_width(s) as i64
}

/// `Math.ceil(n / 2)`.
fn ceil_half(n: i64) -> i64 {
    (n + 1).div_euclid(2)
}

/// Where a note box sits, given the lifeline positions.
fn note_geometry(xs: &[i64], anchor: &NoteAnchor, text_w: i64) -> (i64, i64) {
    match *anchor {
        NoteAnchor::Over { from, to } => {
            let center = half(xs[from] + xs[to]);
            let w = (xs[to] - xs[from] + 5).max(text_w + 2 * PAD + 2);
            (sat(center, half(w)), w)
        }
        NoteAnchor::Left { at } => {
            let w = text_w + 2 * PAD + 2;
            (sat(xs[at], 2 + w - 1), w)
        }
        NoteAnchor::Right { at } => (xs[at] + 2, text_w + 2 * PAD + 2),
    }
}

fn item_text_w(text: &Option<String>) -> i64 {
    text.as_deref().map_or(0, sw)
}

pub(crate) fn layout_sequence(seq: &Sequence) -> Option<Canvas> {
    let n = seq.labels.len();
    let labels: Vec<String> = seq.labels.iter().map(|l| fit_label(l, WRAP_WIDTH)).collect();
    let box_w: Vec<i64> = labels.iter().map(|l| sw(l).max(1) + 2 * PAD + 2).collect();
    let box_h = 3;

    let mut gaps: Vec<i64> =
        (0..n.saturating_sub(1)).map(|i| SEQ_GAP.max(ceil_half(box_w[i]) + ceil_half(box_w[i + 1]) + 1)).collect();

    // Each requirement is "columns l..r together need at least `need` cells".
    let mut reqs: Vec<(usize, usize, i64)> = Vec::new();
    for item in &seq.items {
        match item {
            SeqItem::Message { from, to, text, .. } => {
                let tw = item_text_w(text);
                if from != to {
                    reqs.push(((*from).min(*to), (*from).max(*to), (tw + 2).max(4)));
                } else if from + 1 < n {
                    reqs.push((*from, from + 1, 5 + tw + 2));
                }
            }
            SeqItem::Note { anchor, text } => {
                let tw = sw(text);
                match *anchor {
                    NoteAnchor::Over { from, to } if from < to => reqs.push((from, to, sat(tw, 1))),
                    NoteAnchor::Over { from, .. } => {
                        let need = ceil_half(tw + 4) + 2;
                        if from > 0 {
                            reqs.push((from - 1, from, need));
                        }
                        if from + 1 < n {
                            reqs.push((from, from + 1, need));
                        }
                    }
                    NoteAnchor::Left { at } if at > 0 => reqs.push((at - 1, at, tw + 7)),
                    NoteAnchor::Right { at } if at + 1 < n => reqs.push((at, at + 1, tw + 7)),
                    _ => {}
                }
            }
            SeqItem::Divider { .. } => {}
        }
    }
    // Narrowest spans first, so a wide requirement absorbs what they already gave.
    reqs.sort_by_key(|&(l, r, _)| r as i64 - l as i64);
    for (l, r, need) in reqs {
        let cur: i64 = gaps[l..r].iter().sum();
        if cur < need {
            gaps[r - 1] += need - cur;
        }
    }

    let mut xs = vec![0i64; n];
    xs[0] = half(box_w[0]);
    for i in 1..n {
        xs[i] = xs[i - 1] + gaps[i - 1];
    }

    let mut canvas_w = xs[n - 1] + ceil_half(box_w[n - 1]) + 1;
    for item in &seq.items {
        match item {
            SeqItem::Message { from, to, text, .. } if from == to => {
                canvas_w = canvas_w.max(xs[*from] + 5 + item_text_w(text) + 1);
            }
            SeqItem::Note { anchor, text } => {
                let (x, w) = note_geometry(&xs, anchor, sw(text));
                canvas_w = canvas_w.max(x + w + 1);
            }
            SeqItem::Divider { text } => {
                canvas_w = canvas_w.max(sw(text) + 4);
            }
            _ => {}
        }
    }

    let mut rows: Vec<i64> = Vec::new();
    let mut y = box_h + 1;
    for item in &seq.items {
        rows.push(y);
        y += row_height(item);
    }
    let bottom_top = y;
    let canvas_h = bottom_top + box_h;

    if canvas_w * canvas_h > MAX_CANVAS_CELLS {
        return None;
    }

    let mut canvas = Canvas::new(canvas_w, canvas_h);

    for i in 0..n {
        for by in [0, bottom_top] {
            draw_box(
                &mut canvas,
                &bx(sat(xs[i], half(box_w[i])), by, box_w[i], box_h),
                std::slice::from_ref(&labels[i]),
                Shape::Rect,
            );
        }
    }
    for (k, item) in seq.items.iter().enumerate() {
        let SeqItem::Note { anchor, text } = item else { continue };
        let (x, w) = note_geometry(&xs, anchor, sw(text));
        draw_box(&mut canvas, &bx(x, rows[k], w, 3), std::slice::from_ref(text), Shape::Rect);
    }

    for &x in &xs {
        canvas.junction(x, box_h - 1, D);
        canvas.seg_v(x, box_h, bottom_top - 1);
        canvas.junction(x, bottom_top, U);
    }

    for (k, item) in seq.items.iter().enumerate() {
        let r = rows[k];
        match item {
            SeqItem::Message { .. } => draw_message(&mut canvas, item, &xs, r),
            SeqItem::Divider { text } => draw_divider(&mut canvas, text, r, canvas_w),
            SeqItem::Note { .. } => {}
        }
    }

    canvas.finalize_mask();
    Some(canvas)
}

fn row_height(item: &SeqItem) -> i64 {
    match item {
        SeqItem::Note { .. } => 4,
        SeqItem::Divider { .. } => 2,
        SeqItem::Message { from, to, .. } if from == to => 4,
        SeqItem::Message { text, .. } => {
            if text.is_some() {
                3
            } else {
                2
            }
        }
    }
}

/// Geometry for a box drawn by position and size; ranks are irrelevant here.
fn bx(x: i64, y: i64, w: i64, h: i64) -> Placed {
    Placed { x, y, w, h, cx: x + half(w), cy: y + 1, rank: 0 }
}

fn draw_message(canvas: &mut Canvas, item: &SeqItem, xs: &[i64], r: i64) {
    let SeqItem::Message { from, to, text, dashed, head } = item else { return };
    let line_ch = if *dashed { "╌" } else { "─" };

    if from == to {
        // A stub that leaves the lifeline and returns two rows down.
        let x = xs[*from];
        canvas.junction(x, r, R);
        canvas.set(x + 1, r, line_ch, Cls::Edge);
        canvas.set(x + 2, r, line_ch, Cls::Edge);
        canvas.set(x + 3, r, "╮", Cls::Edge);
        canvas.set(x + 3, r + 1, "│", Cls::Edge);
        canvas.set(x + 1, r + 2, if *head == SeqHead::Cross { "×" } else { "◄" }, Cls::Edge);
        canvas.set(x + 2, r + 2, line_ch, Cls::Edge);
        canvas.set(x + 3, r + 2, "╯", Cls::Edge);
        if let Some(text) = text {
            draw_text_over_edges(canvas, text, x + 5, r + 1, Cls::Text);
        }
        return;
    }

    let x0 = xs[*from];
    let x1 = xs[*to];
    let rightward = x1 > x0;
    // A labelled message writes its text on `r` and draws the arrow below it.
    let arrow_row = if text.is_some() { r + 1 } else { r };
    let lo = x0.min(x1);
    let hi = x0.max(x1);

    canvas.junction(x0, arrow_row, if rightward { R } else { L });
    for x in lo + 1..hi {
        canvas.set(x, arrow_row, line_ch, Cls::Edge);
    }
    let head_ch = if *head == SeqHead::Cross {
        "×"
    } else if rightward {
        "▶"
    } else {
        "◄"
    };
    canvas.set(if rightward { x1 - 1 } else { x1 + 1 }, arrow_row, head_ch, Cls::Edge);

    if let Some(text) = text {
        let span = hi - lo - 1;
        let t = fit_label(text, span.max(1) as usize);
        draw_text_over_edges(canvas, &t, lo + 1 + half(sat(span, sw(&t))), r, Cls::Text);
    }
}

/// A full-width rule labelling a `loop` / `alt` / `opt` block boundary.
fn draw_divider(canvas: &mut Canvas, text: &str, r: i64, canvas_w: i64) {
    for x in 0..canvas_w {
        canvas.set(x, r, "─", Cls::Edge);
    }
    draw_text_over_edges(canvas, &format!(" {} ", fit_label(text, sat(canvas_w, 4) as usize)), 2, r, Cls::EdgeLabel);
}
