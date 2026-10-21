//! Transcript search for the fullscreen renderer. Port of `alt-screen-search.ts`.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::StyleFn;
use crate::components::input::{Input, InputOptions};
use crate::keybindings::get_keybindings;
use crate::tui::Component;
use crate::utils::{graphemes, strip_terminal_sequences, truncate_to_width, visible_width};

/// JavaScript `\s`.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{0B}'
            | '\u{0C}'
            | '\r'
            | ' '
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

#[derive(Debug, Clone)]
struct SearchSourceSpan {
    text_start: usize,
    text_end: usize,
    row: usize,
    start_col: usize,
    end_col: usize,
    linear_columns: bool,
}

#[derive(Debug, Clone, Default)]
struct SearchCorpus {
    text: String,
    spans: Vec<SearchSourceSpan>,
}

/// A highlighted cell range on one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AltScreenSearchSegment {
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
}

/// One search match, possibly spanning several rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AltScreenSearchMatch {
    pub segments: Vec<AltScreenSearchSegment>,
}

fn build_search_corpus(lines: &[String]) -> SearchCorpus {
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut pending_separator = false;

    for (row, raw) in lines.iter().enumerate() {
        let line = strip_terminal_sequences(raw);
        let mut column = 0;
        if line.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
            let bytes = line.as_bytes();
            let mut index = 0;
            while index < bytes.len() {
                if bytes[index] == b' ' {
                    if !text.is_empty() {
                        pending_separator = true;
                    }
                    column += 1;
                    index += 1;
                    continue;
                }
                let mut end = index + 1;
                while end < bytes.len() && bytes[end] != b' ' {
                    end += 1;
                }
                if pending_separator {
                    text.push(' ');
                    pending_separator = false;
                }
                let chunk = &line[index..end];
                spans.push(SearchSourceSpan {
                    text_start: text.len(),
                    text_end: text.len() + chunk.len(),
                    row,
                    start_col: column,
                    end_col: column + chunk.len(),
                    linear_columns: true,
                });
                text.push_str(chunk);
                column += chunk.len();
                index = end;
            }
        } else {
            for grapheme in graphemes(&line) {
                let width = visible_width(grapheme);
                if !grapheme.is_empty() && grapheme.chars().all(is_js_space) {
                    if !text.is_empty() {
                        pending_separator = true;
                    }
                    column += width;
                    continue;
                }
                if pending_separator {
                    text.push(' ');
                    pending_separator = false;
                }
                spans.push(SearchSourceSpan {
                    text_start: text.len(),
                    text_end: text.len() + grapheme.len(),
                    row,
                    start_col: column,
                    end_col: column + width,
                    linear_columns: false,
                });
                text.push_str(grapheme);
                column += width;
            }
        }
        if !text.is_empty() {
            pending_separator = true;
        }
    }
    SearchCorpus { text, spans }
}

fn normalize_query(query: &str) -> String {
    let mut out = String::new();
    let mut in_space = false;
    for c in query.chars() {
        if is_js_space(c) {
            in_space = true;
        } else {
            if in_space && !out.is_empty() {
                out.push(' ');
            }
            in_space = false;
            out.push(c);
        }
    }
    out
}

fn find_search_corpus_matches(corpus: &SearchCorpus, normalized_query: &str) -> Vec<AltScreenSearchMatch> {
    if normalized_query.is_empty() {
        return Vec::new();
    }
    let Ok(expression) = regex::RegexBuilder::new(&regex::escape(normalized_query)).case_insensitive(true).build()
    else {
        return Vec::new();
    };
    let mut matches = Vec::new();
    let mut span_index = 0;
    for m in expression.find_iter(&corpus.text) {
        let (start, end) = (m.start(), m.end());
        while span_index < corpus.spans.len() && corpus.spans[span_index].text_end <= start {
            span_index += 1;
        }
        let mut segments: Vec<AltScreenSearchSegment> = Vec::new();
        for span in &corpus.spans[span_index..] {
            if span.text_start >= end {
                break;
            }
            if span.text_end <= start {
                continue;
            }
            let start_col = if span.linear_columns {
                span.start_col + start.max(span.text_start) - span.text_start
            } else {
                span.start_col
            };
            let end_col =
                if span.linear_columns { span.start_col + end.min(span.text_end) - span.text_start } else { span.end_col };
            match segments.last_mut() {
                Some(previous) if previous.row == span.row && start_col <= previous.end_col => {
                    previous.end_col = previous.end_col.max(end_col);
                }
                _ => segments.push(AltScreenSearchSegment { row: span.row, start_col, end_col }),
            }
        }
        while span_index < corpus.spans.len() && corpus.spans[span_index].text_end <= end {
            span_index += 1;
        }
        if !segments.is_empty() {
            matches.push(AltScreenSearchMatch { segments });
        }
    }
    matches
}

