//! Test helpers: an in-memory terminal emulator implementing [`Terminal`].
//!
//! [`VirtualTerminal`] mirrors the TypeScript test helper built on `@xterm/headless`: it
//! interprets the escape sequences the renderer emits (cursor movement, erase, scrolling,
//! alternate screen, autowrap) and exposes the viewport and scrollback as plain text.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::terminal::{InputHandler, ResizeHandler, Terminal};
use crate::utils::{grapheme_width, graphemes};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Cell {
    /// Grapheme stored in this cell; empty for blank cells and for the trailing half of
    /// wide characters.
    text: String,
    /// True for the cell following a wide character.
    continuation: bool,
}

type Line = Vec<Cell>;

fn blank_line(cols: usize) -> Line {
    vec![Cell::default(); cols]
}

/// Like xterm's `translateToString(true)`: trailing cells that were never written (or were
/// erased) are trimmed; explicitly written spaces are kept.
fn line_to_string(line: &Line) -> String {
    let end = line.iter().rposition(|cell| !cell.text.is_empty()).map_or(0, |i| i + 1);
    let mut s = String::new();
    for cell in &line[..end] {
        if cell.continuation {
            continue;
        }
        if cell.text.is_empty() {
            s.push(' ');
        } else {
            s.push_str(&cell.text);
        }
    }
    s
}

#[derive(Clone)]
struct Buffer {
    /// Scrollback lines followed by the screen rows (last `rows` entries are the screen).
    lines: Vec<Line>,
    cursor_x: usize,
    cursor_y: usize,
    wrap_pending: bool,
    saved_cursor: (usize, usize),
}

impl Buffer {
    fn new(cols: usize, rows: usize) -> Self {
        Self { lines: (0..rows).map(|_| blank_line(cols)).collect(), cursor_x: 0, cursor_y: 0, wrap_pending: false, saved_cursor: (0, 0) }
    }
}

const SCROLLBACK_LIMIT: usize = 1000;

/// Screen emulator state.
struct Emulator {
    cols: usize,
    rows: usize,
    normal: Buffer,
    alt: Option<Buffer>,
    autowrap: bool,
    cursor_visible: bool,
    /// Pending partial escape sequence.
    pending: String,
}

impl Emulator {
    fn new(cols: usize, rows: usize) -> Self {
        Self { cols, rows, normal: Buffer::new(cols, rows), alt: None, autowrap: true, cursor_visible: true, pending: String::new() }
    }

    fn buf(&mut self) -> &mut Buffer {
        match self.alt.as_mut() {
            Some(a) => a,
            None => &mut self.normal,
        }
    }

    fn buf_ref(&self) -> &Buffer {
        self.alt.as_ref().unwrap_or(&self.normal)
    }

    fn screen_start(&self) -> usize {
        self.buf_ref().lines.len() - self.rows
    }

    fn line_mut(&mut self, row: usize) -> &mut Line {
        let start = self.screen_start();
        let cols = self.cols;
        let line = &mut self.buf().lines[start + row];
        if line.len() < cols {
            line.resize(cols, Cell::default());
        }
        line
    }

    fn scroll_up(&mut self) {
        let cols = self.cols;
        let rows = self.rows;
        let is_alt = self.alt.is_some();
        let b = self.buf();
        b.lines.push(blank_line(cols));
        if is_alt || b.lines.len() > SCROLLBACK_LIMIT + rows {
            b.lines.remove(0);
        }
    }

    fn line_feed(&mut self) {
        let rows = self.rows;
        let b = self.buf();
        b.wrap_pending = false;
        if b.cursor_y + 1 >= rows {
            self.scroll_up();
        } else {
            b.cursor_y += 1;
        }
    }

    fn clear_cell(line: &mut Line, x: usize) {
        if x >= line.len() {
            return;
        }
        // Clearing half of a wide character clears both halves.
        if line[x].continuation && x > 0 {
            line[x - 1] = Cell::default();
        }
        if x + 1 < line.len() && line[x + 1].continuation {
            line[x + 1] = Cell::default();
        }
        line[x] = Cell::default();
    }

