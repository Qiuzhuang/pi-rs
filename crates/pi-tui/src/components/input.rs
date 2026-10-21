//! Single-line text input with horizontal scrolling. Port of `components/input.ts`.
//!
//! Cursor positions are byte offsets into the value.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::StyleFn;
use crate::keybindings::get_keybindings;
use crate::keys::decode_kitty_printable;
use crate::kill_ring::KillRing;
use crate::tui::{Component, CURSOR_MARKER, MouseButton, MouseEvent, MouseEventType, MouseResult, render_fake_cursor};
use crate::undo_stack::UndoStack;
use crate::utils::{grapheme_indices, graphemes, is_whitespace_char, slice_by_column, truncate_to_width, visible_width};
use crate::word_navigation::{WordNavigationOptions, find_word_backward, find_word_forward};

#[derive(Clone)]
struct InputState {
    value: String,
    cursor: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LastAction {
    Kill,
    Yank,
    TypeWord,
}

/// Options for [`Input`].
#[derive(Clone, Default)]
pub struct InputOptions {
    pub prompt: Option<String>,
    pub placeholder: Option<String>,
    pub placeholder_style: Option<StyleFn>,
}

struct State {
    value: String,
    cursor: usize,
    rendered_start_column: usize,
    focused: bool,
    paste_buffer: String,
    is_in_paste: bool,
    kill_ring: KillRing,
    last_action: Option<LastAction>,
    undo: UndoStack<InputState>,
}

/// Submit callback.
pub type SubmitFn = Arc<dyn Fn(String) + Send + Sync>;
/// Escape callback.
pub type EscapeFn = Arc<dyn Fn() + Send + Sync>;

/// Single-line text input with horizontal scrolling.
pub struct Input {
    prompt: String,
    placeholder: String,
    placeholder_style: StyleFn,
    st: Mutex<State>,
    on_submit: Mutex<Option<SubmitFn>>,
    on_escape: Mutex<Option<EscapeFn>>,
}

impl Default for Input {
    fn default() -> Self {
        Self::new(InputOptions::default())
    }
}

enum Callback {
    None,
    Submit(SubmitFn, String),
    Escape(EscapeFn),
}

impl Input {
    pub fn new(options: InputOptions) -> Self {
        Self {
            prompt: options.prompt.unwrap_or_else(|| "> ".into()),
            placeholder: options.placeholder.unwrap_or_default(),
            placeholder_style: options.placeholder_style.unwrap_or_else(|| Arc::new(|t: &str| t.to_string())),
            st: Mutex::new(State {
                value: String::new(),
                cursor: 0,
                rendered_start_column: 0,
                focused: false,
                paste_buffer: String::new(),
                is_in_paste: false,
                kill_ring: KillRing::new(),
                last_action: None,
                undo: UndoStack::new(),
            }),
            on_submit: Mutex::new(None),
            on_escape: Mutex::new(None),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get_value(&self) -> String {
        self.lock().value.clone()
    }

    pub fn set_value(&self, value: impl Into<String>) {
        let mut s = self.lock();
        s.value = value.into();
        let mut c = s.cursor.min(s.value.len());
        while !s.value.is_char_boundary(c) {
            c -= 1;
        }
        s.cursor = c;
    }

    /// Cursor position (byte offset).
    pub fn cursor(&self) -> usize {
        self.lock().cursor
    }

    pub fn set_on_submit(&self, f: Option<SubmitFn>) {
        *self.on_submit.lock().unwrap_or_else(|e| e.into_inner()) = f;
    }

    pub fn set_on_escape(&self, f: Option<EscapeFn>) {
        *self.on_escape.lock().unwrap_or_else(|e| e.into_inner()) = f;
    }

    fn handle_input_locked(&self, s: &mut State, data: &str) -> Callback {
        let mut data = data.to_string();
        if data.contains("\x1b[200~") {
            s.is_in_paste = true;
            s.paste_buffer.clear();
            data = data.replacen("\x1b[200~", "", 1);
        }
        if s.is_in_paste {
            s.paste_buffer.push_str(&data);
            if let Some(end) = s.paste_buffer.find("\x1b[201~") {
                let content = s.paste_buffer[..end].to_string();
                Self::handle_paste(s, &content);
                s.is_in_paste = false;
                let remaining = s.paste_buffer[end + 6..].to_string();
                s.paste_buffer.clear();
                if !remaining.is_empty() {
                    return self.handle_input_locked(s, &remaining);
                }
            }
            return Callback::None;
        }

        let kb = get_keybindings();
        if kb.matches(&data, "tui.select.cancel") {
            let cb = self.on_escape.lock().unwrap_or_else(|e| e.into_inner()).clone();
            return cb.map_or(Callback::None, Callback::Escape);
        }
        if kb.matches(&data, "tui.editor.undo") {
            if let Some(snap) = s.undo.pop() {
                s.value = snap.value;
                s.cursor = snap.cursor;
                s.last_action = None;
            }
            return Callback::None;
        }
        if kb.matches(&data, "tui.input.submit") || data == "\n" {
            let cb = self.on_submit.lock().unwrap_or_else(|e| e.into_inner()).clone();
            return cb.map_or(Callback::None, |f| Callback::Submit(f, s.value.clone()));
        }
        if kb.matches(&data, "tui.editor.deleteCharBackward") {
            s.last_action = None;
            if s.cursor > 0 {
                Self::push_undo(s);
                let len = graphemes(&s.value[..s.cursor]).last().map_or(1, str::len);
                let c = s.cursor;
                s.value.replace_range(c - len..c, "");
                s.cursor -= len;
            }
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.deleteCharForward") {
            s.last_action = None;
            if s.cursor < s.value.len() {
                Self::push_undo(s);
                let len = graphemes(&s.value[s.cursor..]).next().map_or(1, str::len);
                let c = s.cursor;
                s.value.replace_range(c..c + len, "");
            }
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.deleteWordBackward") {
            Self::delete_word_backwards(s);
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.deleteWordForward") {
            Self::delete_word_forward(s);
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.deleteToLineStart") {
            if s.cursor == 0 {
                return Callback::None;
            }
            Self::push_undo(s);
            let deleted = s.value[..s.cursor].to_string();
            let acc = s.last_action == Some(LastAction::Kill);
            s.kill_ring.push(&deleted, true, acc);
            s.last_action = Some(LastAction::Kill);
            s.value = s.value[s.cursor..].to_string();
            s.cursor = 0;
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.deleteToLineEnd") {
            if s.cursor >= s.value.len() {
                return Callback::None;
            }
            Self::push_undo(s);
            let deleted = s.value[s.cursor..].to_string();
            let acc = s.last_action == Some(LastAction::Kill);
            s.kill_ring.push(&deleted, false, acc);
            s.last_action = Some(LastAction::Kill);
            let c = s.cursor;
            s.value.truncate(c);
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.yank") {
            let Some(text) = s.kill_ring.peek().map(String::from) else { return Callback::None };
            Self::push_undo(s);
            let c = s.cursor;
            s.value.insert_str(c, &text);
            s.cursor += text.len();
            s.last_action = Some(LastAction::Yank);
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.yankPop") {
            if s.last_action != Some(LastAction::Yank) || s.kill_ring.len() <= 1 {
                return Callback::None;
            }
            Self::push_undo(s);
            let prev = s.kill_ring.peek().unwrap_or("").to_string();
            let c = s.cursor;
            let start = c.saturating_sub(prev.len());
            s.value.replace_range(start..c, "");
            s.cursor = start;
            s.kill_ring.rotate();
            let text = s.kill_ring.peek().unwrap_or("").to_string();
            let c = s.cursor;
            s.value.insert_str(c, &text);
            s.cursor += text.len();
            s.last_action = Some(LastAction::Yank);
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.cursorLeft") {
            s.last_action = None;
            if s.cursor > 0 {
                let len = graphemes(&s.value[..s.cursor]).last().map_or(1, str::len);
                s.cursor -= len;
            }
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.cursorRight") {
            s.last_action = None;
            if s.cursor < s.value.len() {
                let len = graphemes(&s.value[s.cursor..]).next().map_or(1, str::len);
                s.cursor += len;
            }
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.cursorLineStart") {
            s.last_action = None;
            s.cursor = 0;
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.cursorLineEnd") {
            s.last_action = None;
            s.cursor = s.value.len();
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.cursorWordLeft") {
            Self::move_word_backwards(s);
            return Callback::None;
        }
        if kb.matches(&data, "tui.editor.cursorWordRight") {
            Self::move_word_forwards(s);
            return Callback::None;
        }
        if let Some(ch) = decode_kitty_printable(&data) {
            Self::insert_character(s, &ch);
            return Callback::None;
        }
        let has_control = data.chars().any(|c| {
            let code = c as u32;
            code < 32 || code == 0x7f || (0x80..=0x9f).contains(&code)
        });
        if !has_control {
            Self::insert_character(s, &data);
        }
        Callback::None
    }

    fn insert_character(s: &mut State, ch: &str) {
        if is_whitespace_char(ch) || s.last_action != Some(LastAction::TypeWord) {
            Self::push_undo(s);
        }
        s.last_action = Some(LastAction::TypeWord);
        let c = s.cursor;
        s.value.insert_str(c, ch);
        s.cursor += ch.len();
    }

    fn delete_word_backwards(s: &mut State) {
        if s.cursor == 0 {
            return;
        }
        let was_kill = s.last_action == Some(LastAction::Kill);
        Self::push_undo(s);
        let old = s.cursor;
        Self::move_word_backwards(s);
        let from = s.cursor;
        s.cursor = old;
        let deleted = s.value[from..old].to_string();
        s.kill_ring.push(&deleted, true, was_kill);
        s.last_action = Some(LastAction::Kill);
        s.value.replace_range(from..old, "");
        s.cursor = from;
    }

    fn delete_word_forward(s: &mut State) {
        if s.cursor >= s.value.len() {
            return;
        }
        let was_kill = s.last_action == Some(LastAction::Kill);
        Self::push_undo(s);
        let old = s.cursor;
        Self::move_word_forwards(s);
        let to = s.cursor;
        s.cursor = old;
        let deleted = s.value[old..to].to_string();
        s.kill_ring.push(&deleted, false, was_kill);
        s.last_action = Some(LastAction::Kill);
        s.value.replace_range(old..to, "");
    }

    fn push_undo(s: &mut State) {
        let snap = InputState { value: s.value.clone(), cursor: s.cursor };
        s.undo.push(&snap);
    }

    fn move_word_backwards(s: &mut State) {
        if s.cursor == 0 {
            return;
        }
        s.last_action = None;
        s.cursor = find_word_backward(&s.value, s.cursor, &WordNavigationOptions::default());
    }

    fn move_word_forwards(s: &mut State) {
        if s.cursor >= s.value.len() {
            return;
        }
        s.last_action = None;
        s.cursor = find_word_forward(&s.value, s.cursor, &WordNavigationOptions::default());
    }

    fn handle_paste(s: &mut State, pasted: &str) {
        s.last_action = None;
        Self::push_undo(s);
        let clean = pasted.replace("\r\n", "").replace('\r', "").replace('\n', "").replace('\t', "    ");
        let c = s.cursor;
        s.value.insert_str(c, &clean);
        s.cursor += clean.len();
    }
}

impl Component for Input {
    fn handles_input(&self) -> bool {
        true
    }

    fn handle_input(&self, data: &str) {
        let cb = {
            let mut s = self.lock();
            self.handle_input_locked(&mut s, data)
        };
        match cb {
            Callback::None => {}
            Callback::Submit(f, v) => f(v),
            Callback::Escape(f) => f(),
        }
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        if event.kind != MouseEventType::Press || event.button != MouseButton::Left || event.y != 0 {
            return None;
        }
        let mut s = self.lock();
        let visible_column = (event.x - 2).max(0) as usize;
        let target = s.rendered_start_column + visible_column;
        let mut current = 0;
        let mut cursor = s.value.len();
        for (index, g) in grapheme_indices(&s.value) {
            let next = current + visible_width(g);
            if target < next {
                cursor = index;
                break;
            }
            current = next;
        }
        s.cursor = cursor;
        s.last_action = None;
        Some(MouseResult { handled: true, focus: true, ..Default::default() })
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        self.lock().focused = focused;
    }

    fn focused(&self) -> bool {
        self.lock().focused
    }

    fn render(&self, width: usize) -> Vec<String> {
        let mut s = self.lock();
        let prompt_width = visible_width(&self.prompt);
        if width <= prompt_width {
            return vec![truncate_to_width(&self.prompt, width, "", false)];
        }
        let available = width - prompt_width;
        let marker = if s.focused { CURSOR_MARKER } else { "" };

        if s.value.is_empty() && !self.placeholder.is_empty() {
            let placeholder = truncate_to_width(&self.placeholder, available, "", false);
            let at_cursor = graphemes(&placeholder).next().unwrap_or(" ").to_string();
            let after = placeholder.get(at_cursor.len()..).unwrap_or("").to_string();
            let cursor_char = render_fake_cursor(&(self.placeholder_style)(&at_cursor));
            let text = format!("{marker}{cursor_char}{}", (self.placeholder_style)(&after));
            let padding = " ".repeat(available.saturating_sub(visible_width(&text)));
            return vec![format!("{}{text}{padding}", self.prompt)];
        }

        let mut visible_text = s.value.clone();
        let mut cursor_display = s.cursor;
        s.rendered_start_column = 0;
        let total_width = visible_width(&s.value);
        if total_width >= available {
            let scroll_width = if s.cursor == s.value.len() { available - 1 } else { available };
            let cursor_col = visible_width(&s.value[..s.cursor]);
            if scroll_width > 0 {
                let half = scroll_width / 2;
                let start_col = if cursor_col < half {
                    0
                } else if cursor_col > total_width.saturating_sub(half) {
                    total_width.saturating_sub(scroll_width)
                } else {
                    cursor_col.saturating_sub(half)
                };
                s.rendered_start_column = start_col;
                visible_text = slice_by_column(&s.value, start_col, scroll_width, true);
                let before = slice_by_column(&s.value, start_col, cursor_col.saturating_sub(start_col), true);
                cursor_display = before.len();
            } else {
                visible_text = String::new();
                cursor_display = 0;
            }
        }
        let cursor_display = cursor_display.min(visible_text.len());
        let at_cursor = graphemes(&visible_text[cursor_display..]).next().unwrap_or(" ").to_string();
        let before = &visible_text[..cursor_display];
        let after = visible_text.get(cursor_display + at_cursor.len()..).unwrap_or("");
        let text = format!("{before}{marker}{}{after}", render_fake_cursor(&at_cursor));
        let padding = " ".repeat(available.saturating_sub(visible_width(&text)));
        vec![format!("{}{text}{padding}", self.prompt)]
    }
}
