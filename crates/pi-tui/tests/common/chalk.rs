//! Minimal emulation of chalk (level 3) styling used by the reference test themes.
#![allow(dead_code)]

use std::sync::Arc;

use pi_tui::StyleFn;
use pi_tui::components::editor::EditorTheme;
use pi_tui::components::select_list::SelectListTheme;

/// Apply a chain of `(open, close)` styles (outermost first) like chalk does.
pub fn apply(styles: &[(&str, &str)], text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut s = text.to_string();
    if s.contains('\x1b') {
        for (open, close) in styles.iter().rev() {
            s = s.replace(close, &format!("{close}{open}"));
        }
    }
    let open_all: String = styles.iter().map(|(o, _)| *o).collect();
    let close_all: String = styles.iter().rev().map(|(_, c)| *c).collect();
    if s.contains('\n') {
        let mut out = String::new();
        let mut rest = s.as_str();
        while let Some(i) = rest.find('\n') {
            let cr = i > 0 && rest.as_bytes()[i - 1] == b'\r';
            out.push_str(&rest[..if cr { i - 1 } else { i }]);
            out.push_str(&close_all);
            out.push_str(if cr { "\r\n" } else { "\n" });
            out.push_str(&open_all);
            rest = &rest[i + 1..];
        }
        out.push_str(rest);
        s = out;
    }
    format!("{open_all}{s}{close_all}")
}

pub const BOLD: (&str, &str) = ("\x1b[1m", "\x1b[22m");
pub const DIM: (&str, &str) = ("\x1b[2m", "\x1b[22m");
pub const ITALIC: (&str, &str) = ("\x1b[3m", "\x1b[23m");
pub const UNDERLINE: (&str, &str) = ("\x1b[4m", "\x1b[24m");
pub const STRIKE: (&str, &str) = ("\x1b[9m", "\x1b[29m");
pub const GREEN: (&str, &str) = ("\x1b[32m", "\x1b[39m");
pub const YELLOW: (&str, &str) = ("\x1b[33m", "\x1b[39m");
pub const BLUE: (&str, &str) = ("\x1b[34m", "\x1b[39m");
pub const CYAN: (&str, &str) = ("\x1b[36m", "\x1b[39m");

pub fn style(styles: &'static [(&'static str, &'static str)]) -> StyleFn {
    Arc::new(move |t: &str| apply(styles, t))
}

pub fn select_list_theme() -> SelectListTheme {
    SelectListTheme {
        selected_prefix: style(&[BLUE]),
        selected_text: style(&[BOLD]),
        description: style(&[DIM]),
        scroll_info: style(&[DIM]),
        no_match: style(&[DIM]),
    }
}

pub fn editor_theme() -> EditorTheme {
    EditorTheme { border_color: style(&[DIM]), select_list: select_list_theme() }
}
