//! Multi-line editor with word wrap, autocomplete, kill ring, undo, paste markers and
//! history. Port of `components/editor.ts`.
//!
//! Cursor columns are byte offsets into the logical line.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Weak};
use std::time::Duration;

use regex::Regex;

use crate::StyleFn;
use crate::autocomplete::{AutocompleteItem, AutocompleteProvider, AutocompleteSuggestions, CompletionResult, SuggestOptions};
use crate::components::select_list::{SelectItem, SelectList, SelectListLayoutOptions, SelectListTheme};
use crate::event_loop::{self, AbortController, EventLoop, TimerId};
use crate::keybindings::{KeybindingsManager, get_keybindings};
use crate::keys::{decode_printable_key, matches_key};
use crate::kill_ring::KillRing;
use crate::terminal::Terminal;
use crate::tui::{Component, CURSOR_MARKER, MouseButton, MouseEvent, MouseEventType, MouseResult, Tui, render_fake_cursor};
use crate::undo_stack::UndoStack;
use crate::utils::{
    contains_cjk_break, grapheme_indices, is_autocomplete_separator_char, is_whitespace_char, js_trim, js_trim_start,
    slice_by_column, visible_width, word_segments,
};
use crate::word_navigation::{Segment, WordNavigationOptions, find_word_backward, find_word_forward};