    fn print_grapheme(&mut self, g: &str) {
        let w = grapheme_width(g);
        if w == 0 {
            // Attach zero-width graphemes to the previous cell.
            let (x, y) = (self.buf_ref().cursor_x, self.buf_ref().cursor_y);
            let line = self.line_mut(y);
            let target = if x > 0 { x - 1 } else { 0 };
            if let Some(cell) = line.get_mut(target) {
                cell.text.push_str(g);
            }
            return;
        }
        let cols = self.cols;
        if self.buf_ref().wrap_pending && self.autowrap {
            self.buf().cursor_x = 0;
            self.line_feed();
        }
        let mut x = self.buf_ref().cursor_x;
        if x + w > cols {
            if self.autowrap {
                self.buf().cursor_x = 0;
                self.line_feed();
                x = 0;
            } else {
                x = cols.saturating_sub(w);
            }
        }
        let y = self.buf_ref().cursor_y;
        let line = self.line_mut(y);
        Self::clear_cell(line, x);
        if w == 2 && x + 1 < line.len() {
            Self::clear_cell(line, x + 1);
        }
        line[x] = Cell { text: g.to_string(), continuation: false };
        if w == 2 && x + 1 < line.len() {
            line[x + 1] = Cell { text: String::new(), continuation: true };
        }
        let nx = x + w;
        let b = self.buf();
        if nx >= cols {
            b.cursor_x = cols - 1;
            b.wrap_pending = true;
        } else {
            b.cursor_x = nx;
            b.wrap_pending = false;
        }
    }

    fn erase_in_line(&mut self, mode: u32) {
        let (x, y) = (self.buf_ref().cursor_x, self.buf_ref().cursor_y);
        let cols = self.cols;
        let line = self.line_mut(y);
        let (from, to) = match mode {
            0 => (x, cols),
            1 => (0, (x + 1).min(cols)),
            _ => (0, cols),
        };
        for i in from..to {
            Self::clear_cell(line, i);
        }
    }

    fn erase_in_display(&mut self, mode: u32) {
        let rows = self.rows;
        let y = self.buf_ref().cursor_y;
        match mode {
            0 => {
                self.erase_in_line(0);
                for r in y + 1..rows {
                    let cols = self.cols;
                    *self.line_mut(r) = blank_line(cols);
                }
            }
            1 => {
                self.erase_in_line(1);
                for r in 0..y {
                    let cols = self.cols;
                    *self.line_mut(r) = blank_line(cols);
                }
            }
            2 => {
                for r in 0..rows {
                    let cols = self.cols;
                    *self.line_mut(r) = blank_line(cols);
                }
            }
            3 => {
                let start = self.screen_start();
                if start > 0 {
                    self.buf().lines.drain(..start);
                }
            }
            _ => {}
        }
    }

    fn set_mode(&mut self, private: bool, params: &[u32], on: bool) {
        if !private {
            return;
        }
        for &p in params {
            match p {
                7 => self.autowrap = on,
                25 => self.cursor_visible = on,
                1049 => {
                    if on && self.alt.is_none() {
                        let b = self.buf_ref();
                        self.normal.saved_cursor = (b.cursor_x, b.cursor_y);
                        let mut alt = Buffer::new(self.cols, self.rows);
                        alt.cursor_x = self.normal.cursor_x;
                        alt.cursor_y = self.normal.cursor_y;
                        self.alt = Some(alt);
                    } else if !on && self.alt.is_some() {
                        self.alt = None;
                        let (x, y) = self.normal.saved_cursor;
                        self.normal.cursor_x = x;
                        self.normal.cursor_y = y;
                        self.normal.wrap_pending = false;
                    }
                }
                _ => {}
            }
        }
    }