/// Result of [`AltScreenSearchIndex::search`].
#[derive(Debug, Clone)]
pub struct AltScreenSearchResult {
    pub matches: Vec<AltScreenSearchMatch>,
    pub changed: bool,
}

/// Caches the searchable corpus and matches while the rendered lines stay unchanged.
#[derive(Debug, Default)]
pub struct AltScreenSearchIndex {
    source_lines: Option<Vec<String>>,
    corpus: Option<SearchCorpus>,
    normalized_query: Option<String>,
    matches: Vec<AltScreenSearchMatch>,
}

impl AltScreenSearchIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn search(&mut self, lines: &[String], query: &str) -> AltScreenSearchResult {
        let source_changed = self.source_lines.as_deref() != Some(lines);
        if source_changed || self.corpus.is_none() {
            self.source_lines = Some(lines.to_vec());
            self.corpus = Some(build_search_corpus(lines));
        }
        let normalized = normalize_query(query);
        let changed = source_changed || self.normalized_query.as_deref() != Some(normalized.as_str());
        if changed {
            self.matches = find_search_corpus_matches(self.corpus.as_ref().unwrap(), &normalized);
            self.normalized_query = Some(normalized);
        }
        AltScreenSearchResult { matches: self.matches.clone(), changed }
    }
}

/// Find all matches of `query` in rendered `lines` (whitespace-normalized, case-insensitive).
pub fn find_alt_screen_search_matches(lines: &[String], query: &str) -> Vec<AltScreenSearchMatch> {
    let normalized = normalize_query(query);
    if normalized.is_empty() {
        return Vec::new();
    }
    find_search_corpus_matches(&build_search_corpus(lines), &normalized)
}

/// A stable key identifying a match by its first and last segment.
pub fn get_alt_screen_search_match_key(m: &AltScreenSearchMatch) -> String {
    match (m.segments.first(), m.segments.last()) {
        (Some(first), Some(last)) => format!("{}:{}:{}:{}", first.row, first.start_col, last.row, last.end_col),
        _ => String::new(),
    }
}

/// Style for a navigation button: `(text, hovered) -> styled`.
pub type NavigationButtonStyle = Arc<dyn Fn(&str, bool) -> String + Send + Sync>;

struct SearchState {
    result_count: usize,
    result_index: i64,
    previous_button_start: i64,
    previous_button_end: i64,
    next_button_start: i64,
    next_button_end: i64,
    hovered_navigation_direction: Option<i32>,
    focused: bool,
}

/// The search box overlay.
pub struct AltScreenSearchComponent {
    input: Input,
    on_query_change: Arc<dyn Fn(&str) + Send + Sync>,
    navigation_button_style: NavigationButtonStyle,
    st: Mutex<SearchState>,
}

