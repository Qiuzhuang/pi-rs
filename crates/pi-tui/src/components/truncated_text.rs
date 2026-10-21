//! Single-line text truncated to the viewport width. Port of `components/truncated-text.ts`.

use std::sync::Mutex;

use crate::tui::Component;
use crate::utils::{truncate_to_width, visible_width};

/// Text component that truncates to fit the viewport width (first line only).
pub struct TruncatedText {
    text: Mutex<String>,
    padding_x: usize,
    padding_y: usize,
}

impl TruncatedText {
    pub fn new(text: impl Into<String>, padding_x: usize, padding_y: usize) -> Self {
        Self { text: Mutex::new(text.into()), padding_x, padding_y }
    }

    pub fn set_text(&self, text: impl Into<String>) {
        *self.text.lock().unwrap_or_else(|e| e.into_inner()) = text.into();
    }
}

impl Component for TruncatedText {
    fn render(&self, width: usize) -> Vec<String> {
        let text = self.text.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut result = Vec::new();
        let empty_line = " ".repeat(width);
        for _ in 0..self.padding_y {
            result.push(empty_line.clone());
        }
        let available = width.saturating_sub(self.padding_x * 2).max(1);
        let single = match text.find('\n') {
            Some(i) => &text[..i],
            None => &text[..],
        };
        let display = truncate_to_width(single, available, "...", false);
        let pad = " ".repeat(self.padding_x);
        let with_padding = format!("{pad}{display}{pad}");
        let needed = width.saturating_sub(visible_width(&with_padding));
        result.push(format!("{with_padding}{}", " ".repeat(needed)));
        for _ in 0..self.padding_y {
            result.push(empty_line.clone());
        }
        result
    }
}
