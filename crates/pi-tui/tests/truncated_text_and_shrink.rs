//! Ports of truncated-text.test.ts and tui-shrink.test.ts.

mod common;

use common::chalk::{BLUE, apply};
use common::*;
use pi_tui::components::truncated_text::TruncatedText;
use pi_tui::tui::Component;
use pi_tui::utils::visible_width;

const RED: (&str, &str) = ("\x1b[31m", "\x1b[39m");

fn strip(line: &str) -> String {
    regex::Regex::new("\x1b\\[[0-9;]*m").unwrap().replace_all(line, "").into_owned()
}

#[test]
fn pads_output_lines_to_exactly_match_width() {
    let lines = TruncatedText::new("Hello world", 1, 0).render(50);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 50);
}

#[test]
fn pads_output_with_vertical_padding_lines_to_width() {
    let lines = TruncatedText::new("Hello", 0, 2).render(40);
    assert_eq!(lines.len(), 5);
    assert!(lines.iter().all(|line| visible_width(line) == 40));
}

#[test]
fn truncates_long_text_and_pads_to_width() {
    let lines = TruncatedText::new("This is a very long piece of text that will definitely exceed the available width", 1, 0).render(30);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 30);
    assert!(strip(&lines[0]).contains("..."));
}

#[test]
fn preserves_ansi_codes_in_output_and_pads_correctly() {
    let styled = format!("{} {}", apply(&[RED], "Hello"), apply(&[BLUE], "world"));
    let lines = TruncatedText::new(styled, 1, 0).render(40);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 40);
    assert!(lines[0].contains("\x1b["));
}

#[test]
fn truncates_styled_text_and_adds_reset_code_before_ellipsis() {
    let styled = apply(&[RED], "This is a very long red text that will be truncated");
    let lines = TruncatedText::new(styled, 1, 0).render(20);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 20);
    assert!(lines[0].contains("\x1b[0m..."), "{:?}", lines[0]);
}

#[test]
fn handles_text_that_fits_exactly() {
    let lines = TruncatedText::new("Hello world", 1, 0).render(30);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 30);
    assert!(!strip(&lines[0]).contains("..."));
}

#[test]
fn handles_empty_text() {
    let lines = TruncatedText::new("", 1, 0).render(30);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 30);
}

#[test]
fn stops_at_newline_and_only_shows_first_line() {
    let lines = TruncatedText::new("First line\nSecond line\nThird line", 1, 0).render(40);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 40);
    let stripped = strip(&lines[0]);
    assert!(stripped.trim().contains("First line"));
    assert!(!stripped.contains("Second line") && !stripped.contains("Third line"));
}

#[test]
fn truncates_first_line_even_with_newlines_in_text() {
    let lines = TruncatedText::new("This is a very long first line that needs truncation\nSecond line", 1, 0).render(25);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 25);
    let stripped = strip(&lines[0]);
    assert!(stripped.contains("..."));
    assert!(!stripped.contains("Second line"));
}

// ---- tui-shrink.test.ts ----

#[test]
fn clears_all_rendered_lines_when_content_shrinks_to_zero() {
    let h = Harness::new(40, 10);
    h.add(TestComponent::new(&["first", "second", "third"]));
    h.tui.start();
    h.wait_for_render();
    let viewport = h.terminal.get_viewport();
    for word in ["first", "second", "third"] {
        assert!(viewport.iter().any(|line| line.contains(word)), "{word}");
    }

    h.tui.clear();
    h.tui.request_render(false);
    h.wait_for_render();
    let viewport = h.terminal.get_viewport();
    for word in ["first", "second", "third"] {
        assert!(!viewport.iter().any(|line| line.contains(word)), "{word} line should be cleared: {viewport:?}");
    }
    h.tui.stop(Default::default());
}