    fn csi(&mut self, private: Option<char>, params_str: &str, final_char: char) {
        let params: Vec<u32> = params_str
            .split(';')
            .map(|p| p.split(':').next().unwrap_or("").parse::<u32>().unwrap_or(0))
            .collect();
        let p = |i: usize, default: u32| -> u32 {
            match params.get(i) {
                Some(&0) | None => default,
                Some(&v) => v,
            }
        };
        let rows = self.rows;
        let cols = self.cols;
        if private == Some('?') {
            match final_char {
                'h' => self.set_mode(true, &params, true),
                'l' => self.set_mode(true, &params, false),
                _ => {}
            }
            return;
        }
        if private.is_some() {
            return;
        }
        match final_char {
            'A' => {
                let n = p(0, 1) as usize;
                let b = self.buf();
                b.cursor_y = b.cursor_y.saturating_sub(n);
                b.wrap_pending = false;
            }
            'B' | 'e' => {
                let n = p(0, 1) as usize;
                let b = self.buf();
                b.cursor_y = (b.cursor_y + n).min(rows - 1);
                b.wrap_pending = false;
            }
            'C' | 'a' => {
                let n = p(0, 1) as usize;
                let b = self.buf();
                b.cursor_x = (b.cursor_x + n).min(cols - 1);
                b.wrap_pending = false;
            }
            'D' => {
                let n = p(0, 1) as usize;
                let b = self.buf();
                b.cursor_x = b.cursor_x.saturating_sub(n);
                b.wrap_pending = false;
            }
            'E' => {
                let n = p(0, 1) as usize;
                let b = self.buf();
                b.cursor_y = (b.cursor_y + n).min(rows - 1);
                b.cursor_x = 0;
                b.wrap_pending = false;
            }
            'F' => {
                let n = p(0, 1) as usize;
                let b = self.buf();
                b.cursor_y = b.cursor_y.saturating_sub(n);
                b.cursor_x = 0;
                b.wrap_pending = false;
            }
            'G' | '`' => {
                let n = p(0, 1) as usize;
                let b = self.buf();
                b.cursor_x = (n - 1).min(cols - 1);
                b.wrap_pending = false;
            }
            'd' => {
                let n = p(0, 1) as usize;
                let b = self.buf();
                b.cursor_y = (n - 1).min(rows - 1);
                b.wrap_pending = false;
            }
            'H' | 'f' => {
                let r = p(0, 1) as usize;
                let c = p(1, 1) as usize;
                let b = self.buf();
                b.cursor_y = (r - 1).min(rows - 1);
                b.cursor_x = (c - 1).min(cols - 1);
                b.wrap_pending = false;
            }
            'J' => self.erase_in_display(params.first().copied().unwrap_or(0)),
            'K' => self.erase_in_line(params.first().copied().unwrap_or(0)),
            'S' => {
                for _ in 0..p(0, 1) {
                    self.scroll_up();
                }
            }
            _ => {}
        }
    }

