//! Word-wise cursor movement. Port of `packages/tui/src/word-navigation.ts`.
//!
//! Cursor positions are byte offsets.

use crate::utils::{is_punctuation_ch, is_whitespace_char, word_segments};

/// An owned word segment (see [`crate::utils::WordSegment`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub segment: String,
    pub is_word_like: bool,
}

/// Options for word navigation.
#[derive(Default)]
pub struct WordNavigationOptions<'a> {
    /// Custom segmenter returning word segments for the given text.
    pub segment: Option<&'a dyn Fn(&str) -> Vec<Segment>>,
    /// Predicate identifying atomic segments treated as single units (e.g. paste markers).
    pub is_atomic_segment: Option<&'a dyn Fn(&str) -> bool>,
}

fn default_segments(text: &str) -> Vec<Segment> {
    word_segments(text)
        .into_iter()
        .map(|s| Segment { segment: s.segment.to_string(), is_word_like: s.is_word_like })
        .collect()
}

/// Cursor position after moving one word backward from `cursor` in `text`.
pub fn find_word_backward(text: &str, cursor: usize, options: &WordNavigationOptions) -> usize {
    if cursor == 0 {
        return 0;
    }
    let before = &text[..cursor.min(text.len())];
    let mut segments = match options.segment {
        Some(f) => f(before),
        None => default_segments(before),
    };
    let is_atomic = |s: &str| options.is_atomic_segment.is_some_and(|f| f(s));
    let mut new_cursor = cursor;

    while let Some(last) = segments.last() {
        if is_atomic(&last.segment) || !is_whitespace_char(&last.segment) {
            break;
        }
        new_cursor -= segments.pop().unwrap().segment.len();
    }
    let Some(last) = segments.last() else {
        return new_cursor;
    };

    if is_atomic(&last.segment) {
        new_cursor -= last.segment.len();
    } else if last.is_word_like {
        let seg = &last.segment;
        match seg.char_indices().filter(|(_, c)| is_punctuation_ch(*c)).last() {
            None => new_cursor -= seg.len(),
            Some((idx, c)) => new_cursor -= seg.len() - (idx + c.len_utf8()),
        }
    } else {
        while let Some(last) = segments.last() {
            if is_atomic(&last.segment) || last.is_word_like || is_whitespace_char(&last.segment) {
                break;
            }
            new_cursor -= segments.pop().unwrap().segment.len();
        }
    }
    new_cursor
}

/// Cursor position after moving one word forward from `cursor` in `text`.
pub fn find_word_forward(text: &str, cursor: usize, options: &WordNavigationOptions) -> usize {
    if cursor >= text.len() {
        return text.len();
    }
    let after = &text[cursor..];
    let segments = match options.segment {
        Some(f) => f(after),
        None => default_segments(after),
    };
    let is_atomic = |s: &str| options.is_atomic_segment.is_some_and(|f| f(s));
    let mut it = segments.iter().peekable();
    let mut new_cursor = cursor;

    while let Some(next) = it.peek() {
        if is_atomic(&next.segment) || !is_whitespace_char(&next.segment) {
            break;
        }
        new_cursor += next.segment.len();
        it.next();
    }
    let Some(next) = it.peek() else {
        return new_cursor;
    };
    if is_atomic(&next.segment) {
        new_cursor += next.segment.len();
    } else if next.is_word_like {
        new_cursor += next.segment.find(is_punctuation_ch).unwrap_or(next.segment.len());
    } else {
        while let Some(next) = it.peek() {
            if is_atomic(&next.segment) || next.is_word_like || is_whitespace_char(&next.segment) {
                break;
            }
            new_cursor += next.segment.len();
            it.next();
        }
    }
    new_cursor
}
