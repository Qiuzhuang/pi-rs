//! Main-screen renderer: differential rendering into the terminal's main screen and
//! scrollback. Port of `packages/tui/src/tui-main-screen.ts`.

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard, Once, Weak};

use crate::terminal_image::{delete_kitty_image, is_image_line};
use crate::tui::{
    Screen, Tui, TuiStopOptions, apply_line_resets, extract_cursor_position, resolve_fake_cursors,
};
use crate::utils::visible_width;

const KITTY_SEQUENCE_PREFIX: &str = "\x1b_G";
const MAX_RENDER_WRITE_CHARS: usize = 1024 * 1024;

/// Streams terminal output in bounded chunks (1 MiB of UTF-16 units, like the TS version).
struct BoundedTerminalWriter<'a> {
    buffer: String,
    buffer_units: usize,
    written: usize,
    write: &'a dyn Fn(&str),
}

impl<'a> BoundedTerminalWriter<'a> {
    fn new(write: &'a dyn Fn(&str)) -> Self {
        Self { buffer: String::new(), buffer_units: 0, written: 0, write }
    }

    fn append(&mut self, value: &str) {
        for ch in value.chars() {
            let units = ch.len_utf16();
            if self.buffer_units + units > MAX_RENDER_WRITE_CHARS {
                self.flush();
            }
            self.buffer.push(ch);
            self.buffer_units += units;
            if self.buffer_units == MAX_RENDER_WRITE_CHARS {
                self.flush();
            }
        }
    }

    fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        (self.write)(&self.buffer);
        self.written += self.buffer_units;
        self.buffer.clear();
        self.buffer_units = 0;
    }

    fn len(&self) -> usize {
        self.written + self.buffer_units
    }
}

struct KittyImageHeader {
    ids: Vec<u32>,
    rows: usize,
}

fn parse_kitty_image_header(line: &str) -> Option<KittyImageHeader> {
    let start = line.find(KITTY_SEQUENCE_PREFIX)?;
    let params_start = start + KITTY_SEQUENCE_PREFIX.len();
    let params_end = line[params_start..].find(';')? + params_start;
    let mut ids = Vec::new();
    let mut rows = 1usize;
    for param in line[params_start..params_end].split(',') {
        let mut it = param.splitn(2, '=');
        let key = it.next().unwrap_or("");
        let Some(value) = it.next() else { continue };
        // Number(value): integer in (0, 0xffffffff]
        let Ok(n) = value.trim().parse::<f64>() else { continue };
        if n.fract() != 0.0 || n <= 0.0 || n > 0xffffffffu32 as f64 || value.trim().is_empty() {
            continue;
        }
        if key == "i" {
            ids.push(n as u32);
        } else if key == "r" {
            rows = n as usize;
        }
    }
    Some(KittyImageHeader { ids, rows })
}

fn extract_kitty_image_ids(line: &str) -> Vec<u32> {
    parse_kitty_image_header(line).map(|h| h.ids).unwrap_or_default()
}

fn extract_kitty_image_rows(line: &str) -> usize {
    parse_kitty_image_header(line).map_or(1, |h| h.rows)
}

fn is_termux_session() -> bool {
    std::env::var_os("TERMUX_VERSION").is_some_and(|v| !v.is_empty())
}

/// Snapshot of the main-screen render state (see [`Tui::capture_render_state`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TuiMainScreenRenderState {
    pub previous_lines: Vec<String>,
    pub previous_width: i64,
    pub previous_height: i64,
    pub cursor_row: i64,
    pub hardware_cursor_row: i64,
    pub max_lines_rendered: i64,
    pub previous_viewport_top: i64,
}

#[derive(Debug, Default)]
pub(crate) struct MainRenderState {
    previous_lines: Vec<String>,
    previous_kitty_image_ids: BTreeSet<u32>,
    previous_width: i64,
    previous_height: i64,
    cursor_row: i64,
    hardware_cursor_row: i64,
    max_lines_rendered: i64,
    previous_viewport_top: i64,
}

fn collect_kitty_image_ids(lines: &[String]) -> BTreeSet<u32> {
    let mut ids = BTreeSet::new();
    for line in lines {
        ids.extend(extract_kitty_image_ids(line));
    }
    ids
}

fn delete_kitty_images<'a>(ids: impl IntoIterator<Item = &'a u32>) -> String {
    ids.into_iter().map(|id| delete_kitty_image(*id)).collect()
}

