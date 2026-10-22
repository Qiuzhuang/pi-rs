//! The drawing grid (grok-mermaid `canvas.ts`).
//!
//! Coordinates are `i64` like the JS numbers they port: an out-of-range negative index is ignored
//! on write, as a negative array index is in JS.

use super::width::measured;
use super::{Cls, Span};

/// Sentinel occupying the trailing column of a wide glyph. Never emitted.
pub(crate) const CONT: &str = "\0";

/// Connection direction bits, combined into a box-drawing glyph by `mask_char`.
pub(crate) const U: u8 = 1;
pub(crate) const D: u8 = 2;
pub(crate) const L: u8 = 4;
pub(crate) const R: u8 = 8;

/// Line styles, tracked per cell so crossing edges keep their own stroke.
pub(crate) const STY_DOT: u8 = 1;
pub(crate) const STY_THICK: u8 = 2;
pub(crate) const STY_SOLID: u8 = 4;

/// A grid of cells. Edges accumulate as direction bits rather than glyphs so
/// that crossings and junctions resolve correctly whatever order they are drawn
/// in; `finalize_mask` turns the accumulated bits into characters at the end.
#[derive(Debug, Clone)]
pub(crate) struct Canvas {
    pub w: i64,
    pub h: i64,
    pub ch: Vec<String>,
    pub cls: Vec<Cls>,
    pub mask: Vec<u8>,
    pub style: Vec<u8>,
    pub occupied: Vec<u8>,
    pub cur_style: u8,
}

/// Result of [`Canvas::to_lines`].
pub(crate) struct Lines {
    pub plain: Vec<String>,
    pub styled: Vec<Vec<Span>>,
    pub width: usize,
}

impl Canvas {
    pub(crate) fn new(w: i64, h: i64) -> Self {
        let n = (w * h).max(0) as usize;
        Self {
            w,
            h,
            ch: vec![" ".to_string(); n],
            cls: vec![Cls::None; n],
            mask: vec![0; n],
            style: vec![0; n],
            occupied: vec![0; n],
            cur_style: STY_SOLID,
        }
    }

    pub(crate) fn idx(&self, x: i64, y: i64) -> i64 {
        y * self.w + x
    }

    /// The array index for `(x, y)` when it addresses a cell.
    pub(crate) fn cell(&self, x: i64, y: i64) -> Option<usize> {
        let i = self.idx(x, y);
        (i >= 0 && (i as usize) < self.ch.len()).then_some(i as usize)
    }

    pub(crate) fn set(&mut self, x: i64, y: i64, c: &str, cls: Cls) {
        if x >= self.w || y >= self.h {
            return;
        }
        if let Some(i) = self.cell(x, y) {
            self.ch[i] = c.to_string();
            self.cls[i] = cls;
        }
    }

    pub(crate) fn set_occupied(&mut self, x: i64, y: i64) {
        if let Some(i) = self.cell(x, y) {
            self.occupied[i] = 1;
        }
    }

    /// Accumulate direction bits on a free cell. `border` cells are never reclassified.
    pub(crate) fn add_bits(&mut self, x: i64, y: i64, bits: u8, cls: Cls) {
        if x >= self.w || y >= self.h {
            return;
        }
        let Some(i) = self.cell(x, y) else { return };
        if self.occupied[i] != 0 {
            return;
        }
        self.mask[i] |= bits;
        self.style[i] |= self.cur_style;
        if self.cls[i] != Cls::Border {
            self.cls[i] = cls;
        }
    }

    /// Stamp a finished sub-canvas (a subgraph frame's contents) at an offset.
    pub(crate) fn blit(&mut self, sub: &Canvas, ox: i64, oy: i64) {
        for sy in 0..sub.h {
            for sx in 0..sub.w {
                let x = ox + sx;
                let y = oy + sy;
                if x >= self.w || y >= self.h {
                    continue;
                }
                let Some(si) = sub.cell(sx, sy) else { continue };
                let Some(di) = self.cell(x, y) else { continue };
                self.ch[di] = sub.ch[si].clone();
                self.cls[di] = sub.cls[si];
                self.style[di] = sub.style[si];
                self.occupied[di] = 1;
            }
        }
    }