    fn feed(&mut self, data: &str) {
        let mut input = std::mem::take(&mut self.pending);
        input.push_str(data);
        let bytes = input.as_bytes();
        let mut i = 0;
        let mut text_start = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == 0x1b || b < 0x20 || b == 0x7f {
                if text_start < i {
                    let text = input[text_start..i].to_string();
                    for g in graphemes(&text) {
                        self.print_grapheme(g);
                    }
                }
                if b == 0x1b {
                    match self.parse_escape(&input[i..]) {
                        Some(len) => {
                            i += len;
                        }
                        None => {
                            self.pending = input[i..].to_string();
                            return;
                        }
                    }
                } else {
                    match b {
                        b'\r' => {
                            let bf = self.buf();
                            bf.cursor_x = 0;
                            bf.wrap_pending = false;
                        }
                        b'\n' | 0x0b | 0x0c => self.line_feed(),
                        0x08 => {
                            let bf = self.buf();
                            bf.cursor_x = bf.cursor_x.saturating_sub(1);
                            bf.wrap_pending = false;
                        }
                        b'\t' => {
                            let cols = self.cols;
                            let bf = self.buf();
                            let next = ((bf.cursor_x / 8) + 1) * 8;
                            bf.cursor_x = next.min(cols - 1);
                        }
                        _ => {}
                    }
                    i += 1;
                }
                text_start = i;
                continue;
            }
            i += 1;
        }
        if text_start < bytes.len() {
            let text = input[text_start..].to_string();
            for g in graphemes(&text) {
                self.print_grapheme(g);
            }
        }
    }

    /// Parse and apply an escape sequence; returns its byte length or `None` if incomplete.
    fn parse_escape(&mut self, s: &str) -> Option<usize> {
        let bytes = s.as_bytes();
        let next = *bytes.get(1)?;
        match next {
            b'[' => {
                let mut j = 2;
                let private = match bytes.get(j) {
                    Some(&c @ (b'?' | b'>' | b'<' | b'=')) => {
                        j += 1;
                        Some(c as char)
                    }
                    _ => None,
                };
                let params_start = j;
                while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';' || bytes[j] == b':') {
                    j += 1;
                }
                // Intermediate bytes.
                while j < bytes.len() && (0x20..=0x2f).contains(&bytes[j]) {
                    j += 1;
                }
                let final_byte = *bytes.get(j)?;
                let params = &s[params_start..j];
                let params = params.trim_end_matches(|c: char| (' '..='/').contains(&c));
                self.csi(private, params, final_byte as char);
                Some(j + 1)
            }
            b']' | b'_' | b'P' | b'^' | b'X' => {
                let mut j = 2;
                while j < bytes.len() {
                    if bytes[j] == 0x07 {
                        return Some(j + 1);
                    }
                    if bytes[j] == 0x1b {
                        if j + 1 >= bytes.len() {
                            return None;
                        }
                        if bytes[j + 1] == b'\\' {
                            return Some(j + 2);
                        }
                    }
                    j += 1;
                }
                None
            }
            b'7' => {
                let b = self.buf();
                b.saved_cursor = (b.cursor_x, b.cursor_y);
                Some(2)
            }
            b'8' => {
                let b = self.buf();
                let (x, y) = b.saved_cursor;
                b.cursor_x = x;
                b.cursor_y = y;
                Some(2)
            }
            b'(' | b')' => {
                if bytes.len() < 3 {
                    None
                } else {
                    Some(3)
                }
            }
            b'M' => {
                let b = self.buf();
                b.cursor_y = b.cursor_y.saturating_sub(1);
                Some(2)
            }
            _ => Some(2),
        }
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        let old_rows = self.rows;
        for buf in std::iter::once(&mut self.normal).chain(self.alt.iter_mut()) {
            for line in buf.lines.iter_mut() {
                line.resize(cols, Cell::default());
                if let Some(last) = line.last_mut() {
                    if !last.continuation && grapheme_width(&last.text) == 2 {
                        *last = Cell::default();
                    }
                }
            }
            if rows < old_rows {
                // Remove empty rows below the cursor first, then push rows into scrollback.
                let mut remove = old_rows - rows;
                let total = buf.lines.len();
                let screen_start = total - old_rows;
                let mut bottom = total;
                while remove > 0 && bottom > screen_start + buf.cursor_y + 1 {
                    let idx = bottom - 1;
                    if buf.lines[idx].iter().all(|c| c.text.is_empty()) {
                        buf.lines.remove(idx);
                        remove -= 1;
                        bottom -= 1;
                    } else {
                        break;
                    }
                }
                buf.cursor_y = buf.cursor_y.saturating_sub(remove);
            } else if rows > old_rows {
                let add = rows - old_rows;
                let scrollback = buf.lines.len() - old_rows;
                let pull = add.min(scrollback);
                buf.cursor_y += pull;
                for _ in 0..(add - pull) {
                    buf.lines.push(blank_line(cols));
                }
            }
            buf.cursor_x = buf.cursor_x.min(cols.saturating_sub(1));
            buf.cursor_y = buf.cursor_y.min(rows.saturating_sub(1));
            buf.wrap_pending = false;
        }
        self.cols = cols;
        self.rows = rows;
    }
}

struct VtState {
    emu: Emulator,
    input_handler: Option<InputHandler>,
    resize_handler: Option<ResizeHandler>,
    writes: Vec<String>,
    log_writes: bool,
    kitty_protocol_active: bool,
    title: Option<String>,
}

/// Virtual terminal for tests, backed by a small terminal emulator.
#[derive(Clone)]
pub struct VirtualTerminal {
    state: Arc<Mutex<VtState>>,
}