fn kitty_image_reserved_rows(lines: &[String], index: usize, max_index: Option<usize>) -> usize {
    let rows = extract_kitty_image_rows(lines.get(index).map(String::as_str).unwrap_or(""));
    if rows <= 1 {
        return 1;
    }
    let max_index = max_index.unwrap_or(lines.len().saturating_sub(1));
    let max_rows = rows.min((max_index + 1).saturating_sub(index)).min(lines.len() - index);
    let mut reserved = 1;
    while reserved < max_rows {
        let line = lines.get(index + reserved).map(String::as_str).unwrap_or("");
        if is_image_line(line) || visible_width(line) > 0 {
            break;
        }
        reserved += 1;
    }
    reserved
}

static EXIT_HOOK: Once = Once::new();
static ACTIVE_TUIS: Mutex<Vec<Weak<Tui>>> = Mutex::new(Vec::new());

extern "C" fn restore_active_tuis_at_exit() {
    let tuis: Vec<Weak<Tui>> = match ACTIVE_TUIS.try_lock() {
        Ok(g) => g.clone(),
        Err(_) => return,
    };
    for weak in tuis {
        if let Some(tui) = weak.upgrade() {
            tui.restore_on_exit();
        }
    }
}

/// Register a TUI for last-resort terminal restoration at process exit.
pub(crate) fn register_exit_restore(tui: Weak<Tui>) {
    EXIT_HOOK.call_once(|| {
        #[cfg(unix)]
        unsafe {
            libc::atexit(restore_active_tuis_at_exit);
        }
    });
    let mut active = ACTIVE_TUIS.lock().unwrap_or_else(|e| e.into_inner());
    active.retain(|w| w.strong_count() > 0 && !w.ptr_eq(&tui));
    active.push(tui);
}