static PASTE_MARKER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[paste #(\d+)( (\+\d+ lines|\d+ chars))?\]").unwrap());
static PASTE_MARKER_SINGLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[paste #(\d+)( (\+\d+ lines|\d+ chars))?\]$").unwrap());
static CSI_U_CTRL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[(\d+);5u").unwrap());

/// Whether a segment is a paste marker (merged by marker-aware segmentation).
fn is_paste_marker(segment: &str) -> bool {
    crate::utils::utf16_len(segment) >= 10 && PASTE_MARKER_SINGLE.is_match(segment)
}

/// A grapheme or word segment with its byte offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentData {
    pub segment: String,
    pub index: usize,
    pub is_word_like: bool,
}

fn base_segments(text: &str, word: bool) -> Vec<SegmentData> {
    if word {
        word_segments(text)
            .into_iter()
            .map(|s| SegmentData { segment: s.segment.to_string(), index: s.index, is_word_like: s.is_word_like })
            .collect()
    } else {
        grapheme_indices(text)
            .map(|(i, g)| SegmentData { segment: g.to_string(), index: i, is_word_like: false })
            .collect()
    }
}

/// Segment text, merging valid paste markers into single atomic segments.
fn segment_with_markers(text: &str, word: bool, valid_ids: &[u32]) -> Vec<SegmentData> {
    if valid_ids.is_empty() || !text.contains("[paste #") {
        return base_segments(text, word);
    }
    let mut markers: Vec<(usize, usize)> = Vec::new();
    for m in PASTE_MARKER_REGEX.captures_iter(text) {
        let id: u32 = m[1].parse().unwrap_or(u32::MAX);
        if !valid_ids.contains(&id) {
            continue;
        }
        let whole = m.get(0).unwrap();
        markers.push((whole.start(), whole.end()));
    }
    if markers.is_empty() {
        return base_segments(text, word);
    }
    let mut result = Vec::new();
    let mut marker_idx = 0;
    for seg in base_segments(text, word) {
        while marker_idx < markers.len() && markers[marker_idx].1 <= seg.index {
            marker_idx += 1;
        }
        let marker = markers.get(marker_idx);
        match marker {
            Some(&(start, end)) if seg.index >= start && seg.index < end => {
                if seg.index == start {
                    result.push(SegmentData { segment: text[start..end].to_string(), index: start, is_word_like: false });
                }
            }
            _ => result.push(seg),
        }
    }
    result
}

/// A chunk of a word-wrapped line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    pub text: String,
    pub start_index: usize,
    pub end_index: usize,
}

/// Split a line into word-wrapped chunks (byte offsets).
pub fn word_wrap_line(line: &str, max_width: usize, pre_segmented: Option<&[SegmentData]>) -> Vec<TextChunk> {
    if line.is_empty() || max_width == 0 {
        return vec![TextChunk { text: String::new(), start_index: 0, end_index: 0 }];
    }
    if visible_width(line) <= max_width {
        return vec![TextChunk { text: line.to_string(), start_index: 0, end_index: line.len() }];
    }
    let owned;
    let segments: &[SegmentData] = match pre_segmented {
        Some(s) => s,
        None => {
            owned = base_segments(line, false);
            &owned
        }
    };
    let mut chunks = Vec::new();
    let mut current_width = 0usize;
    let mut chunk_start = 0usize;
    let mut wrap_opp_index: Option<usize> = None;
    let mut wrap_opp_width = 0usize;

    let mut i = 0;
    while i < segments.len() {
        let seg = &segments[i];
        let grapheme = seg.segment.as_str();
        let g_width = visible_width(grapheme);
        let char_index = seg.index;
        let is_ws = !is_paste_marker(grapheme) && is_whitespace_char(grapheme);

        if current_width + g_width > max_width {
            match wrap_opp_index {
                Some(opp) if current_width - wrap_opp_width + g_width <= max_width => {
                    chunks.push(TextChunk { text: line[chunk_start..opp].to_string(), start_index: chunk_start, end_index: opp });
                    chunk_start = opp;
                    current_width -= wrap_opp_width;
                }
                _ => {
                    if chunk_start < char_index {
                        chunks.push(TextChunk {
                            text: line[chunk_start..char_index].to_string(),
                            start_index: chunk_start,
                            end_index: char_index,
                        });
                        chunk_start = char_index;
                        current_width = 0;
                    }
                }
            }
            wrap_opp_index = None;
        }

        if g_width > max_width {
            let sub = word_wrap_line(grapheme, max_width, None);
            for sc in &sub[..sub.len() - 1] {
                chunks.push(TextChunk {
                    text: sc.text.clone(),
                    start_index: char_index + sc.start_index,
                    end_index: char_index + sc.end_index,
                });
            }
            let last = sub.last().unwrap();
            chunk_start = char_index + last.start_index;
            current_width = visible_width(&last.text);
            wrap_opp_index = None;
            i += 1;
            continue;
        }

        current_width += g_width;
        if let Some(next) = segments.get(i + 1) {
            if is_ws && (is_paste_marker(&next.segment) || !is_whitespace_char(&next.segment)) {
                wrap_opp_index = Some(next.index);
                wrap_opp_width = current_width;
            } else if !is_ws && !is_whitespace_char(&next.segment) {
                let is_cjk = !is_paste_marker(grapheme) && contains_cjk_break(grapheme);
                let next_cjk = !is_paste_marker(&next.segment) && contains_cjk_break(&next.segment);
                if is_cjk || next_cjk {
                    wrap_opp_index = Some(next.index);
                    wrap_opp_width = current_width;
                }
            }
        }
        i += 1;
    }
    chunks.push(TextChunk { text: line[chunk_start..].to_string(), start_index: chunk_start, end_index: line.len() });
    chunks
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorState {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
}

impl Default for EditorState {
    fn default() -> Self {
        Self { lines: vec![String::new()], cursor_line: 0, cursor_col: 0 }
    }
}

#[derive(Debug, Clone)]
struct EditorSnapshot {
    state: EditorState,
    pastes: Vec<(u32, String)>,
    paste_counter: u32,
}

struct LayoutLine {
    text: String,
    has_cursor: bool,
    cursor_pos: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct VisualLine {
    logical_line: usize,
    start_col: usize,
    length: usize,
}

/// Editor theme.
#[derive(Clone)]
pub struct EditorTheme {
    pub border_color: StyleFn,
    pub select_list: SelectListTheme,
}

/// Editor options.
#[derive(Debug, Clone, Copy, Default)]
pub struct EditorOptions {
    pub padding_x: Option<usize>,
    pub autocomplete_max_visible: Option<usize>,
}

const ATTACHMENT_AUTOCOMPLETE_DEBOUNCE_MS: u64 = 20;
const DEFAULT_TRIGGER_CHARACTERS: [&str; 2] = ["@", "#"];
const WRAPPER_CHARS: &str = "([{<`";

fn is_unquoted_suffix(s: &str) -> bool {
    !s.chars().any(|c| is_autocomplete_separator_char(c) || c == '\n' || c == '\r')
}

/// `autocompleteTokenStartSource` followed by `body`, anchored at the end of `text`.
fn matches_token_pattern(text: &str, body: &dyn Fn(&str) -> bool) -> bool {
    let positions: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect();
    for &k in &positions {
        let boundary_ok = k == 0 || text[..k].chars().last().is_some_and(is_autocomplete_separator_char);
        if !boundary_ok {
            continue;
        }
        // W* then body.
        let mut j = k;
        loop {
            if body(&text[j..]) {
                return true;
            }
            match text[j..].chars().next() {
                Some(c) if WRAPPER_CHARS.contains(c) => j += c.len_utf8(),
                _ => break,
            }
        }
    }
    false
}

fn matches_trigger_pattern(text: &str, triggers: &[String]) -> bool {
    matches_token_pattern(text, &|rest: &str| {
        if let Some(q) = rest.strip_prefix("@\"")
            && !q.contains('"') && !q.contains('\n')
        {
            return true;
        }
        let Some(first) = rest.chars().next() else { return false };
        triggers.iter().any(|t| t.starts_with(first)) && is_unquoted_suffix(&rest[first.len_utf8()..])
    })
}

fn matches_debounce_pattern(text: &str, triggers: &[String]) -> bool {
    matches_token_pattern(text, &|rest: &str| {
        if let Some(after_at) = rest.strip_prefix('@') {
            if let Some(q) = after_at.strip_prefix('"')
                && !q.contains('"') && !q.contains('\n')
            {
                return true;
            }
            if is_unquoted_suffix(after_at) {
                return true;
            }
        }
        let Some(first) = rest.chars().next() else { return false };
        first != '@'
            && triggers.iter().any(|t| t != "@" && t.starts_with(first))
            && is_unquoted_suffix(&rest[first.len_utf8()..])
    })
}

fn create_scroll_border(direction: &str, hidden: usize, width: usize) -> String {
    let label = format!(" {direction} {hidden} more ");
    let label_width = visible_width(&label);
    if label_width + 2 <= width {
        let left = (width - label_width) / 2;
        return format!("{}{label}{}", "─".repeat(left), "─".repeat(width - left - label_width));
    }
    let indicator = format!("─── {direction} {hidden} more ");
    let iw = visible_width(&indicator);
    if width >= iw {
        return format!("{indicator}{}", "─".repeat(width - iw));
    }
    let ellipsis = &"..."[..width.min(3)];
    let indicator_width = width - ellipsis.len();
    format!("{}{ellipsis}", slice_by_column(&indicator, 0, indicator_width, true))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastAction {
    Kill,
    Yank,
    TypeWord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcState {
    Regular,
    Force,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JumpDir {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy)]
struct AcOptions {
    force: bool,
    explicit_tab: bool,
}

/// Autocomplete work that calls into the provider. Queued while the state lock is held and run by
/// [`Editor::with`] after the lock is released: providers may call into extension or application
/// code (session state, a JS provider blocking on the extension thread that reads the editor back),
/// which must never run under the editor lock.
#[derive(Clone, Copy)]
enum PendingAutocomplete {
    /// `requestAutocomplete` for a forced request: ask `shouldTriggerFileCompletion` first.
    CheckThenRequest(AcOptions),
    /// `startAutocompleteRequest`: call `getSuggestions`.
    Start { token: u64, options: AcOptions },
}

enum Event {
    Change(String),
    Submit(String),
    RequestRender,
}

/// An `apply_completion` result computed without the editor lock. Providers may call into
/// extension code (a JS provider blocks on the extension thread, which may read the editor back),
/// so they must never run while the state lock is held. The result is used only when the editor
/// still matches the snapshot it was computed from; otherwise the provider is asked again.
struct PrecomputedCompletion {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
    item: AutocompleteItem,
    prefix: String,
    result: CompletionResult,
}

impl PrecomputedCompletion {
    fn compute(provider: &Arc<dyn AutocompleteProvider>, s: &Inner, item: AutocompleteItem, prefix: String) -> impl FnOnce() -> Self + use<> {
        let (provider, lines, cursor_line, cursor_col) =
            (provider.clone(), s.state.lines.clone(), s.state.cursor_line, s.state.cursor_col);
        move || {
            let result = provider.apply_completion(&lines, cursor_line, cursor_col, &item, &prefix);
            Self { lines, cursor_line, cursor_col, item, prefix, result }
        }
    }
}

/// The completion for `item`: the precomputed one when it still applies, else ask the provider.
fn completion_for(s: &mut Inner, provider: &Arc<dyn AutocompleteProvider>, item: &AutocompleteItem, prefix: &str) -> CompletionResult {
    if let Some(p) = s.precomputed_completion.take()
        && p.lines == s.state.lines
        && p.cursor_line == s.state.cursor_line
        && p.cursor_col == s.state.cursor_col
        && p.item == *item
        && p.prefix == prefix
    {
        return p.result;
    }
    provider.apply_completion(&s.state.lines, s.state.cursor_line, s.state.cursor_col, item, prefix)
}

struct Inner {
    state: EditorState,
    focused: bool,
    padding_x: usize,
    last_width: usize,
    rendered_visible_line_count: usize,
    rendered_autocomplete_height: usize,
    scroll_offset: usize,
    border_color: StyleFn,
    provider: Option<Arc<dyn AutocompleteProvider>>,
    trigger_chars: Vec<String>,
    ac_list: Option<Arc<SelectList>>,
    ac_state: Option<AcState>,
    ac_prefix: String,
    /// Provider result computed before the state lock was taken (see [`PrecomputedCompletion`]).
    precomputed_completion: Option<PrecomputedCompletion>,
    ac_max_visible: usize,
    ac_abort: Option<AbortController>,
    ac_debounce_timer: Option<TimerId>,
    /// Provider call to make once the state lock is released (see [`PendingAutocomplete`]).
    pending_autocomplete: Option<PendingAutocomplete>,
    ac_start_token: u64,
    ac_request_id: u64,
    pastes: Vec<(u32, String)>,
    paste_counter: u32,
    paste_buffer: String,
    is_in_paste: bool,
    history: Vec<String>,
    history_index: i64,
    history_draft: Option<EditorState>,
    kill_ring: KillRing,
    last_action: Option<LastAction>,
    jump_mode: Option<JumpDir>,
    preferred_visual_col: Option<usize>,
    snapped_from_cursor_col: Option<usize>,
    undo: UndoStack<EditorSnapshot>,
    disable_submit: bool,
    events: Vec<Event>,
}

/// Submit / change callback.
pub type TextCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Override of the top border (TS subclasses overriding `renderTopBorder(width, hiddenLineCount)`).
/// Returns the styled border line, or `None` for the default border. Called without the editor's
/// lock held, so it may call back into the editor (e.g. [`Editor::border_color`]).
pub type TopBorderRenderer = Arc<dyn Fn(usize, usize) -> Option<String> + Send + Sync>;

#[derive(Default)]
struct Callbacks {
    on_submit: Option<TextCallback>,
    on_change: Option<TextCallback>,
    top_border: Option<TopBorderRenderer>,
}

/// Multi-line text editor component.
pub struct Editor {
    weak_self: Weak<Editor>,
    tui: Weak<Tui>,
    terminal: Arc<dyn Terminal>,
    event_loop: EventLoop,
    theme: EditorTheme,
    st: Mutex<Inner>,
    callbacks: Mutex<Callbacks>,
}

impl Editor {
    pub fn new(tui: &Arc<Tui>, theme: EditorTheme, options: EditorOptions) -> Arc<Self> {
        let max_visible = options.autocomplete_max_visible.unwrap_or(5).clamp(3, 20);
        Arc::new_cyclic(|weak| Editor {
            weak_self: weak.clone(),
            tui: Arc::downgrade(tui),
            terminal: tui.terminal().clone(),
            event_loop: tui.event_loop().clone(),
            st: Mutex::new(Inner {
                state: EditorState::default(),
                focused: false,
                padding_x: options.padding_x.unwrap_or(0),
                last_width: 80,
                rendered_visible_line_count: 1,
                rendered_autocomplete_height: 0,
                scroll_offset: 0,
                border_color: theme.border_color.clone(),
                provider: None,
                trigger_chars: DEFAULT_TRIGGER_CHARACTERS.iter().map(|s| s.to_string()).collect(),
                ac_list: None,
                ac_state: None,
                ac_prefix: String::new(),
                precomputed_completion: None,
                ac_max_visible: max_visible,
                ac_abort: None,
                ac_debounce_timer: None,
                pending_autocomplete: None,
                ac_start_token: 0,
                ac_request_id: 0,
                pastes: Vec::new(),
                paste_counter: 0,
                paste_buffer: String::new(),
                is_in_paste: false,
                history: Vec::new(),
                history_index: -1,
                history_draft: None,
                kill_ring: KillRing::new(),
                last_action: None,
                jump_mode: None,
                preferred_visual_col: None,
                snapped_from_cursor_col: None,
                undo: UndoStack::new(),
                disable_submit: false,
                events: Vec::new(),
            }),
            theme,
            callbacks: Mutex::new(Callbacks::default()),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cbs(&self) -> MutexGuard<'_, Callbacks> {
        self.callbacks.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Run `f` with the state locked, then deliver queued callbacks and make queued provider calls
    /// without the lock.
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let (r, events) = {
            let mut s = self.lock();
            let r = f(&mut s);
            (r, std::mem::take(&mut s.events))
        };
        self.dispatch(events);
        self.run_pending_autocomplete();
        r
    }

    fn dispatch(&self, events: Vec<Event>) {
        if events.is_empty() {
            return;
        }
        let (on_change, on_submit) = {
            let c = self.cbs();
            (c.on_change.clone(), c.on_submit.clone())
        };
        for e in events {
            match e {
                Event::Change(t) => {
                    if let Some(f) = &on_change {
                        f(t);
                    }
                }
                Event::Submit(t) => {
                    if let Some(f) = &on_submit {
                        f(t);
                    }
                }
                Event::RequestRender => {
                    if let Some(tui) = self.tui.upgrade() {
                        tui.request_render(false);
                    }
                }
            }
        }
    }

    // ---- public API ----

    pub fn set_on_submit(&self, f: Option<TextCallback>) {
        self.cbs().on_submit = f;
    }

    pub fn set_on_change(&self, f: Option<TextCallback>) {
        self.cbs().on_change = f;
    }

    /// Install a top border override (see [`TopBorderRenderer`]).
    pub fn set_top_border_renderer(&self, f: Option<TopBorderRenderer>) {
        self.cbs().top_border = f;
    }

    pub fn disable_submit(&self) -> bool {
        self.lock().disable_submit
    }

    pub fn set_disable_submit(&self, disable: bool) {
        self.lock().disable_submit = disable;
    }

    pub fn set_border_color(&self, f: StyleFn) {
        self.lock().border_color = f;
    }

    pub fn border_color(&self) -> StyleFn {
        self.lock().border_color.clone()
    }

    pub fn get_padding_x(&self) -> usize {
        self.lock().padding_x
    }

    pub fn set_padding_x(&self, padding: usize) {
        self.with(|s| {
            if s.padding_x != padding {
                s.padding_x = padding;
                s.events.push(Event::RequestRender);
            }
        });
    }

    pub fn get_autocomplete_max_visible(&self) -> usize {
        self.lock().ac_max_visible
    }

    pub fn set_autocomplete_max_visible(&self, max_visible: usize) {
        let v = max_visible.clamp(3, 20);
        self.with(|s| {
            if s.ac_max_visible != v {
                s.ac_max_visible = v;
                s.events.push(Event::RequestRender);
            }
        });
    }

    pub fn set_autocomplete_provider(&self, provider: Arc<dyn AutocompleteProvider>) {
        // Provider methods may call into extension code: never under the state lock.
        let triggers = provider.trigger_characters();
        self.with(|s| {
            self.cancel_autocomplete(s);
            s.provider = Some(provider);
            let mut next: Vec<String> = DEFAULT_TRIGGER_CHARACTERS.iter().map(|s| s.to_string()).collect();
            for c in triggers {
                if c.chars().count() != 1 || crate::utils::utf16_len(&c) != 1 || c == "/" || is_whitespace_char(&c) || next.contains(&c) {
                    continue;
                }
                next.push(c);
            }
            s.trigger_chars = next;
        });
    }

    /// Add a prompt to history for up/down navigation.
    pub fn add_to_history(&self, text: &str) {
        let trimmed = js_trim(text);
        if trimmed.is_empty() {
            return;
        }
        let mut s = self.lock();
        if s.history.first().is_some_and(|h| h == trimmed) {
            return;
        }
        s.history.insert(0, trimmed.to_string());
        if s.history.len() > 100 {
            s.history.pop();
        }
    }

    pub fn get_text(&self) -> String {
        self.lock().state.lines.join("\n")
    }

    /// Text with paste markers expanded to their content.
    pub fn get_expanded_text(&self) -> String {
        let s = self.lock();
        expand_paste_markers(&s.pastes, &s.state.lines.join("\n"))
    }

    pub fn get_lines(&self) -> Vec<String> {
        self.lock().state.lines.clone()
    }

    /// Cursor position: `(line, byte column)`.
    pub fn get_cursor(&self) -> (usize, usize) {
        let s = self.lock();
        (s.state.cursor_line, s.state.cursor_col)
    }

    pub fn set_text(&self, text: &str) {
        self.with(|s| {
            self.cancel_autocomplete(s);
            s.last_action = None;
            exit_history_browsing(s);
            let normalized = normalize_text(text);
            if s.state.lines.join("\n") != normalized {
                push_undo_snapshot(s);
            }
            s.pastes.clear();
            s.paste_counter = 0;
            set_text_internal(s, &normalized, false);
        });
    }

    /// Insert text at the cursor (atomic for undo).
    pub fn insert_text_at_cursor(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.with(|s| {
            self.cancel_autocomplete(s);
            push_undo_snapshot(s);
            s.last_action = None;
            exit_history_browsing(s);
            insert_text_at_cursor_internal(s, text);
        });
    }

    pub fn is_showing_autocomplete(&self) -> bool {
        self.lock().ac_state.is_some()
    }

    // ---- rendering helpers ----

    fn segment(s: &Inner, text: &str, word: bool) -> Vec<SegmentData> {
        let ids: Vec<u32> = s.pastes.iter().map(|(id, _)| *id).collect();
        segment_with_markers(text, word, &ids)
    }

    fn layout_text(s: &Inner, content_width: usize) -> Vec<LayoutLine> {
        let mut out = Vec::new();
        if s.state.lines.is_empty() || (s.state.lines.len() == 1 && s.state.lines[0].is_empty()) {
            out.push(LayoutLine { text: String::new(), has_cursor: true, cursor_pos: Some(0) });
            return out;
        }
        for (i, line) in s.state.lines.iter().enumerate() {
            let is_current = i == s.state.cursor_line;
            if visible_width(line) <= content_width {
                out.push(LayoutLine {
                    text: line.clone(),
                    has_cursor: is_current,
                    cursor_pos: if is_current { Some(s.state.cursor_col) } else { None },
                });
                continue;
            }
            let segs = Self::segment(s, line, false);
            let chunks = word_wrap_line(line, content_width, Some(&segs));
            let n = chunks.len();
            for (ci, chunk) in chunks.into_iter().enumerate() {
                let cursor = s.state.cursor_col;
                let is_last = ci == n - 1;
                let mut has = false;
                let mut adjusted = 0;
                if is_current {
                    if is_last {
                        has = cursor >= chunk.start_index;
                        adjusted = cursor.saturating_sub(chunk.start_index);
                    } else {
                        has = cursor >= chunk.start_index && cursor < chunk.end_index;
                        if has {
                            adjusted = (cursor - chunk.start_index).min(chunk.text.len());
                        }
                    }
                }
                out.push(LayoutLine { text: chunk.text, has_cursor: has, cursor_pos: if has { Some(adjusted) } else { None } });
            }
        }
        out
    }

    fn build_visual_line_map(s: &Inner, width: usize) -> Vec<VisualLine> {
        let mut out = Vec::new();
        for (i, line) in s.state.lines.iter().enumerate() {
            if line.is_empty() {
                out.push(VisualLine { logical_line: i, start_col: 0, length: 0 });
            } else if visible_width(line) <= width {
                out.push(VisualLine { logical_line: i, start_col: 0, length: line.len() });
            } else {
                let segs = Self::segment(s, line, false);
                for chunk in word_wrap_line(line, width, Some(&segs)) {
                    out.push(VisualLine {
                        logical_line: i,
                        start_col: chunk.start_index,
                        length: chunk.end_index - chunk.start_index,
                    });
                }
            }
        }
        out
    }

    fn find_visual_line_at(vls: &[VisualLine], line: usize, col: usize) -> usize {
        for (i, vl) in vls.iter().enumerate() {
            if vl.logical_line != line {
                continue;
            }
            let is_last = i == vls.len() - 1 || vls[i + 1].logical_line != vl.logical_line;
            if col >= vl.start_col {
                let offset = col - vl.start_col;
                if offset < vl.length || (is_last && offset == vl.length) {
                    return i;
                }
            }
        }
        vls.len().saturating_sub(1)
    }

    fn find_current_visual_line(s: &Inner, vls: &[VisualLine]) -> usize {
        Self::find_visual_line_at(vls, s.state.cursor_line, s.state.cursor_col)
    }

    // ---- autocomplete ----

    fn cancel_autocomplete_request(&self, s: &mut Inner) {
        s.ac_start_token += 1;
        if let Some(t) = s.ac_debounce_timer.take() {
            self.event_loop.clear_timer(t);
        }
        if let Some(a) = s.ac_abort.take() {
            a.abort();
        }
    }

    fn cancel_autocomplete(&self, s: &mut Inner) {
        self.cancel_autocomplete_request(s);
        s.ac_state = None;
        s.ac_list = None;
        s.ac_prefix.clear();
    }

    fn update_autocomplete(&self, s: &mut Inner) {
        let Some(state) = s.ac_state else { return };
        if s.provider.is_none() {
            return;
        }
        self.request_autocomplete(s, AcOptions { force: state == AcState::Force, explicit_tab: false });
    }

    fn try_trigger_autocomplete(&self, s: &mut Inner, explicit_tab: bool) {
        self.request_autocomplete(s, AcOptions { force: false, explicit_tab });
    }

    fn request_autocomplete(&self, s: &mut Inner, options: AcOptions) {
        let Some(provider) = s.provider.clone() else { return };
        if options.force && provider.has_should_trigger_file_completion() {
            // Asked without the lock; the request continues in `run_pending_autocomplete`.
            s.pending_autocomplete = Some(PendingAutocomplete::CheckThenRequest(options));
            return;
        }
        self.request_autocomplete_checked(s, options);
    }

    /// `requestAutocomplete` after the `shouldTriggerFileCompletion` gate.
    fn request_autocomplete_checked(&self, s: &mut Inner, options: AcOptions) {
        self.cancel_autocomplete_request(s);
        let token = s.ac_start_token + 1;
        s.ac_start_token = token;
        let debounce = if options.explicit_tab || options.force {
            0
        } else if matches_debounce_pattern(&text_before_cursor(s), &s.trigger_chars) {
            ATTACHMENT_AUTOCOMPLETE_DEBOUNCE_MS
        } else {
            0
        };
        if debounce > 0 {
            let weak = self.weak_self.clone();
            s.ac_debounce_timer = Some(self.event_loop.set_timeout(Duration::from_millis(debounce), move || {
                if let Some(me) = weak.upgrade() {
                    me.with(|s| {
                        s.ac_debounce_timer = None;
                        me.start_autocomplete_request(s, token, options);
                    });
                }
            }));
            return;
        }
        self.start_autocomplete_request(s, token, options);
    }

    fn start_autocomplete_request(&self, s: &mut Inner, token: u64, options: AcOptions) {
        if token != s.ac_start_token {
            return;
        }
        s.pending_autocomplete = Some(PendingAutocomplete::Start { token, options });
    }

    /// Make the provider call queued by [`request_autocomplete`](Self::request_autocomplete) or
    /// [`start_autocomplete_request`](Self::start_autocomplete_request), without the state lock.
    fn run_pending_autocomplete(&self) {
        let Some((pending, provider, lines, cursor_line, cursor_col)) = ({
            let mut s = self.lock();
            match (s.pending_autocomplete.take(), s.provider.clone()) {
                (Some(p), Some(provider)) => Some((p, provider, s.state.lines.clone(), s.state.cursor_line, s.state.cursor_col)),
                _ => None,
            }
        }) else {
            return;
        };
        match pending {
            PendingAutocomplete::CheckThenRequest(options) => {
                if provider.should_trigger_file_completion(&lines, cursor_line, cursor_col) {
                    self.with(|s| {
                        // The editor may have changed meanwhile; TS checks and requests in one go.
                        if s.state.lines == lines && s.state.cursor_line == cursor_line && s.state.cursor_col == cursor_col {
                            self.request_autocomplete_checked(s, options);
                        }
                    });
                }
            }
            PendingAutocomplete::Start { token, options } => self.run_autocomplete_request(token, options),
        }
    }

    fn run_autocomplete_request(&self, token: u64, options: AcOptions) {
        let Some((provider, controller, request_id, lines, cursor_line, cursor_col)) = ({
            let mut s = self.lock();
            if token != s.ac_start_token {
                None
            } else if let Some(provider) = s.provider.clone() {
                let controller = AbortController::new();
                s.ac_abort = Some(controller.clone());
                s.ac_request_id += 1;
                Some((provider, controller, s.ac_request_id, s.state.lines.clone(), s.state.cursor_line, s.state.cursor_col))
            } else {
                None
            }
        }) else {
            return;
        };
        let snapshot = (lines.join("\n"), cursor_line, cursor_col);
        let mut fut = provider.get_suggestions(
            &lines,
            cursor_line,
            cursor_col,
            SuggestOptions { signal: controller.signal(), force: options.force },
        );
        // TS resolves synchronous suggestions (slash commands) in microtasks, before the next key
        // is handled. Apply a ready result right away so a following Escape/Enter/Tab sees the
        // current list, instead of racing a spawned task: with typing faster than the task ran,
        // every newer result was discarded as stale and an old list (items and prefix) stayed up,
        // so accepting it spliced the wrong completion ("/thinking" became "/thinkitree"), and
        // Escape right after "/" interrupted the agent instead of closing the list.
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        if let std::task::Poll::Ready(suggestions) = fut.as_mut().poll(&mut cx) {
            // A single forced suggestion is applied directly: compute it here, without the lock.
            let precomputed = match &suggestions {
                Some(sg) if options.force && options.explicit_tab && sg.items.len() == 1 => {
                    let item = sg.items[0].clone();
                    let result = provider.apply_completion(&lines, cursor_line, cursor_col, &item, &sg.prefix);
                    Some(PrecomputedCompletion { lines, cursor_line, cursor_col, item, prefix: sg.prefix.clone(), result })
                }
                _ => None,
            };
            self.with(|s| {
                s.precomputed_completion = precomputed;
                self.finish_autocomplete(s, request_id, &controller, &snapshot, suggestions, options);
                s.precomputed_completion = None;
            });
            return;
        }
        let weak = self.weak_self.clone();
        event_loop::spawn(&self.event_loop, async move {
            let suggestions = fut.await;
            if let Some(me) = weak.upgrade() {
                // A single forced suggestion is applied directly: compute it without the lock.
                let precompute = match &suggestions {
                    Some(sg) if options.force && options.explicit_tab && sg.items.len() == 1 => {
                        let s = me.lock();
                        s.provider.as_ref().map(|p| PrecomputedCompletion::compute(p, &s, sg.items[0].clone(), sg.prefix.clone()))
                    }
                    _ => None,
                };
                let precomputed = precompute.map(|f| f());
                me.with(|s| {
                    s.precomputed_completion = precomputed;
                    me.finish_autocomplete(s, request_id, &controller, &snapshot, suggestions, options);
                    s.precomputed_completion = None;
                });
            }
        });
    }

    fn finish_autocomplete(
        &self,
        s: &mut Inner,
        request_id: u64,
        controller: &AbortController,
        snapshot: &(String, usize, usize),
        suggestions: Option<AutocompleteSuggestions>,
        options: AcOptions,
    ) {
        let Some(provider) = s.provider.clone() else { return };
        let current = !controller.signal().is_aborted()
            && request_id == s.ac_request_id
            && s.state.lines.join("\n") == snapshot.0
            && s.state.cursor_line == snapshot.1
            && s.state.cursor_col == snapshot.2;
        if !current {
            return;
        }
        s.ac_abort = None;
        let Some(suggestions) = suggestions.filter(|sg| !sg.items.is_empty()) else {
            self.cancel_autocomplete(s);
            s.events.push(Event::RequestRender);
            return;
        };
        if options.force && options.explicit_tab && suggestions.items.len() == 1 {
            let item = suggestions.items[0].clone();
            push_undo_snapshot(s);
            s.last_action = None;
            let r = completion_for(s, &provider, &item, &suggestions.prefix);
            s.state.lines = r.lines;
            s.state.cursor_line = r.cursor_line;
            set_cursor_col(s, r.cursor_col);
            emit_change(s);
            s.events.push(Event::RequestRender);
            return;
        }
        self.apply_autocomplete_suggestions(s, suggestions, if options.force { AcState::Force } else { AcState::Regular });
        s.events.push(Event::RequestRender);
    }

    fn apply_autocomplete_suggestions(&self, s: &mut Inner, suggestions: AutocompleteSuggestions, state: AcState) {
        s.ac_prefix = suggestions.prefix.clone();
        let list = self.create_autocomplete_list(s, &suggestions.prefix, &suggestions.items);
        if let Some(best) = best_autocomplete_match_index(&suggestions.items, &suggestions.prefix) {
            list.set_selected_index(best);
        }
        s.ac_list = Some(list);
        s.ac_state = Some(state);
    }

    fn create_autocomplete_list(&self, s: &Inner, prefix: &str, items: &[AutocompleteItem]) -> Arc<SelectList> {
        let layout = if prefix.starts_with('/') {
            SelectListLayoutOptions { min_primary_column_width: Some(12), max_primary_column_width: Some(32), truncate_primary: None }
        } else {
            SelectListLayoutOptions::default()
        };
        let select_items: Vec<SelectItem> = items
            .iter()
            .map(|i| SelectItem { value: i.value.clone(), label: i.label.clone(), description: i.description.clone() })
            .collect();
        let list = Arc::new(SelectList::new(select_items, s.ac_max_visible, self.theme.select_list.clone(), layout));
        let weak = self.weak_self.clone();
        let item_lookup: Vec<AutocompleteItem> = items.to_vec();
        list.set_on_select(Some(Arc::new(move |selected: SelectItem| {
            let Some(me) = weak.upgrade() else { return };
            let item = item_lookup
                .iter()
                .find(|i| i.value == selected.value && i.label == selected.label)
                .cloned()
                .unwrap_or(AutocompleteItem { value: selected.value, label: selected.label, description: selected.description });
            let precompute = {
                let s = me.lock();
                s.provider.as_ref().map(|p| PrecomputedCompletion::compute(p, &s, item.clone(), s.ac_prefix.clone()))
            };
            let precomputed = precompute.map(|f| f());
            me.with(|s| {
                s.precomputed_completion = precomputed;
                let Some(provider) = s.provider.clone() else { return };
                push_undo_snapshot(s);
                s.last_action = None;
                let prefix = s.ac_prefix.clone();
                let r = completion_for(s, &provider, &item, &prefix);
                s.state.lines = r.lines;
                s.state.cursor_line = r.cursor_line;
                set_cursor_col(s, r.cursor_col);
                me.cancel_autocomplete(s);
                emit_change(s);
            });
        })));
        list
    }

    fn selected_autocomplete_item(s: &Inner) -> Option<AutocompleteItem> {
        let list = s.ac_list.as_ref()?;
        let sel = list.get_selected_item()?;
        Some(AutocompleteItem { value: sel.value, label: sel.label, description: sel.description })
    }

    fn handle_tab_completion(&self, s: &mut Inner) {
        if s.provider.is_none() {
            return;
        }
        let before = text_before_cursor(s);
        if is_in_slash_command_context(s, &before) && !js_trim_start(&before).contains(' ') {
            self.request_autocomplete(s, AcOptions { force: false, explicit_tab: true });
        } else {
            self.request_autocomplete(s, AcOptions { force: true, explicit_tab: true });
        }
    }

    // ---- editing operations ----

    fn insert_character(&self, s: &mut Inner, ch: &str, skip_undo_coalescing: bool) {
        exit_history_browsing(s);
        if !skip_undo_coalescing {
            if is_whitespace_char(ch) || s.last_action != Some(LastAction::TypeWord) {
                push_undo_snapshot(s);
            }
            s.last_action = Some(LastAction::TypeWord);
        }
        let line_idx = s.state.cursor_line;
        let col = s.state.cursor_col;
        s.state.lines[line_idx].insert_str(col, ch);
        set_cursor_col(s, col + ch.len());
        emit_change(s);

        if s.ac_state.is_none() {
            if ch == "/" && is_at_start_of_message(s) {
                self.try_trigger_autocomplete(s, false);
            } else if s.trigger_chars.iter().any(|t| t == ch) {
                if matches_trigger_pattern(&text_before_cursor(s), &s.trigger_chars) {
                    self.try_trigger_autocomplete(s, false);
                }
            } else if ch.chars().any(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                || ch.chars().any(crate::utils::is_cjk_break_char)
            {
                let before = text_before_cursor(s);
                if is_in_slash_command_context(s, &before) || matches_trigger_pattern(&before, &s.trigger_chars) {
                    self.try_trigger_autocomplete(s, false);
                }
            }
        } else {
            self.update_autocomplete(s);
        }
    }

    fn handle_paste(&self, s: &mut Inner, pasted: &str) {
        self.cancel_autocomplete(s);
        exit_history_browsing(s);
        s.last_action = None;
        push_undo_snapshot(s);
        let decoded = CSI_U_CTRL.replace_all(pasted, |caps: &regex::Captures| {
            let cp: u64 = caps[1].parse().unwrap_or(0);
            if (97..=122).contains(&cp) {
                char::from_u32((cp - 96) as u32).unwrap().to_string()
            } else if (65..=90).contains(&cp) {
                char::from_u32((cp - 64) as u32).unwrap().to_string()
            } else {
                caps[0].to_string()
            }
        });
        let clean = normalize_text(&decoded);
        let mut filtered: String = clean.chars().filter(|c| *c == '\n' || (*c as u32) >= 32).collect();
        if filtered.starts_with(['/', '~', '.']) {
            let line = &s.state.lines[s.state.cursor_line];
            let col = s.state.cursor_col;
            if col > 0 {
                let prev = line.as_bytes()[col - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' {
                    filtered = format!(" {filtered}");
                }
            }
        }
        let pasted_lines: Vec<&str> = filtered.split('\n').collect();
        let total_chars = crate::utils::utf16_len(&filtered);
        if pasted_lines.len() > 10 || total_chars > 1000 {
            s.paste_counter += 1;
            let id = s.paste_counter;
            let line_count = pasted_lines.len();
            s.pastes.retain(|(i, _)| *i != id);
            s.pastes.push((id, filtered.clone()));
            let marker = if line_count > 10 {
                format!("[paste #{id} +{line_count} lines]")
            } else {
                format!("[paste #{id} {total_chars} chars]")
            };
            insert_text_at_cursor_internal(s, &marker);
            return;
        }
        insert_text_at_cursor_internal(s, &filtered);
    }

    fn add_new_line(&self, s: &mut Inner) {
        self.cancel_autocomplete(s);
        exit_history_browsing(s);
        s.last_action = None;
        push_undo_snapshot(s);
        let line_idx = s.state.cursor_line;
        let col = s.state.cursor_col;
        let after = s.state.lines[line_idx].split_off(col);
        s.state.lines.insert(line_idx + 1, after);
        s.state.cursor_line += 1;
        set_cursor_col(s, 0);
        emit_change(s);
    }

    fn should_submit_on_backslash_enter(s: &Inner, data: &str, kb: &KeybindingsManager) -> bool {
        if s.disable_submit || !matches_key(data, "enter") {
            return false;
        }
        let submit_keys = kb.get_keys("tui.input.submit");
        if !submit_keys.iter().any(|k| k == "shift+enter" || k == "shift+return") {
            return false;
        }
        char_before_cursor_is_backslash(s)
    }

    fn submit_value(&self, s: &mut Inner) {
        self.cancel_autocomplete(s);
        let result = js_trim(&expand_paste_markers(&s.pastes, &s.state.lines.join("\n"))).to_string();
        s.state = EditorState::default();
        s.pastes.clear();
        s.paste_counter = 0;
        exit_history_browsing(s);
        s.scroll_offset = 0;
        s.undo.clear();
        s.last_action = None;
        s.events.push(Event::Change(String::new()));
        s.events.push(Event::Submit(result));
    }

    fn handle_backspace(&self, s: &mut Inner) {
        exit_history_browsing(s);
        s.last_action = None;
        if s.state.cursor_col > 0 {
            push_undo_snapshot(s);
            let line = s.state.lines[s.state.cursor_line].clone();
            let before = &line[..s.state.cursor_col];
            let graphemes = Self::segment(s, before, false);
            let last = graphemes.last().map(|g| g.segment.clone()).unwrap_or_default();
            let glen = if last.is_empty() { 1 } else { last.len() };
            if let Some(caps) = PASTE_MARKER_SINGLE.captures(&last) {
                let target: u32 = caps[1].parse().unwrap_or(0);
                s.pastes.retain(|(id, _)| *id != target);
                s.paste_counter = s.paste_counter.saturating_sub(1);
                let mut higher: Vec<u32> = s.pastes.iter().map(|(id, _)| *id).filter(|id| *id > target).collect();
                higher.sort();
                for id in higher {
                    if let Some(pos) = s.pastes.iter().position(|(i, _)| *i == id) {
                        let (_, content) = s.pastes.remove(pos);
                        // `Map.set(id - 1, ...)` then `delete(id)`: the new key is appended.
                        if let Some(existing) = s.pastes.iter_mut().find(|(i, _)| *i == id - 1) {
                            existing.1 = content;
                        } else {
                            s.pastes.push((id - 1, content));
                        }
                    }
                }
                s.state.lines = s
                    .state
                    .lines
                    .iter()
                    .map(|l| {
                        PASTE_MARKER_REGEX
                            .replace_all(l, |c: &regex::Captures| {
                                let x: u32 = c[1].parse().unwrap_or(0);
                                if x <= target {
                                    c[0].to_string()
                                } else {
                                    format!("[paste #{}{}]", x - 1, c.get(2).map_or("", |m| m.as_str()))
                                }
                            })
                            .to_string()
                    })
                    .collect();
            }
            let line = s.state.lines[s.state.cursor_line].clone();
            let col = s.state.cursor_col.min(line.len());
            let start = col.saturating_sub(glen);
            s.state.lines[s.state.cursor_line] = format!("{}{}", &line[..start], &line[col..]);
            set_cursor_col(s, start);
        } else if s.state.cursor_line > 0 {
            push_undo_snapshot(s);
            let current = s.state.lines.remove(s.state.cursor_line);
            s.state.cursor_line -= 1;
            let prev_len = s.state.lines[s.state.cursor_line].len();
            s.state.lines[s.state.cursor_line].push_str(&current);
            set_cursor_col(s, prev_len);
        }
        emit_change(s);
        self.retrigger_after_delete(s);
    }

    fn retrigger_after_delete(&self, s: &mut Inner) {
        if s.ac_state.is_some() {
            self.update_autocomplete(s);
        } else {
            let before = text_before_cursor(s);
            if is_in_slash_command_context(s, &before) || matches_trigger_pattern(&before, &s.trigger_chars) {
                self.try_trigger_autocomplete(s, false);
            }
        }
    }

    fn handle_forward_delete(&self, s: &mut Inner) {
        exit_history_browsing(s);
        s.last_action = None;
        let line = s.state.lines[s.state.cursor_line].clone();
        if s.state.cursor_col < line.len() {
            push_undo_snapshot(s);
            let after = &line[s.state.cursor_col..];
            let segs = Self::segment(s, after, false);
            let glen = segs.first().map_or(1, |g| g.segment.len());
            let col = s.state.cursor_col;
            s.state.lines[s.state.cursor_line] = format!("{}{}", &line[..col], &line[(col + glen).min(line.len())..]);
        } else if s.state.cursor_line + 1 < s.state.lines.len() {
            push_undo_snapshot(s);
            let next = s.state.lines.remove(s.state.cursor_line + 1);
            s.state.lines[s.state.cursor_line].push_str(&next);
        }
        emit_change(s);
        self.retrigger_after_delete(s);
    }

    fn move_to_visual_line(&self, s: &mut Inner, vls: &[VisualLine], current: usize, target: usize) {
        let (Some(cur), Some(tgt)) = (vls.get(current).copied(), vls.get(target).copied()) else { return };
        // Visual columns are measured in UTF-16 code units, like the reference
        // implementation, so sticky-column behaviour matches exactly.
        let cur_line = s.state.lines[cur.logical_line].clone();
        let current_visual_col = match s.snapped_from_cursor_col {
            Some(snapped) => {
                let idx = Self::find_visual_line_at(vls, cur.logical_line, snapped);
                u16_span(&cur_line, vls[idx].start_col, snapped)
            }
            None => u16_span(&cur_line, cur.start_col, s.state.cursor_col),
        };
        let cur_len = u16_span(&cur_line, cur.start_col, cur.start_col + cur.length);
        let is_last_source = current == vls.len() - 1 || vls[current + 1].logical_line != cur.logical_line;
        let source_max = if is_last_source { cur_len } else { cur_len.saturating_sub(1) };
        let logical = s.state.lines[tgt.logical_line].clone();
        let tgt_len = u16_span(&logical, tgt.start_col, tgt.start_col + tgt.length);
        let is_last_target = target == vls.len() - 1 || vls[target + 1].logical_line != tgt.logical_line;
        let target_max = if is_last_target { tgt_len } else { tgt_len.saturating_sub(1) };
        let move_to = compute_vertical_move_column(s, current_visual_col, source_max, target_max);

        s.state.cursor_line = tgt.logical_line;
        s.state.cursor_col = byte_at_u16(&logical, tgt.start_col, move_to);

        for seg in Self::segment(s, &logical, false) {
            if seg.index > s.state.cursor_col {
                break;
            }
            if crate::utils::utf16_len(&seg.segment) <= 1 {
                continue;
            }
            if s.state.cursor_col < seg.index + seg.segment.len() {
                let is_continuation = seg.index < tgt.start_col;
                let moving_down = target > current;
                if is_continuation && moving_down {
                    let seg_end = seg.index + seg.segment.len();
                    let mut next = target + 1;
                    while next < vls.len() && vls[next].logical_line == tgt.logical_line && vls[next].start_col < seg_end {
                        next += 1;
                    }
                    if next < vls.len() {
                        self.move_to_visual_line(s, vls, current, next);
                        return;
                    }
                }
                // Snap to the segment start; remember the pre-snap position.
                s.snapped_from_cursor_col = Some(s.state.cursor_col);
                s.state.cursor_col = seg.index;
                return;
            }
        }
        s.snapped_from_cursor_col = None;
    }

    fn move_cursor(&self, s: &mut Inner, delta_line: i64, delta_col: i64) {
        s.last_action = None;
        let vls = Self::build_visual_line_map(s, s.last_width);
        let current = Self::find_current_visual_line(s, &vls);
        if delta_line != 0 {
            let target = current as i64 + delta_line;
            if target >= 0 && (target as usize) < vls.len() {
                self.move_to_visual_line(s, &vls, current, target as usize);
            }
        }
        if delta_col != 0 {
            let line = s.state.lines[s.state.cursor_line].clone();
            if delta_col > 0 {
                if s.state.cursor_col < line.len() {
                    let segs = Self::segment(s, &line[s.state.cursor_col..], false);
                    let len = segs.first().map_or(1, |g| g.segment.len());
                    set_cursor_col(s, s.state.cursor_col + len);
                } else if s.state.cursor_line + 1 < s.state.lines.len() {
                    s.state.cursor_line += 1;
                    set_cursor_col(s, 0);
                } else if let Some(vl) = vls.get(current) {
                    s.preferred_visual_col = Some(u16_span(&line, vl.start_col, s.state.cursor_col));
                }
            } else if s.state.cursor_col > 0 {
                let segs = Self::segment(s, &line[..s.state.cursor_col], false);
                let len = segs.last().map_or(1, |g| g.segment.len());
                set_cursor_col(s, s.state.cursor_col - len);
            } else if s.state.cursor_line > 0 {
                s.state.cursor_line -= 1;
                let len = s.state.lines[s.state.cursor_line].len();
                set_cursor_col(s, len);
            }
        }
        if s.ac_state.is_some() {
            self.update_autocomplete(s);
        }
    }

    fn page_scroll(&self, s: &mut Inner, direction: i64) {
        s.last_action = None;
        let rows = self.terminal.rows();
        let page = ((rows as f64 * 0.3).floor() as i64).max(5);
        let vls = Self::build_visual_line_map(s, s.last_width);
        let current = Self::find_current_visual_line(s, &vls);
        let target = (current as i64 + direction * page).clamp(0, vls.len() as i64 - 1) as usize;
        self.move_to_visual_line(s, &vls, current, target);
    }

    fn word_nav_options<'a>(
        s: &'a Inner,
        seg_fn: &'a dyn Fn(&str) -> Vec<Segment>,
        atomic: &'a dyn Fn(&str) -> bool,
    ) -> WordNavigationOptions<'a> {
        let _ = s;
        WordNavigationOptions { segment: Some(seg_fn), is_atomic_segment: Some(atomic) }
    }

    fn move_word_backwards(&self, s: &mut Inner) {
        s.last_action = None;
        if s.state.cursor_col == 0 {
            if s.state.cursor_line > 0 {
                s.state.cursor_line -= 1;
                let len = s.state.lines[s.state.cursor_line].len();
                set_cursor_col(s, len);
            }
            return;
        }
        let line = s.state.lines[s.state.cursor_line].clone();
        let ids: Vec<u32> = s.pastes.iter().map(|(i, _)| *i).collect();
        let seg_fn = move |t: &str| -> Vec<Segment> {
            segment_with_markers(t, true, &ids)
                .into_iter()
                .map(|d| Segment { segment: d.segment, is_word_like: d.is_word_like })
                .collect()
        };
        let atomic = |t: &str| is_paste_marker(t);
        let col = find_word_backward(&line, s.state.cursor_col, &Self::word_nav_options(s, &seg_fn, &atomic));
        set_cursor_col(s, col);
    }

    fn move_word_forwards(&self, s: &mut Inner) {
        s.last_action = None;
        let line = s.state.lines[s.state.cursor_line].clone();
        if s.state.cursor_col >= line.len() {
            if s.state.cursor_line + 1 < s.state.lines.len() {
                s.state.cursor_line += 1;
                set_cursor_col(s, 0);
            }
            return;
        }
        let ids: Vec<u32> = s.pastes.iter().map(|(i, _)| *i).collect();
        let seg_fn = move |t: &str| -> Vec<Segment> {
            segment_with_markers(t, true, &ids)
                .into_iter()
                .map(|d| Segment { segment: d.segment, is_word_like: d.is_word_like })
                .collect()
        };
        let atomic = |t: &str| is_paste_marker(t);
        let col = find_word_forward(&line, s.state.cursor_col, &Self::word_nav_options(s, &seg_fn, &atomic));
        set_cursor_col(s, col);
    }

    fn delete_to_start_of_line(&self, s: &mut Inner) {
        exit_history_browsing(s);
        let line = s.state.lines[s.state.cursor_line].clone();
        if s.state.cursor_col > 0 {
            push_undo_snapshot(s);
            let deleted = &line[..s.state.cursor_col];
            let acc = s.last_action == Some(LastAction::Kill);
            s.kill_ring.push(deleted, true, acc);
            s.last_action = Some(LastAction::Kill);
            s.state.lines[s.state.cursor_line] = line[s.state.cursor_col..].to_string();
            set_cursor_col(s, 0);
        } else if s.state.cursor_line > 0 {
            push_undo_snapshot(s);
            let acc = s.last_action == Some(LastAction::Kill);
            s.kill_ring.push("\n", true, acc);
            s.last_action = Some(LastAction::Kill);
            merge_with_previous_line(s);
        }
        emit_change(s);
    }

    fn delete_to_end_of_line(&self, s: &mut Inner) {
        exit_history_browsing(s);
        let line = s.state.lines[s.state.cursor_line].clone();
        if s.state.cursor_col < line.len() {
            push_undo_snapshot(s);
            let deleted = &line[s.state.cursor_col..];
            let acc = s.last_action == Some(LastAction::Kill);
            s.kill_ring.push(deleted, false, acc);
            s.last_action = Some(LastAction::Kill);
            s.state.lines[s.state.cursor_line] = line[..s.state.cursor_col].to_string();
        } else if s.state.cursor_line + 1 < s.state.lines.len() {
            push_undo_snapshot(s);
            let acc = s.last_action == Some(LastAction::Kill);
            s.kill_ring.push("\n", false, acc);
            s.last_action = Some(LastAction::Kill);
            let next = s.state.lines.remove(s.state.cursor_line + 1);
            s.state.lines[s.state.cursor_line].push_str(&next);
        }
        emit_change(s);
    }

    fn delete_word_backwards(&self, s: &mut Inner) {
        exit_history_browsing(s);
        let line = s.state.lines[s.state.cursor_line].clone();
        if s.state.cursor_col == 0 {
            if s.state.cursor_line > 0 {
                push_undo_snapshot(s);
                let acc = s.last_action == Some(LastAction::Kill);
                s.kill_ring.push("\n", true, acc);
                s.last_action = Some(LastAction::Kill);
                merge_with_previous_line(s);
            }
        } else {
            push_undo_snapshot(s);
            let was_kill = s.last_action == Some(LastAction::Kill);
            let old = s.state.cursor_col;
            self.move_word_backwards(s);
            let from = s.state.cursor_col;
            set_cursor_col(s, old);
            s.kill_ring.push(&line[from..old], true, was_kill);
            s.last_action = Some(LastAction::Kill);
            s.state.lines[s.state.cursor_line] = format!("{}{}", &line[..from], &line[old..]);
            set_cursor_col(s, from);
        }
        emit_change(s);
    }

    fn delete_word_forward(&self, s: &mut Inner) {
        exit_history_browsing(s);
        let line = s.state.lines[s.state.cursor_line].clone();
        if s.state.cursor_col >= line.len() {
            if s.state.cursor_line + 1 < s.state.lines.len() {
                push_undo_snapshot(s);
                let acc = s.last_action == Some(LastAction::Kill);
                s.kill_ring.push("\n", false, acc);
                s.last_action = Some(LastAction::Kill);
                let next = s.state.lines.remove(s.state.cursor_line + 1);
                s.state.lines[s.state.cursor_line].push_str(&next);
            }
        } else {
            push_undo_snapshot(s);
            let was_kill = s.last_action == Some(LastAction::Kill);
            let old = s.state.cursor_col;
            self.move_word_forwards(s);
            let to = s.state.cursor_col;
            set_cursor_col(s, old);
            s.kill_ring.push(&line[old..to], false, was_kill);
            s.last_action = Some(LastAction::Kill);
            s.state.lines[s.state.cursor_line] = format!("{}{}", &line[..old], &line[to..]);
        }
        emit_change(s);
    }

    fn yank(&self, s: &mut Inner) {
        if s.kill_ring.is_empty() {
            return;
        }
        push_undo_snapshot(s);
        let text = s.kill_ring.peek().unwrap_or("").to_string();
        insert_yanked_text(s, &text);
        s.last_action = Some(LastAction::Yank);
    }

    fn yank_pop(&self, s: &mut Inner) {
        if s.last_action != Some(LastAction::Yank) || s.kill_ring.len() <= 1 {
            return;
        }
        push_undo_snapshot(s);
        delete_yanked_text(s);
        s.kill_ring.rotate();
        let text = s.kill_ring.peek().unwrap_or("").to_string();
        insert_yanked_text(s, &text);
        s.last_action = Some(LastAction::Yank);
    }

    fn undo(&self, s: &mut Inner) {
        exit_history_browsing(s);
        let Some(snapshot) = s.undo.pop() else { return };
        s.state = snapshot.state;
        s.pastes = snapshot.pastes;
        s.paste_counter = snapshot.paste_counter;
        s.last_action = None;
        s.preferred_visual_col = None;
        emit_change(s);
    }

    fn jump_to_char(&self, s: &mut Inner, ch: &str, dir: JumpDir) {
        s.last_action = None;
        let n = s.state.lines.len();
        let line_indices: Vec<usize> = match dir {
            JumpDir::Forward => (s.state.cursor_line..n).collect(),
            JumpDir::Backward => (0..=s.state.cursor_line).rev().collect(),
        };
        for idx in line_indices {
            let line = s.state.lines[idx].clone();
            let is_current = idx == s.state.cursor_line;
            let found = match dir {
                JumpDir::Forward => {
                    let from = if is_current {
                        let col = s.state.cursor_col;
                        if col >= line.len() { line.len() + 1 } else { col + line[col..].chars().next().map_or(1, char::len_utf8) }
                    } else {
                        0
                    };
                    if from > line.len() { None } else { line[from..].find(ch).map(|i| i + from) }
                }
                JumpDir::Backward => {
                    if is_current {
                        let col = s.state.cursor_col;
                        if col == 0 {
                            if line.starts_with(ch) { Some(0) } else { None }
                        } else {
                            let prev_len = line[..col].chars().last().map_or(1, char::len_utf8);
                            let limit = (col - prev_len + ch.len()).min(line.len());
                            line[..limit].rfind(ch)
                        }
                    } else {
                        line.rfind(ch)
                    }
                }
            };
            if let Some(i) = found {
                s.state.cursor_line = idx;
                set_cursor_col(s, i);
                return;
            }
        }
    }

    fn navigate_history(&self, s: &mut Inner, direction: i64) {
        s.last_action = None;
        if s.history.is_empty() {
            return;
        }
        let new_index = s.history_index - direction;
        if new_index < -1 || new_index >= s.history.len() as i64 {
            return;
        }
        if s.history_index == -1 && new_index >= 0 {
            push_undo_snapshot(s);
            s.history_draft = Some(s.state.clone());
        }
        s.history_index = new_index;
        if s.history_index == -1 {
            match s.history_draft.take() {
                Some(draft) => {
                    s.state = draft;
                    s.preferred_visual_col = None;
                    s.snapped_from_cursor_col = None;
                    s.scroll_offset = 0;
                    emit_change(s);
                }
                None => set_text_internal(s, "", false),
            }
        } else {
            let text = s.history[s.history_index as usize].clone();
            set_text_internal(s, &text, direction == -1);
        }
    }

    fn is_on_first_visual_line(s: &Inner) -> bool {
        let vls = Self::build_visual_line_map(s, s.last_width);
        Self::find_current_visual_line(s, &vls) == 0
    }

    fn is_on_last_visual_line(s: &Inner) -> bool {
        let vls = Self::build_visual_line_map(s, s.last_width);
        Self::find_current_visual_line(s, &vls) + 1 == vls.len()
    }

    fn apply_selected_completion(&self, s: &mut Inner, selected: &AutocompleteItem) {
        let Some(provider) = s.provider.clone() else { return };
        push_undo_snapshot(s);
        s.last_action = None;
        let prefix = s.ac_prefix.clone();
        let r = completion_for(s, &provider, selected, &prefix);
        s.state.lines = r.lines;
        s.state.cursor_line = r.cursor_line;
        set_cursor_col(s, r.cursor_col);
    }

    /// When `data` will accept the selected autocomplete item (tab / confirm while the list is
    /// shown), prepare the provider call to run before `handle_input_locked` takes the lock.
    fn precompute_selected_completion(&self, data: &str) -> Option<impl FnOnce() -> PrecomputedCompletion + use<>> {
        let kb = get_keybindings();
        if !(kb.matches(data, "tui.input.tab") || kb.matches(data, "tui.select.confirm")) {
            return None;
        }
        let s = self.lock();
        if s.jump_mode.is_some() || s.is_in_paste || s.ac_state.is_none() || s.ac_list.is_none() {
            return None;
        }
        let provider = s.provider.clone()?;
        let item = Self::selected_autocomplete_item(&s)?;
        Some(PrecomputedCompletion::compute(&provider, &s, item, s.ac_prefix.clone()))
    }

    /// Process input with the state locked. Returns an autocomplete list key to forward.
    fn handle_input_locked(&self, s: &mut Inner, data: &str) -> Option<(Arc<SelectList>, String)> {
        let kb = get_keybindings();

        if let Some(dir) = s.jump_mode {
            if kb.matches(data, "tui.editor.jumpForward") || kb.matches(data, "tui.editor.jumpBackward") {
                s.jump_mode = None;
                return None;
            }
            let first_code = data.encode_utf16().next().unwrap_or(0);
            let printable = decode_printable_key(data).or_else(|| if first_code >= 32 { Some(data.to_string()) } else { None });
            if let Some(p) = printable {
                s.jump_mode = None;
                self.jump_to_char(s, &p, dir);
                return None;
            }
            s.jump_mode = None;
        }

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
                if !content.is_empty() {
                    self.handle_paste(s, &content);
                }
                s.is_in_paste = false;
                let remaining = s.paste_buffer[end + 6..].to_string();
                s.paste_buffer.clear();
                if !remaining.is_empty() {
                    return self.handle_input_locked(s, &remaining);
                }
            }
            return None;
        }
        let data = data.as_str();

        if kb.matches(data, "tui.input.copy") {
            return None;
        }
        if kb.matches(data, "tui.editor.undo") {
            self.undo(s);
            return None;
        }

        if s.ac_state.is_some() && s.ac_list.is_some() {
            if kb.matches(data, "tui.select.cancel") {
                self.cancel_autocomplete(s);
                return None;
            }
            if kb.matches(data, "tui.select.up") || kb.matches(data, "tui.select.down") {
                return s.ac_list.clone().map(|l| (l, data.to_string()));
            }
            if kb.matches(data, "tui.input.tab") {
                if let Some(selected) = Self::selected_autocomplete_item(s)
                    && s.provider.is_some()
                {
                    self.apply_selected_completion(s, &selected);
                    self.cancel_autocomplete(s);
                    emit_change(s);
                }
                return None;
            }
            if kb.matches(data, "tui.select.confirm")
                && let Some(selected) = Self::selected_autocomplete_item(s)
                && s.provider.is_some()
            {
                self.apply_selected_completion(s, &selected);
                if s.ac_prefix.starts_with('/') {
                    self.cancel_autocomplete(s);
                    // Fall through to submit.
                } else {
                    self.cancel_autocomplete(s);
                    emit_change(s);
                    return None;
                }
            }
        }

        if kb.matches(data, "tui.input.tab") && s.ac_state.is_none() {
            self.handle_tab_completion(s);
            return None;
        }

        if kb.matches(data, "tui.editor.deleteToLineEnd") {
            self.delete_to_end_of_line(s);
            return None;
        }
        if kb.matches(data, "tui.editor.deleteToLineStart") {
            self.delete_to_start_of_line(s);
            return None;
        }
        if kb.matches(data, "tui.editor.deleteWordBackward") {
            self.delete_word_backwards(s);
            return None;
        }
        if kb.matches(data, "tui.editor.deleteWordForward") {
            self.delete_word_forward(s);
            return None;
        }
        if kb.matches(data, "tui.editor.deleteCharBackward") || matches_key(data, "shift+backspace") {
            self.handle_backspace(s);
            return None;
        }
        if kb.matches(data, "tui.editor.deleteCharForward") || matches_key(data, "shift+delete") {
            self.handle_forward_delete(s);
            return None;
        }
        if kb.matches(data, "tui.editor.yank") {
            self.yank(s);
            return None;
        }
        if kb.matches(data, "tui.editor.yankPop") {
            self.yank_pop(s);
            return None;
        }
        if kb.matches(data, "tui.editor.historyPrevious") {
            self.cancel_autocomplete(s);
            self.navigate_history(s, -1);
            return None;
        }
        if kb.matches(data, "tui.editor.historyNext") {
            self.cancel_autocomplete(s);
            self.navigate_history(s, 1);
            return None;
        }
        if kb.matches(data, "tui.editor.cursorLineStart") {
            s.last_action = None;
            set_cursor_col(s, 0);
            return None;
        }
        if kb.matches(data, "tui.editor.cursorLineEnd") {
            s.last_action = None;
            let len = s.state.lines[s.state.cursor_line].len();
            set_cursor_col(s, len);
            return None;
        }
        if kb.matches(data, "tui.editor.cursorWordLeft") {
            self.move_word_backwards(s);
            return None;
        }
        if kb.matches(data, "tui.editor.cursorWordRight") {
            self.move_word_forwards(s);
            return None;
        }

        let units: Vec<u16> = data.encode_utf16().collect();
        let first = units.first().copied().unwrap_or(0);
        if kb.matches(data, "tui.input.newLine")
            || (first == 10 && units.len() > 1)
            || data == "\x1b\r"
            || data == "\x1b[13;2~"
            || (units.len() > 1 && data.contains('\x1b') && data.contains('\r'))
            || data == "\n"
        {
            if Self::should_submit_on_backslash_enter(s, data, &kb) {
                self.handle_backspace(s);
                self.submit_value(s);
                return None;
            }
            self.add_new_line(s);
            return None;
        }

        if kb.matches(data, "tui.input.submit") {
            if s.disable_submit {
                return None;
            }
            if char_before_cursor_is_backslash(s) {
                self.handle_backspace(s);
                self.add_new_line(s);
                return None;
            }
            self.submit_value(s);
            return None;
        }

        if kb.matches(data, "tui.editor.cursorUp") {
            let first_vl = Self::is_on_first_visual_line(s);
            if first_vl && (is_editor_empty(s) || s.history_index > -1 || s.state.cursor_col == 0) {
                self.navigate_history(s, -1);
            } else if first_vl {
                s.last_action = None;
                set_cursor_col(s, 0);
            } else {
                self.move_cursor(s, -1, 0);
            }
            return None;
        }
        if kb.matches(data, "tui.editor.cursorDown") {
            let last_vl = Self::is_on_last_visual_line(s);
            if s.history_index > -1 && last_vl {
                self.navigate_history(s, 1);
            } else if last_vl {
                s.last_action = None;
                let len = s.state.lines[s.state.cursor_line].len();
                set_cursor_col(s, len);
            } else {
                self.move_cursor(s, 1, 0);
            }
            return None;
        }
        if kb.matches(data, "tui.editor.cursorRight") {
            self.move_cursor(s, 0, 1);
            return None;
        }
        if kb.matches(data, "tui.editor.cursorLeft") {
            self.move_cursor(s, 0, -1);
            return None;
        }
        if kb.matches(data, "tui.editor.pageUp") {
            self.page_scroll(s, -1);
            return None;
        }
        if kb.matches(data, "tui.editor.pageDown") {
            self.page_scroll(s, 1);
            return None;
        }
        if kb.matches(data, "tui.editor.jumpForward") {
            s.jump_mode = Some(JumpDir::Forward);
            return None;
        }
        if kb.matches(data, "tui.editor.jumpBackward") {
            s.jump_mode = Some(JumpDir::Backward);
            return None;
        }
        if matches_key(data, "shift+space") {
            self.insert_character(s, " ", false);
            return None;
        }
        if let Some(p) = decode_printable_key(data) {
            self.insert_character(s, &p, false);
            return None;
        }
        if first >= 32 {
            self.insert_character(s, data, false);
        }
        None
    }
}

// ---- free helpers on Inner ----

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ")
}

fn expand_paste_markers(pastes: &[(u32, String)], text: &str) -> String {
    let mut result = text.to_string();
    for (id, content) in pastes {
        let re = Regex::new(&format!(r"\[paste #{id}( (\+\d+ lines|\d+ chars))?\]")).unwrap();
        result = re.replace_all(&result, regex::NoExpand(content)).to_string();
    }
    result
}

fn text_before_cursor(s: &Inner) -> String {
    let line = &s.state.lines[s.state.cursor_line];
    line[..s.state.cursor_col.min(line.len())].to_string()
}

fn char_before_cursor_is_backslash(s: &Inner) -> bool {
    let line = &s.state.lines[s.state.cursor_line];
    s.state.cursor_col > 0 && line.as_bytes().get(s.state.cursor_col - 1) == Some(&b'\\')
}

fn emit_change(s: &mut Inner) {
    let text = s.state.lines.join("\n");
    s.events.push(Event::Change(text));
}

fn set_cursor_col(s: &mut Inner, col: usize) {
    s.state.cursor_col = col;
    s.preferred_visual_col = None;
    s.snapped_from_cursor_col = None;
}

fn push_undo_snapshot(s: &mut Inner) {
    let snap = EditorSnapshot { state: s.state.clone(), pastes: s.pastes.clone(), paste_counter: s.paste_counter };
    s.undo.push(&snap);
}

fn exit_history_browsing(s: &mut Inner) {
    s.history_index = -1;
    s.history_draft = None;
}

fn is_editor_empty(s: &Inner) -> bool {
    s.state.lines.len() == 1 && s.state.lines[0].is_empty()
}

fn is_slash_menu_allowed(s: &Inner) -> bool {
    s.state.cursor_line == 0
}

fn is_at_start_of_message(s: &Inner) -> bool {
    if !is_slash_menu_allowed(s) {
        return false;
    }
    let before = text_before_cursor(s);
    let t = js_trim(&before);
    t.is_empty() || t == "/"
}

fn is_in_slash_command_context(s: &Inner, before: &str) -> bool {
    is_slash_menu_allowed(s) && js_trim_start(before).starts_with('/')
}

fn set_text_internal(s: &mut Inner, text: &str, cursor_at_start: bool) {
    let lines: Vec<String> = text.split('\n').map(String::from).collect();
    s.state.lines = if lines.is_empty() { vec![String::new()] } else { lines };
    s.state.cursor_line = if cursor_at_start { 0 } else { s.state.lines.len() - 1 };
    let col = if cursor_at_start { 0 } else { s.state.lines[s.state.cursor_line].len() };
    set_cursor_col(s, col);
    s.scroll_offset = 0;
    emit_change(s);
}

fn insert_text_at_cursor_internal(s: &mut Inner, text: &str) {
    if text.is_empty() {
        return;
    }
    let normalized = normalize_text(text);
    let inserted: Vec<&str> = normalized.split('\n').collect();
    let line = s.state.lines[s.state.cursor_line].clone();
    let col = s.state.cursor_col.min(line.len());
    let (before, after) = line.split_at(col);
    if inserted.len() == 1 {
        s.state.lines[s.state.cursor_line] = format!("{before}{normalized}{after}");
        set_cursor_col(s, col + normalized.len());
    } else {
        let mut new_lines: Vec<String> = s.state.lines[..s.state.cursor_line].to_vec();
        new_lines.push(format!("{before}{}", inserted[0]));
        for l in &inserted[1..inserted.len() - 1] {
            new_lines.push(l.to_string());
        }
        let last = inserted[inserted.len() - 1];
        new_lines.push(format!("{last}{after}"));
        new_lines.extend(s.state.lines[s.state.cursor_line + 1..].iter().cloned());
        s.state.lines = new_lines;
        s.state.cursor_line += inserted.len() - 1;
        set_cursor_col(s, last.len());
    }
    emit_change(s);
}

fn merge_with_previous_line(s: &mut Inner) {
    let current = s.state.lines.remove(s.state.cursor_line);
    s.state.cursor_line -= 1;
    let prev_len = s.state.lines[s.state.cursor_line].len();
    s.state.lines[s.state.cursor_line].push_str(&current);
    set_cursor_col(s, prev_len);
}

fn insert_yanked_text(s: &mut Inner, text: &str) {
    exit_history_browsing(s);
    let lines: Vec<&str> = text.split('\n').collect();
    let line = s.state.lines[s.state.cursor_line].clone();
    let col = s.state.cursor_col.min(line.len());
    let (before, after) = line.split_at(col);
    if lines.len() == 1 {
        s.state.lines[s.state.cursor_line] = format!("{before}{text}{after}");
        set_cursor_col(s, col + text.len());
    } else {
        let cl = s.state.cursor_line;
        s.state.lines[cl] = format!("{before}{}", lines[0]);
        for (i, l) in lines.iter().enumerate().take(lines.len() - 1).skip(1) {
            s.state.lines.insert(cl + i, l.to_string());
        }
        let last_idx = cl + lines.len() - 1;
        let last = lines[lines.len() - 1];
        s.state.lines.insert(last_idx, format!("{last}{after}"));
        s.state.cursor_line = last_idx;
        set_cursor_col(s, last.len());
    }
    emit_change(s);
}

fn delete_yanked_text(s: &mut Inner) {
    let Some(yanked) = s.kill_ring.peek().map(String::from) else { return };
    if yanked.is_empty() {
        return;
    }
    let yank_lines: Vec<&str> = yanked.split('\n').collect();
    if yank_lines.len() == 1 {
        let line = s.state.lines[s.state.cursor_line].clone();
        let col = s.state.cursor_col.min(line.len());
        let start = col.saturating_sub(yanked.len());
        s.state.lines[s.state.cursor_line] = format!("{}{}", &line[..start], &line[col..]);
        set_cursor_col(s, start);
    } else {
        let start_line = s.state.cursor_line + 1 - yank_lines.len();
        let start_col = s.state.lines[start_line].len().saturating_sub(yank_lines[0].len());
        let after = s.state.lines[s.state.cursor_line][s.state.cursor_col..].to_string();
        let before = s.state.lines[start_line][..start_col].to_string();
        s.state.lines.splice(start_line..start_line + yank_lines.len(), std::iter::once(format!("{before}{after}")));
        s.state.cursor_line = start_line;
        set_cursor_col(s, start_col);
    }
    emit_change(s);
}

/// UTF-16 length of `line[from..to]` (clamped to char boundaries).
fn u16_span(line: &str, from: usize, to: usize) -> usize {
    let to = to.min(line.len());
    if from >= to {
        return 0;
    }
    line.get(from..to).map_or(0, crate::utils::utf16_len)
}

/// Byte offset reached by advancing `units` UTF-16 code units from `start`
/// (clamped to the end of the line; never splits a character).
fn byte_at_u16(line: &str, start: usize, units: usize) -> usize {
    let mut remaining = units;
    let mut pos = start.min(line.len());
    for ch in line[pos..].chars() {
        let w = ch.len_utf16();
        if remaining < w {
            break;
        }
        remaining -= w;
        pos += ch.len_utf8();
        if remaining == 0 {
            break;
        }
    }
    pos
}

fn compute_vertical_move_column(s: &mut Inner, current: usize, source_max: usize, target_max: usize) -> usize {
    let has_preferred = s.preferred_visual_col.is_some();
    let cursor_in_middle = current < source_max;
    let target_too_short = target_max < current;
    if !has_preferred || cursor_in_middle {
        if target_too_short {
            s.preferred_visual_col = Some(current);
            return target_max;
        }
        s.preferred_visual_col = None;
        return current;
    }
    let preferred = s.preferred_visual_col.unwrap();
    if target_too_short || target_max < preferred {
        return target_max;
    }
    s.preferred_visual_col = None;
    preferred
}

fn best_autocomplete_match_index(items: &[AutocompleteItem], prefix: &str) -> Option<usize> {
    if prefix.is_empty() {
        return None;
    }
    let mut first_prefix = None;
    for (i, item) in items.iter().enumerate() {
        if item.value == prefix {
            return Some(i);
        }
        if first_prefix.is_none() && item.value.starts_with(prefix) {
            first_prefix = Some(i);
        }
    }
    first_prefix
}

impl Component for Editor {
    fn render(&self, width: usize) -> Vec<String> {
        let rows = self.terminal.rows();
        let top_border = self.cbs().top_border.clone();
        let (mut result, list, content_width, left, right, top_hidden) = {
            let mut s = self.lock();
            let max_padding = width.saturating_sub(1) / 2;
            let padding_x = s.padding_x.min(max_padding);
            let content_width = width.saturating_sub(padding_x * 2).max(1);
            let layout_width = content_width.saturating_sub(if padding_x > 0 { 0 } else { 1 }).max(1);
            s.last_width = layout_width;
            let layout_lines = Self::layout_text(&s, layout_width);
            let max_visible = ((rows as f64 * 0.3).floor() as usize).max(5);
            let cursor_idx = layout_lines.iter().position(|l| l.has_cursor).unwrap_or(0);
            if cursor_idx < s.scroll_offset {
                s.scroll_offset = cursor_idx;
            } else if cursor_idx >= s.scroll_offset + max_visible {
                s.scroll_offset = cursor_idx + 1 - max_visible;
            }
            let max_scroll = layout_lines.len().saturating_sub(max_visible);
            s.scroll_offset = s.scroll_offset.min(max_scroll);
            let end = (s.scroll_offset + max_visible).min(layout_lines.len());
            let visible = &layout_lines[s.scroll_offset..end];
            s.rendered_visible_line_count = visible.len();

            let mut result = Vec::new();
            let left = " ".repeat(padding_x);
            let right = left.clone();
            let top = if s.scroll_offset > 0 { create_scroll_border("↑", s.scroll_offset, width) } else { "─".repeat(width) };
            result.push((s.border_color)(&top));
            let marker = if s.focused { CURSOR_MARKER } else { "" };
            for ll in visible {
                let mut display = ll.text.clone();
                let mut line_width = visible_width(&ll.text);
                let mut cursor_in_padding = false;
                if let (true, Some(pos)) = (ll.has_cursor, ll.cursor_pos) {
                    let pos = pos.min(display.len());
                    let before = &ll.text[..pos];
                    let after = &ll.text[pos..];
                    if !after.is_empty() {
                        let segs = Self::segment(&s, after, false);
                        let first = segs.first().map(|g| g.segment.clone()).unwrap_or_default();
                        let rest = &after[first.len()..];
                        display = format!("{before}{marker}{}{rest}", render_fake_cursor(&first));
                    } else {
                        display = format!("{before}{marker}{}", render_fake_cursor(" "));
                        line_width += 1;
                        if line_width > content_width && padding_x > 0 {
                            cursor_in_padding = true;
                        }
                    }
                }
                let padding = " ".repeat(content_width.saturating_sub(line_width));
                let line_right = if cursor_in_padding { &right[1..] } else { &right[..] };
                result.push(format!("{left}{display}{padding}{line_right}"));
            }
            let below = layout_lines.len() - (s.scroll_offset + visible.len());
            let bottom = if below > 0 { create_scroll_border("↓", below, width) } else { "─".repeat(width) };
            result.push((s.border_color)(&bottom));
            s.rendered_autocomplete_height = 0;
            let list = if s.ac_state.is_some() { s.ac_list.clone() } else { None };
            let top_hidden = s.scroll_offset;
            (result, list, content_width, left, right, top_hidden)
        };
        if let Some(top) = top_border.and_then(|f| f(width, top_hidden)) {
            result[0] = top;
        }
        if let Some(list) = list {
            let lines = list.render(content_width);
            self.lock().rendered_autocomplete_height = lines.len();
            for line in lines {
                let pad = " ".repeat(content_width.saturating_sub(visible_width(&line)));
                result.push(format!("{left}{line}{pad}{right}"));
            }
        }
        result
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        let (list, start_row, height, padding_x) = {
            let s = self.lock();
            let max_padding = (event.width - 1).max(0) / 2;
            let padding_x = (s.padding_x as i64).min(max_padding);
            let list = if s.ac_state.is_some() { s.ac_list.clone() } else { None };
            (list, s.rendered_visible_line_count as i64 + 2, s.rendered_autocomplete_height as i64, padding_x)
        };
        if let Some(list) = list
            && event.y >= start_row && event.y < start_row + height
        {
            let content_width = (event.width - padding_x * 2).max(1);
            let local = MouseEvent { x: event.x - padding_x, y: event.y - start_row, width: content_width, height, ..*event };
            let mut r = list.handle_mouse(&local)?;
            r.focus = true;
            return Some(r);
        }
        if event.kind != MouseEventType::Click || event.button != MouseButton::Left {
            return None;
        }
        let focus = Some(MouseResult { handled: true, focus: true, ..Default::default() });
        self.with(|s| {
            if event.y <= 0 || event.y > s.rendered_visible_line_count as i64 {
                return;
            }
            let vls = Self::build_visual_line_map(s, s.last_width);
            let vl_index = s.scroll_offset + event.y as usize - 1;
            let Some(vl) = vls.get(vl_index).copied() else { return };
            let logical = s.state.lines[vl.logical_line].clone();
            let chunk_end = (vl.start_col + vl.length).min(logical.len());
            let chunk = &logical[vl.start_col..chunk_end];
            let target_column = (event.x - padding_x).max(0) as usize;
            let mut visible_column = 0;
            let mut target_index = chunk.len();
            let mut last_grapheme_index = 0;
            for g in Self::segment(s, chunk, false) {
                let next = visible_column + visible_width(&g.segment);
                last_grapheme_index = g.index;
                if target_column < next {
                    target_index = g.index;
                    break;
                }
                visible_column = next;
            }
            let is_last = vl_index == vls.len() - 1 || vls[vl_index + 1].logical_line != vl.logical_line;
            if !is_last && target_index == chunk.len() && !chunk.is_empty() {
                target_index = last_grapheme_index;
            }
            s.state.cursor_line = vl.logical_line;
            set_cursor_col(s, vl.start_col + target_index);
            s.last_action = None;
            exit_history_browsing(s);
            if s.ac_state.is_some() {
                self.update_autocomplete(s);
            }
        });
        focus
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn handle_input(&self, data: &str) {
        let precomputed = self.precompute_selected_completion(data).map(|f| f());
        let forward = self.with(|s| {
            s.precomputed_completion = precomputed;
            let forward = self.handle_input_locked(s, data);
            s.precomputed_completion = None;
            forward
        });
        if let Some((list, key)) = forward {
            list.handle_input(&key);
        }
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
}

/// Interface for custom editor components (e.g. vim-mode editors provided by extensions).
pub trait EditorComponent: Component {
    fn get_text(&self) -> String;
    fn set_text(&self, text: &str);
    fn set_on_submit(&self, f: Option<TextCallback>);
    fn set_on_change(&self, f: Option<TextCallback>);
    fn add_to_history(&self, _text: &str) {}
    fn insert_text_at_cursor(&self, _text: &str) {}
    fn get_expanded_text(&self) -> String {
        self.get_text()
    }
    fn set_autocomplete_provider(&self, _provider: Arc<dyn AutocompleteProvider>) {}
    fn set_border_color(&self, _f: StyleFn) {}
    fn set_padding_x(&self, _padding: usize) {}
    fn set_autocomplete_max_visible(&self, _max_visible: usize) {}
}

impl EditorComponent for Editor {
    fn get_text(&self) -> String {
        Editor::get_text(self)
    }
    fn set_text(&self, text: &str) {
        Editor::set_text(self, text)
    }
    fn set_on_submit(&self, f: Option<TextCallback>) {
        Editor::set_on_submit(self, f)
    }
    fn set_on_change(&self, f: Option<TextCallback>) {
        Editor::set_on_change(self, f)
    }
    fn add_to_history(&self, text: &str) {
        Editor::add_to_history(self, text)
    }
    fn insert_text_at_cursor(&self, text: &str) {
        Editor::insert_text_at_cursor(self, text)
    }
    fn get_expanded_text(&self) -> String {
        Editor::get_expanded_text(self)
    }
    fn set_autocomplete_provider(&self, provider: Arc<dyn AutocompleteProvider>) {
        Editor::set_autocomplete_provider(self, provider)
    }
    fn set_border_color(&self, f: StyleFn) {
        Editor::set_border_color(self, f)
    }
    fn set_padding_x(&self, padding: usize) {
        Editor::set_padding_x(self, padding)
    }
    fn set_autocomplete_max_visible(&self, max_visible: usize) {
        Editor::set_autocomplete_max_visible(self, max_visible)
    }
}