impl VirtualTerminal {
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(VtState {
                emu: Emulator::new(columns, rows),
                input_handler: None,
                resize_handler: None,
                writes: Vec::new(),
                log_writes: true,
                kitty_protocol_active: true,
                title: None,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, VtState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The last window title set through `set_title`.
    pub fn get_title(&self) -> Option<String> {
        self.lock().title.clone()
    }

    /// Simulate keyboard input.
    pub fn send_input(&self, data: &str) {
        let handler = self.lock().input_handler.clone();
        if let Some(h) = handler {
            h(data.to_string());
        }
    }

    /// Resize the terminal and notify the resize handler.
    pub fn resize(&self, columns: usize, rows: usize) {
        let handler = {
            let mut st = self.lock();
            st.emu.resize(columns, rows);
            st.resize_handler.clone()
        };
        if let Some(h) = handler {
            h();
        }
    }

    /// The visible screen rows (right-trimmed).
    pub fn get_viewport(&self) -> Vec<String> {
        let st = self.lock();
        let b = st.emu.buf_ref();
        let start = b.lines.len() - st.emu.rows;
        b.lines[start..].iter().map(line_to_string).collect()
    }

    /// Scrollback plus screen rows (right-trimmed).
    pub fn get_scroll_buffer(&self) -> Vec<String> {
        let st = self.lock();
        st.emu.buf_ref().lines.iter().map(line_to_string).collect()
    }

    /// Cursor position (x, y) relative to the screen.
    pub fn get_cursor_position(&self) -> (usize, usize) {
        let st = self.lock();
        let b = st.emu.buf_ref();
        (b.cursor_x, b.cursor_y)
    }

    pub fn is_cursor_visible(&self) -> bool {
        self.lock().emu.cursor_visible
    }

    /// All data written since creation or the last [`clear_writes`](Self::clear_writes).
    pub fn get_writes(&self) -> String {
        self.lock().writes.concat()
    }

    /// Individual write calls.
    pub fn write_calls(&self) -> Vec<String> {
        self.lock().writes.clone()
    }

    pub fn clear_writes(&self) {
        self.lock().writes.clear();
    }

    /// Clear the screen and scrollback.
    pub fn reset(&self) {
        let mut st = self.lock();
        let (c, r) = (st.emu.cols, st.emu.rows);
        st.emu = Emulator::new(c, r);
    }

    pub fn set_kitty_protocol_active(&self, active: bool) {
        self.lock().kitty_protocol_active = active;
    }

    pub fn set_log_writes(&self, enabled: bool) {
        self.lock().log_writes = enabled;
    }
}

impl Terminal for VirtualTerminal {
    fn start(&self, on_input: InputHandler, on_resize: ResizeHandler) {
        let mut st = self.lock();
        st.input_handler = Some(on_input);
        st.resize_handler = Some(on_resize);
        st.emu.feed("\x1b[?2004h");
    }

    fn stop(&self) {
        let mut st = self.lock();
        st.emu.feed("\x1b[?2004l");
        st.input_handler = None;
        st.resize_handler = None;
    }

    fn write(&self, data: &str) {
        let mut st = self.lock();
        if st.log_writes {
            st.writes.push(data.to_string());
        }
        st.emu.feed(data);
    }

    fn columns(&self) -> usize {
        self.lock().emu.cols
    }

    fn rows(&self) -> usize {
        self.lock().emu.rows
    }

    fn kitty_protocol_active(&self) -> bool {
        self.lock().kitty_protocol_active
    }

    // Like the TypeScript helper, these go straight to the emulator and are not logged as writes.
    fn move_by(&self, lines: i64) {
        if lines > 0 {
            self.lock().emu.feed(&format!("\x1b[{lines}B"));
        } else if lines < 0 {
            self.lock().emu.feed(&format!("\x1b[{}A", -lines));
        }
    }
    fn hide_cursor(&self) {
        self.lock().emu.feed("\x1b[?25l");
    }
    fn show_cursor(&self) {
        self.lock().emu.feed("\x1b[?25h");
    }
    fn clear_line(&self) {
        self.lock().emu.feed("\x1b[K");
    }
    fn clear_from_cursor(&self) {
        self.lock().emu.feed("\x1b[J");
    }
    fn clear_screen(&self) {
        self.lock().emu.feed("\x1b[2J\x1b[H");
    }
    fn set_title(&self, title: &str) {
        let mut st = self.lock();
        st.title = Some(title.to_string());
        st.emu.feed(&format!("\x1b]0;{title}\x07"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emulator_basics() {
        let t = VirtualTerminal::new(10, 3);
        t.write("hello\r\nworld");
        assert_eq!(t.get_viewport(), vec!["hello", "world", ""]);
        t.write("\r\nthree\r\nfour");
        assert_eq!(t.get_viewport(), vec!["world", "three", "four"]);
        assert_eq!(t.get_scroll_buffer(), vec!["hello", "world", "three", "four"]);
        t.write("\x1b[2A\r\x1b[2Kxx");
        assert_eq!(t.get_viewport(), vec!["xx", "three", "four"]);
        t.write("\x1b[2J\x1b[H\x1b[3J");
        assert_eq!(t.get_scroll_buffer(), vec!["", "", ""]);
        t.write("日本語abcdefg");
        assert_eq!(t.get_viewport(), vec!["日本語abcd", "efg", ""]);
    }
}