impl Tui {
    fn main_state(&self) -> MutexGuard<'_, MainRenderState> {
        match &self.screen {
            Screen::Main(m) => m.lock().unwrap_or_else(|e| e.into_inner()),
            Screen::Alt(_) => panic!("main-screen state requested on a fullscreen TUI"),
        }
    }

    /// Snapshot the main-screen render state (regular mode only).
    pub fn capture_render_state(&self) -> TuiMainScreenRenderState {
        let s = self.main_state();
        TuiMainScreenRenderState {
            previous_lines: s.previous_lines.clone(),
            previous_width: s.previous_width,
            previous_height: s.previous_height,
            cursor_row: s.cursor_row,
            hardware_cursor_row: s.hardware_cursor_row,
            max_lines_rendered: s.max_lines_rendered,
            previous_viewport_top: s.previous_viewport_top,
        }
    }

    /// Restore a captured main-screen render state (image lines are dropped).
    pub fn restore_render_state(&self, state: &TuiMainScreenRenderState) {
        let mut s = self.main_state();
        s.previous_lines =
            state.previous_lines.iter().map(|l| if is_image_line(l) { String::new() } else { l.clone() }).collect();
        s.previous_kitty_image_ids.clear();
        s.previous_width = state.previous_width;
        s.previous_height = state.previous_height;
        s.cursor_row = state.cursor_row;
        s.hardware_cursor_row = state.hardware_cursor_row;
        s.max_lines_rendered = state.max_lines_rendered;
        s.previous_viewport_top = state.previous_viewport_top;
    }

    pub(crate) fn main_reset_render_state(&self) {
        let mut s = self.main_state();
        s.previous_lines.clear();
        s.previous_width = -1;
        s.previous_height = -1;
        s.cursor_row = 0;
        s.hardware_cursor_row = 0;
        s.max_lines_rendered = 0;
        s.previous_viewport_top = 0;
    }

    pub(crate) fn main_before_terminal_stop(&self, options: TuiStopOptions) {
        let s = self.main_state();
        if options.preserve_screen || s.previous_lines.is_empty() {
            return;
        }
        let terminal = self.terminal();
        terminal.write(" ");
        let target_row = s.previous_lines.len() as i64;
        let line_diff = target_row - s.hardware_cursor_row;
        if line_diff > 0 {
            terminal.write(&format!("\x1b[{line_diff}B"));
        } else if line_diff < 0 {
            terminal.write(&format!("\x1b[{}A", -line_diff));
        }
        terminal.write("\r\n");
    }

    fn expand_changed_range_for_kitty_images(
        previous: &[String],
        first_changed: i64,
        last_changed: i64,
        new_lines: &[String],
    ) -> (i64, i64) {
        let mut first = first_changed;
        let mut last = last_changed;
        let mut expand = |lines: &[String]| {
            for i in 0..lines.len() {
                if extract_kitty_image_ids(&lines[i]).is_empty() {
                    continue;
                }
                let block_end = (i + kitty_image_reserved_rows(lines, i, None) - 1) as i64;
                let i64i = i as i64;
                if i64i >= first_changed || (i64i <= last_changed && block_end >= first_changed) {
                    first = first.min(i64i);
                    last = last.max(block_end);
                }
            }
        };
        expand(previous);
        expand(new_lines);
        (first, last)
    }

    fn delete_changed_kitty_images(previous: &[String], first_changed: i64, last_changed: i64) -> String {
        if first_changed < 0 || last_changed < first_changed {
            return String::new();
        }
        let mut ids = BTreeSet::new();
        let max_line = last_changed.min(previous.len() as i64 - 1);
        let mut i = first_changed;
        while i <= max_line {
            ids.extend(extract_kitty_image_ids(&previous[i as usize]));
            i += 1;
        }
        delete_kitty_images(ids.iter())
    }

    fn position_hardware_cursor(&self, s: &mut MainRenderState, cursor_pos: Option<(usize, usize)>, total_lines: usize) {
        let terminal = self.terminal();
        let Some((row, col)) = cursor_pos.filter(|_| total_lines > 0) else {
            terminal.hide_cursor();
            return;
        };
        let target_row = (row as i64).clamp(0, total_lines as i64 - 1);
        let target_col = col as i64;
        let row_delta = target_row - s.hardware_cursor_row;
        let mut buffer = String::new();
        if row_delta > 0 {
            buffer.push_str(&format!("\x1b[{row_delta}B"));
        } else if row_delta < 0 {
            buffer.push_str(&format!("\x1b[{}A", -row_delta));
        }
        buffer.push_str(&format!("\x1b[{}G", target_col + 1));
        terminal.write(&buffer);
        s.hardware_cursor_row = target_row;
        if self.get_show_hardware_cursor() {
            terminal.show_cursor();
        } else {
            terminal.hide_cursor();
        }
    }

    fn log_redraw(&self, reason: &str, prev: usize, new: usize, height: usize) {
        if std::env::var("PI_TUI_DEBUG_REDRAW").as_deref() != Ok("1") {
            return;
        }
        let Some(dir) = &self.log_directory else { return };
        let path = dir.join("pi-tui-debug.log");
        let _ = std::fs::create_dir_all(dir);
        let msg = format!(
            "[{}] fullRender: {reason} (prev={prev}, new={new}, height={height})\n",
            iso_now()
        );
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(msg.as_bytes());
        }
    }

    pub(crate) fn main_do_render(&self) {
        if self.is_stopped() {
            return;
        }
        let terminal = self.terminal().clone();
        let width = terminal.columns();
        let height = terminal.rows();

        // Render components and overlays before taking the render-state lock: component code may
        // re-enter the TUI (e.g. `request_render(true)` resets the render state).
        let show_hw = self.get_show_hardware_cursor();
        let mut new_lines = resolve_fake_cursors(self.render_root(width), show_hw);
        if self.has_overlay_entries() {
            new_lines = self.composite_overlays(new_lines, width, height);
        }
        let cursor_pos = extract_cursor_position(&mut new_lines, height);
        apply_line_resets(&mut new_lines);

        let mut s = self.main_state();
        let (w, h) = (width as i64, height as i64);
        let width_changed = s.previous_width != 0 && s.previous_width != w;
        let height_changed = s.previous_height != 0 && s.previous_height != h;
        let previous_buffer_length =
            if s.previous_height > 0 { s.previous_viewport_top + s.previous_height } else { h };
        let mut prev_viewport_top =
            if height_changed { (previous_buffer_length - h).max(0) } else { s.previous_viewport_top };
        let mut viewport_top = prev_viewport_top;
        let mut hardware_cursor_row = s.hardware_cursor_row;

        let write = |d: &str| terminal.write(d);

        let full_render = |this: &Tui, s: &mut MainRenderState, new_lines: Vec<String>, clear: bool| {
            this.full_redraw_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut output = BoundedTerminalWriter::new(&write);
            output.append("\x1b[?2026h");
            if clear {
                output.append(&delete_kitty_images(s.previous_kitty_image_ids.iter()));
                output.append("\x1b[2J\x1b[H\x1b[3J");
            }
            let mut i = 0;
            while i < new_lines.len() {
                if i > 0 {
                    output.append("\r\n");
                }
                let line = &new_lines[i];
                let is_image = is_image_line(line);
                let reserved = if is_image { kitty_image_reserved_rows(&new_lines, i, None) } else { 1 };
                if reserved > 1 && reserved <= height {
                    for _ in 1..reserved {
                        output.append("\r\n");
                    }
                    output.append(&format!("\x1b[{}A", reserved - 1));
                    output.append(line);
                    output.append(&format!("\x1b[{}B", reserved - 1));
                    i += reserved;
                    continue;
                }
                output.append(line);
                i += 1;
            }
            output.append("\x1b[?2026l");
            output.flush();
            s.cursor_row = (new_lines.len() as i64 - 1).max(0);
            s.hardware_cursor_row = s.cursor_row;
            if clear {
                s.max_lines_rendered = new_lines.len() as i64;
            } else {
                s.max_lines_rendered = s.max_lines_rendered.max(new_lines.len() as i64);
            }
            let buffer_length = h.max(new_lines.len() as i64);
            s.previous_viewport_top = (buffer_length - h).max(0);
            this.position_hardware_cursor(s, cursor_pos, new_lines.len());
            s.previous_kitty_image_ids = collect_kitty_image_ids(&new_lines);
            s.previous_lines = new_lines;
            s.previous_width = w;
            s.previous_height = h;
        };

        let prev_len = s.previous_lines.len();
        if s.previous_lines.is_empty() && !width_changed && !height_changed {
            self.log_redraw("first render", prev_len, new_lines.len(), height);
            full_render(self, &mut s, new_lines, false);
            return;
        }
        if width_changed {
            self.log_redraw(&format!("terminal width changed ({} -> {w})", s.previous_width), prev_len, new_lines.len(), height);
            full_render(self, &mut s, new_lines, true);
            return;
        }
        if height_changed && !is_termux_session() {
            self.log_redraw(&format!("terminal height changed ({} -> {h})", s.previous_height), prev_len, new_lines.len(), height);
            full_render(self, &mut s, new_lines, true);
            return;
        }
        if self.get_clear_on_shrink() && (new_lines.len() as i64) < s.max_lines_rendered && !self.has_overlay_entries() {
            self.log_redraw(&format!("clearOnShrink (maxLinesRendered={})", s.max_lines_rendered), prev_len, new_lines.len(), height);
            full_render(self, &mut s, new_lines, true);
            return;
        }

        let mut first_changed: i64 = -1;
        let mut last_changed: i64 = -1;
        let max_lines = new_lines.len().max(s.previous_lines.len());
        for i in 0..max_lines {
            let old = s.previous_lines.get(i).map(String::as_str).unwrap_or("");
            let new = new_lines.get(i).map(String::as_str).unwrap_or("");
            if old != new {
                if first_changed == -1 {
                    first_changed = i as i64;
                }
                last_changed = i as i64;
            }
        }
        let appended_lines = new_lines.len() > s.previous_lines.len();
        if appended_lines {
            if first_changed == -1 {
                first_changed = s.previous_lines.len() as i64;
            }
            last_changed = new_lines.len() as i64 - 1;
        }
        if first_changed != -1 {
            let (f, l) =
                Self::expand_changed_range_for_kitty_images(&s.previous_lines, first_changed, last_changed, &new_lines);
            first_changed = f;
            last_changed = l;
        }
        let append_start = appended_lines && first_changed == s.previous_lines.len() as i64 && first_changed > 0;

        if first_changed == -1 {
            self.position_hardware_cursor(&mut s, cursor_pos, new_lines.len());
            s.previous_viewport_top = prev_viewport_top;
            s.previous_height = h;
            return;
        }

        let compute_line_diff = |hardware_cursor_row: i64, prev_viewport_top: i64, viewport_top: i64, target: i64| {
            let current_screen_row = hardware_cursor_row - prev_viewport_top;
            let target_screen_row = target - viewport_top;
            target_screen_row - current_screen_row
        };

        if first_changed >= new_lines.len() as i64 {
            if s.previous_lines.len() > new_lines.len() {
                let mut output = BoundedTerminalWriter::new(&write);
                output.append("\x1b[?2026h");
                output.append(&Self::delete_changed_kitty_images(&s.previous_lines, first_changed, last_changed));
                let target_row = (new_lines.len() as i64 - 1).max(0);
                if target_row < prev_viewport_top {
                    self.log_redraw(
                        &format!("deleted lines moved viewport up ({target_row} < {prev_viewport_top})"),
                        prev_len,
                        new_lines.len(),
                        height,
                    );
                    full_render(self, &mut s, new_lines, true);
                    return;
                }
                let line_diff = compute_line_diff(hardware_cursor_row, prev_viewport_top, viewport_top, target_row);
                if line_diff > 0 {
                    output.append(&format!("\x1b[{line_diff}B"));
                } else if line_diff < 0 {
                    output.append(&format!("\x1b[{}A", -line_diff));
                }
                output.append("\r");
                let extra_lines = s.previous_lines.len() - new_lines.len();
                if extra_lines > height {
                    self.log_redraw(&format!("extraLines > height ({extra_lines} > {height})"), prev_len, new_lines.len(), height);
                    full_render(self, &mut s, new_lines, true);
                    return;
                }
                let clear_start_offset = if new_lines.is_empty() { 0 } else { 1 };
                if extra_lines > 0 && clear_start_offset > 0 {
                    output.append(&format!("\x1b[{clear_start_offset}B"));
                }
                for i in 0..extra_lines {
                    output.append("\r\x1b[2K");
                    if i + 1 < extra_lines {
                        output.append("\x1b[1B");
                    }
                }
                let move_back = (extra_lines as i64 - 1 + clear_start_offset).max(0);
                if move_back > 0 {
                    output.append(&format!("\x1b[{move_back}A"));
                }
                output.append("\x1b[?2026l");
                output.flush();
                s.cursor_row = target_row;
                s.hardware_cursor_row = target_row;
            }
            self.position_hardware_cursor(&mut s, cursor_pos, new_lines.len());
            s.previous_kitty_image_ids = collect_kitty_image_ids(&new_lines);
            s.previous_lines = new_lines;
            s.previous_width = w;
            s.previous_height = h;
            s.previous_viewport_top = prev_viewport_top;
            return;
        }

        if first_changed < prev_viewport_top {
            self.log_redraw(
                &format!("firstChanged < viewportTop ({first_changed} < {prev_viewport_top})"),
                prev_len,
                new_lines.len(),
                height,
            );
            full_render(self, &mut s, new_lines, true);
            return;
        }

        let mut output = BoundedTerminalWriter::new(&write);
        output.append("\x1b[?2026h");
        output.append(&Self::delete_changed_kitty_images(&s.previous_lines, first_changed, last_changed));
        let prev_viewport_bottom = prev_viewport_top + h - 1;
        let move_target_row = if append_start { first_changed - 1 } else { first_changed };
        if move_target_row > prev_viewport_bottom {
            let current_screen_row = (hardware_cursor_row - prev_viewport_top).clamp(0, h - 1);
            let move_to_bottom = h - 1 - current_screen_row;
            if move_to_bottom > 0 {
                output.append(&format!("\x1b[{move_to_bottom}B"));
            }
            let scroll = move_target_row - prev_viewport_bottom;
            output.append(&"\r\n".repeat(scroll as usize));
            prev_viewport_top += scroll;
            viewport_top += scroll;
            hardware_cursor_row = move_target_row;
        }

        let line_diff = compute_line_diff(hardware_cursor_row, prev_viewport_top, viewport_top, move_target_row);
        if line_diff > 0 {
            output.append(&format!("\x1b[{line_diff}B"));
        } else if line_diff < 0 {
            output.append(&format!("\x1b[{}A", -line_diff));
        }
        output.append(if append_start { "\r\n" } else { "\r" });

        let render_end = last_changed.min(new_lines.len() as i64 - 1);
        let mut i = first_changed;
        while i <= render_end {
            if i > first_changed {
                output.append("\r\n");
            }
            let idx = i as usize;
            let line = &new_lines[idx];
            let is_image = is_image_line(line);
            let reserved =
                if is_image { kitty_image_reserved_rows(&new_lines, idx, Some(render_end as usize)) } else { 1 };
            if reserved > 1 {
                let image_start_screen_row = i - viewport_top;
                if image_start_screen_row < 0 || image_start_screen_row + reserved as i64 > h {
                    self.log_redraw(
                        &format!("kitty image pre-clear would scroll ({image_start_screen_row} + {reserved} > {height})"),
                        prev_len,
                        new_lines.len(),
                        height,
                    );
                    drop(output);
                    full_render(self, &mut s, new_lines, true);
                    return;
                }
                output.append("\x1b[2K");
                for _ in 1..reserved {
                    output.append("\r\n\x1b[2K");
                }
                output.append(&format!("\x1b[{}A", reserved - 1));
                output.append(line);
                output.append(&format!("\x1b[{}B", reserved - 1));
                i += reserved as i64;
                continue;
            }
            output.append("\x1b[2K");
            if !is_image && visible_width(line) > width {
                let crash_dir = self.log_directory.clone().unwrap_or_else(std::env::temp_dir);
                let crash_log_path = crash_dir.join("pi-tui-crash.log");
                let mut crash_data = vec![
                    format!("Crash at {}", iso_now()),
                    format!("Terminal width: {width}"),
                    format!("Line {i} visible width: {}", visible_width(line)),
                    String::new(),
                    "=== All rendered lines ===".to_string(),
                ];
                for (idx, l) in new_lines.iter().enumerate() {
                    crash_data.push(format!("[{idx}] (w={}) {l}", visible_width(l)));
                }
                crash_data.push(String::new());
                let _ = std::fs::create_dir_all(&crash_dir);
                let _ = std::fs::write(&crash_log_path, crash_data.join("\n"));
                let error_msg = [
                    format!("Rendered line {i} exceeds terminal width ({} > {width}).", visible_width(line)),
                    String::new(),
                    "This is likely caused by a custom TUI component not truncating its output.".to_string(),
                    "Use visibleWidth() to measure and truncateToWidth() to truncate lines.".to_string(),
                    String::new(),
                    format!("Debug log written to: {}", crash_log_path.display()),
                ]
                .join("\n");
                drop(output);
                drop(s);
                self.stop(TuiStopOptions::default());
                panic!("{error_msg}");
            }
            output.append(line);
            i += 1;
        }

        let mut final_cursor_row = render_end;
        if s.previous_lines.len() > new_lines.len() {
            if render_end < new_lines.len() as i64 - 1 {
                let move_down = new_lines.len() as i64 - 1 - render_end;
                output.append(&format!("\x1b[{move_down}B"));
                final_cursor_row = new_lines.len() as i64 - 1;
            }
            let extra_lines = s.previous_lines.len() - new_lines.len();
            for _ in new_lines.len()..s.previous_lines.len() {
                output.append("\r\n\x1b[2K");
            }
            output.append(&format!("\x1b[{extra_lines}A"));
        }
        output.append("\x1b[?2026l");

        if std::env::var("PI_TUI_DEBUG").as_deref() == Ok("1") {
            let dir = std::path::Path::new("/tmp/tui");
            let _ = std::fs::create_dir_all(dir);
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
            let path = dir.join(format!("render-{}-{nanos:x}.log", nanos / 1_000_000));
            let data = [
                format!("firstChanged: {first_changed}"),
                format!("viewportTop: {viewport_top}"),
                format!("cursorRow: {}", s.cursor_row),
                format!("height: {height}"),
                format!("lineDiff: {line_diff}"),
                format!("hardwareCursorRow: {hardware_cursor_row}"),
                format!("renderEnd: {render_end}"),
                format!("finalCursorRow: {final_cursor_row}"),
                format!("cursorPos: {cursor_pos:?}"),
                format!("newLines.length: {}", new_lines.len()),
                format!("previousLines.length: {}", s.previous_lines.len()),
                String::new(),
                "=== newLines ===".to_string(),
                format!("{new_lines:#?}"),
                String::new(),
                "=== previousLines ===".to_string(),
                format!("{:#?}", s.previous_lines),
                String::new(),
                "=== buffer ===".to_string(),
                format!("[{} chars written in bounded chunks]", output.len()),
            ]
            .join("\n");
            let _ = std::fs::write(path, data);
        }
        output.flush();

        s.cursor_row = (new_lines.len() as i64 - 1).max(0);
        s.hardware_cursor_row = final_cursor_row;
        s.max_lines_rendered = s.max_lines_rendered.max(new_lines.len() as i64);
        s.previous_viewport_top = prev_viewport_top.max(final_cursor_row - h + 1);
        self.position_hardware_cursor(&mut s, cursor_pos, new_lines.len());
        s.previous_kitty_image_ids = collect_kitty_image_ids(&new_lines);
        s.previous_lines = new_lines;
        s.previous_width = w;
        s.previous_height = h;
    }
}

/// Current UTC time as an ISO-8601 string (like `new Date().toISOString()`).
pub(crate) fn iso_now() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let ms = d.subsec_millis();
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z")
}