    /// Add direction bits even to an occupied cell, so an edge can meet a border.
    pub(crate) fn junction(&mut self, x: i64, y: i64, bits: u8) {
        if x >= self.w || y >= self.h {
            return;
        }
        let Some(i) = self.cell(x, y) else { return };
        self.mask[i] |= bits;
        if self.cls[i] != Cls::Border {
            self.cls[i] = Cls::Edge;
        }
    }

    pub(crate) fn seg_v(&mut self, x: i64, y0: i64, y1: i64) {
        let (a, b) = (y0.min(y1), y0.max(y1));
        for y in a..=b {
            let mut bits = 0;
            if y > a {
                bits |= U;
            }
            if y < b {
                bits |= D;
            }
            self.add_bits(x, y, bits, Cls::Edge);
        }
    }

    pub(crate) fn seg_h(&mut self, y: i64, x0: i64, x1: i64) {
        let (a, b) = (x0.min(x1), x0.max(x1));
        for x in a..=b {
            let mut bits = 0;
            if x > a {
                bits |= L;
            }
            if x < b {
                bits |= R;
            }
            self.add_bits(x, y, bits, Cls::Edge);
        }
    }

    /// Resolve accumulated direction bits into glyphs, honouring line style.
    pub(crate) fn finalize_mask(&mut self) {
        for i in 0..self.ch.len() {
            if self.mask[i] != 0 && self.ch[i] == " " {
                let c = mask_char(self.mask[i]);
                self.ch[i] = if self.style[i] == STY_DOT {
                    dotted_char(c)
                } else if self.style[i] == STY_THICK {
                    thick_char(c)
                } else {
                    c
                }
                .to_string();
            }
        }
    }

    /// Mirror top-to-bottom for `BT`. Rows reorder but within-row text does not.
    pub(crate) fn flip_vertical(&mut self) {
        for y in 0..self.h / 2 {
            let y2 = self.h - 1 - y;
            for x in 0..self.w {
                let (Some(i), Some(j)) = (self.cell(x, y), self.cell(x, y2)) else { continue };
                self.ch.swap(i, j);
                self.cls.swap(i, j);
            }
        }
        for c in &mut self.ch {
            *c = flip_glyph_v(c).to_string();
        }
    }

    /// Mirror left-to-right for `RL`; text/label runs are reversed back to reading order.
    pub(crate) fn flip_horizontal(&mut self) {
        for y in 0..self.h {
            for x in 0..self.w / 2 {
                let x2 = self.w - 1 - x;
                let (Some(i), Some(j)) = (self.cell(x, y), self.cell(x2, y)) else { continue };
                self.ch.swap(i, j);
                self.cls.swap(i, j);
            }
        }
        for c in &mut self.ch {
            *c = flip_glyph_h(c).to_string();
        }
        for y in 0..self.h {
            let mut x = 0;
            while x < self.w {
                let Some(start) = self.cell(x, y) else { break };
                let cls = self.cls[start];
                if cls == Cls::Text || cls == Cls::EdgeLabel {
                    while x < self.w && self.cell(x, y).is_some_and(|i| self.cls[i] == cls) {
                        x += 1;
                    }
                    let end = (self.idx(x, y).max(0) as usize).min(self.ch.len());
                    self.ch[start..end].reverse();
                } else {
                    x += 1;
                }
            }
        }
    }

    /// Group each row into runs of one class, dropping wide-glyph continuations.
    pub(crate) fn to_lines(&self) -> Lines {
        let mut plain: Vec<String> = Vec::new();
        let mut styled: Vec<Vec<Span>> = Vec::new();
        let mut width = 0usize;
        for y in 0..self.h {
            // A trailing CONT counts as painted: it is the second cell of a wide glyph.
            let mut last = 0i64;
            for x in (0..self.w).rev() {
                if self.cell(x, y).is_some_and(|i| self.ch[i] != " ") {
                    last = x + 1;
                    break;
                }
            }
            width = width.max(last as usize);
            let mut spans: Vec<Span> = Vec::new();
            let mut plain_row = String::new();
            let mut run = String::new();
            let mut run_cls = Cls::None;
            for x in 0..last {
                let Some(i) = self.cell(x, y) else { continue };
                let c = &self.ch[i];
                if c == CONT {
                    continue;
                }
                let cls = self.cls[i];
                plain_row.push_str(c);
                if cls != run_cls && !run.is_empty() {
                    spans.push(Span { text: std::mem::take(&mut run), cls: run_cls });
                }
                run_cls = cls;
                run.push_str(c);
            }
            if !run.is_empty() {
                spans.push(Span { text: run, cls: run_cls });
            }
            styled.push(spans);
            // Only ASCII spaces, which is all a blank cell ever holds.
            plain.push(plain_row.trim_end_matches(' ').to_string());
        }
        let mut first = 0;
        while first < plain.len() && plain[first].is_empty() {
            first += 1;
        }
        let mut end = plain.len();
        while end > first && plain[end - 1].is_empty() {
            end -= 1;
        }
        Lines { plain: plain[first..end].to_vec(), styled: styled[first..end].to_vec(), width }
    }
}