impl AltScreenSearchComponent {
    pub fn new(on_query_change: Arc<dyn Fn(&str) + Send + Sync>, navigation_button_style: Option<NavigationButtonStyle>) -> Self {
        let dim: StyleFn = Arc::new(|t: &str| format!("\x1b[2m{t}\x1b[22m"));
        Self {
            input: Input::new(InputOptions {
                prompt: Some(" ".into()),
                placeholder: Some("Find in transcript".into()),
                placeholder_style: Some(dim),
            }),
            on_query_change,
            navigation_button_style: navigation_button_style.unwrap_or_else(|| Arc::new(|t: &str, _| t.to_string())),
            st: Mutex::new(SearchState {
                result_count: 0,
                result_index: -1,
                previous_button_start: -1,
                previous_button_end: -1,
                next_button_start: -1,
                next_button_end: -1,
                hovered_navigation_direction: None,
                focused: false,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, SearchState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_result(&self, index: i64, count: usize) {
        let mut s = self.lock();
        s.result_index = index;
        s.result_count = count;
    }

    /// Which navigation button (if any) is at `(row, column)` within the component.
    pub fn get_navigation_direction_at(&self, row: i64, column: i64) -> Option<i32> {
        if row != 2 {
            return None;
        }
        let s = self.lock();
        if column >= s.previous_button_start && column < s.previous_button_end {
            return Some(-1);
        }
        if column >= s.next_button_start && column < s.next_button_end {
            return Some(1);
        }
        None
    }

    /// Returns whether the hover state changed.
    pub fn set_hovered_navigation_direction(&self, direction: Option<i32>) -> bool {
        let mut s = self.lock();
        if s.hovered_navigation_direction == direction {
            return false;
        }
        s.hovered_navigation_direction = direction;
        true
    }

    pub fn query(&self) -> String {
        self.input.get_value()
    }
}

fn format_key(key: Option<&String>) -> String {
    match key {
        None => "Unbound".to_string(),
        Some(key) => key
            .split('+')
            .map(|part| {
                if cfg!(target_os = "macos") && part.eq_ignore_ascii_case("alt") {
                    return "Option".to_string();
                }
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("+"),
    }
}

impl Component for AltScreenSearchComponent {
    fn render(&self, width: usize) -> Vec<String> {
        let safe_width = width.max(1);
        let inner_width = safe_width.saturating_sub(2);
        let keybindings = get_keybindings();
        let previous_key = format_key(keybindings.get_keys("tui.altScreen.searchPrevious").first());
        let next_key = format_key(keybindings.get_keys("tui.altScreen.searchNext").first());
        let query = self.input.get_value();
        let (result_index, result_count, hovered) = {
            let s = self.lock();
            (s.result_index, s.result_count, s.hovered_navigation_direction)
        };
        let result = if query.is_empty() {
            String::new()
        } else if result_count == 0 {
            "No matches".to_string()
        } else {
            format!("{}/{}", result_index + 1, result_count)
        };
        let result_space = inner_width.saturating_sub(3);
        let visible_result = truncate_to_width(&result, result_space, "", false);
        let result_text = if visible_result.is_empty() { String::new() } else { format!("\x1b[2m {visible_result} \x1b[22m") };
        let input_width = inner_width.saturating_sub(visible_width(&result_text));
        let input_line = truncate_to_width(
            self.input.render(input_width.max(1)).first().map_or("", |s| s.as_str()),
            input_width,
            "",
            false,
        );
        let input_padding = " ".repeat(input_width.saturating_sub(visible_width(&input_line)));
        let content = format!("{input_line}{input_padding}{result_text}");

        let mut previous_button = format!("↑ {previous_key}");
        let mut next_button = format!("↓ {next_key}");
        let mut separator = " · ";
        let outer_gap_width = 1;
        let available_controls_width = inner_width.saturating_sub(outer_gap_width * 2 + 1);
        let mut controls_width = visible_width(&previous_button) + visible_width(separator) + visible_width(&next_button);
        if controls_width > available_controls_width {
            previous_button = "↑".into();
            next_button = "↓".into();
            separator = " ";
            controls_width = visible_width(&previous_button) + visible_width(separator) + visible_width(&next_button);
        }
        let show_buttons = controls_width <= available_controls_width;
        let rendered_buttons = if show_buttons {
            format!(
                "{}{separator}{}",
                (self.navigation_button_style)(&previous_button, hovered == Some(-1)),
                (self.navigation_button_style)(&next_button, hovered == Some(1))
            )
        } else {
            String::new()
        };
        let outer_gaps_width = if show_buttons { outer_gap_width * 2 } else { 0 };
        let right_rule_width = usize::from(!rendered_buttons.is_empty() && inner_width > controls_width + outer_gaps_width);
        let left_rule_width = inner_width
            .saturating_sub(if show_buttons { controls_width } else { 0 })
            .saturating_sub(outer_gaps_width)
            .saturating_sub(right_rule_width);
        {
            let mut s = self.lock();
            let previous_start = (1 + left_rule_width + outer_gap_width) as i64;
            s.previous_button_start = if show_buttons { previous_start } else { -1 };
            s.previous_button_end = if show_buttons { previous_start + visible_width(&previous_button) as i64 } else { -1 };
            s.next_button_start = if show_buttons { s.previous_button_end + visible_width(separator) as i64 } else { -1 };
            s.next_button_end = if show_buttons { s.next_button_start + visible_width(&next_button) as i64 } else { -1 };
        }

        if safe_width == 1 {
            return vec!["┌".into(), "│".into(), "└".into()];
        }
        let gap = if rendered_buttons.is_empty() { "" } else { " " };
        vec![
            format!("┌{}┐", "─".repeat(inner_width)),
            format!("│{content}│"),
            format!("└{}{gap}{rendered_buttons}{gap}{}┘", "─".repeat(left_rule_width), "─".repeat(right_rule_width)),
        ]
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn handle_input(&self, data: &str) {
        let previous = self.input.get_value();
        self.input.handle_input(data);
        let query = self.input.get_value();
        if query != previous {
            (self.on_query_change)(&query);
        }
    }

    fn invalidate(&self) {
        self.input.invalidate();
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        self.lock().focused = focused;
        self.input.set_focused(focused);
    }

    fn focused(&self) -> bool {
        self.lock().focused
    }
}