/// Paint `text` at `x, y`, one grapheme cluster per cell.
pub(crate) fn draw_text(canvas: &mut Canvas, text: &str, x: i64, y: i64, cls: Cls) {
    let mut cur = x;
    for (cluster, cw) in measured(text) {
        if cw == 0 {
            continue;
        }
        canvas.set(cur, y, cluster, cls);
        for k in 1..cw as i64 {
            canvas.set(cur + k, y, CONT, cls);
        }
        cur += cw as i64;
    }
}

/// Paint `text` at `x, y`, clearing any edge bits underneath first.
pub(crate) fn draw_text_over_edges(canvas: &mut Canvas, text: &str, x: i64, y: i64, cls: Cls) {
    let mut cur = x;
    for (cluster, cw) in measured(text) {
        if cw == 0 {
            continue;
        }
        for k in 0..cw as i64 {
            if cur + k < canvas.w
                && y < canvas.h
                && let Some(i) = canvas.cell(cur + k, y)
            {
                canvas.mask[i] = 0;
            }
            canvas.set(cur + k, y, if k == 0 { cluster } else { CONT }, cls);
        }
        cur += cw as i64;
    }
}

pub(crate) fn mask_char(mask: u8) -> &'static str {
    match mask {
        0 => " ",
        m if m == U || m == D || m == U | D => "│",
        m if m == L || m == R || m == L | R => "─",
        m if m == D | R => "┌",
        m if m == D | L => "┐",
        m if m == U | R => "└",
        m if m == U | L => "┘",
        m if m == U | D | R => "├",
        m if m == U | D | L => "┤",
        m if m == D | L | R => "┬",
        m if m == U | L | R => "┴",
        _ => "┼",
    }
}

pub(crate) fn dotted_char(c: &str) -> &str {
    match c {
        "─" => "╌",
        "│" => "╎",
        _ => c,
    }
}

pub(crate) fn thick_char(c: &str) -> &str {
    match c {
        "─" => "━",
        "│" => "┃",
        "┌" => "┏",
        "┐" => "┓",
        "└" => "┗",
        "┘" => "┛",
        "├" => "┣",
        "┤" => "┫",
        "┬" => "┳",
        "┴" => "┻",
        "┼" => "╋",
        _ => c,
    }
}

pub(crate) fn flip_glyph_v(c: &str) -> &str {
    match c {
        "┌" => "└",
        "└" => "┌",
        "┐" => "┘",
        "┘" => "┐",
        "┏" => "┗",
        "┗" => "┏",
        "┓" => "┛",
        "┛" => "┓",
        "╭" => "╰",
        "╰" => "╭",
        "╮" => "╯",
        "╯" => "╮",
        "┬" => "┴",
        "┴" => "┬",
        "┳" => "┻",
        "┻" => "┳",
        "▼" => "▲",
        "▲" => "▼",
        "▽" => "△",
        "△" => "▽",
        _ => c,
    }
}

pub(crate) fn flip_glyph_h(c: &str) -> &str {
    match c {
        "┌" => "┐",
        "┐" => "┌",
        "└" => "┘",
        "┘" => "└",
        "┏" => "┓",
        "┓" => "┏",
        "┗" => "┛",
        "┛" => "┗",
        "╭" => "╮",
        "╮" => "╭",
        "╰" => "╯",
        "╯" => "╰",
        "├" => "┤",
        "┤" => "├",
        "┣" => "┫",
        "┫" => "┣",
        "▶" => "◄",
        "◄" => "▶",
        "▷" => "◁",
        "◁" => "▷",
        _ => c,
    }
}
