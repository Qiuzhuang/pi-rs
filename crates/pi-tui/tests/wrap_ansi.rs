//! Port of wrap-ansi.test.ts.

use pi_tui::utils::*;

#[test]
fn underline_not_applied_before_styled_text() {
    let url = "https://example.com/very/long/path/that/will/wrap";
    let wrapped = wrap_text_with_ansi(&format!("read this thread \x1b[4m{url}\x1b[24m"), 40);
    assert_eq!(wrapped[0], "read this thread");
    assert!(wrapped[1].starts_with("\x1b[4m"));
    assert!(wrapped[1].contains("https://"));
}

#[test]
fn no_whitespace_before_underline_reset() {
    let wrapped = wrap_text_with_ansi("\x1b[4munderlined text here \x1b[24mmore", 18);
    assert!(!wrapped[0].contains(" \x1b[24m"));
}

#[test]
fn underline_does_not_bleed_into_padding() {
    let url = "https://example.com/very/long/path/that/will/definitely/wrap";
    let wrapped = wrap_text_with_ansi(&format!("prefix \x1b[4m{url}\x1b[24m suffix"), 30);
    for line in &wrapped[1..wrapped.len() - 1] {
        if line.contains("\x1b[4m") {
            assert!(line.ends_with("\x1b[24m"));
            assert!(!line.ends_with("\x1b[0m"));
        }
    }
}

#[test]
fn preserves_background_across_wrapped_lines() {
    let wrapped = wrap_text_with_ansi("\x1b[44mhello world this is blue background text\x1b[0m", 15);
    for line in &wrapped {
        assert!(line.contains("\x1b[44m"));
    }
    for line in &wrapped[..wrapped.len() - 1] {
        assert!(!line.ends_with("\x1b[0m"));
    }
}

#[test]
fn resets_underline_but_keeps_background() {
    let text = "\x1b[41mprefix \x1b[4mUNDERLINED_CONTENT_THAT_WRAPS\x1b[24m suffix\x1b[0m";
    let wrapped = wrap_text_with_ansi(text, 20);
    for line in &wrapped {
        assert!(line.contains("[41m") || line.contains(";41m") || line.contains("[41;"));
    }
    for line in &wrapped[..wrapped.len() - 1] {
        if (line.contains("[4m") || line.contains("[4;") || line.contains(";4m")) && !line.contains("\x1b[24m") {
            assert!(line.ends_with("\x1b[24m"));
        }
    }
}

#[test]
fn handles_lf_crlf_cr() {
    assert_eq!(wrap_text_with_ansi("first\nsecond\r\nthird\rfourth", 80), vec!["first", "second", "third", "fourth"]);
}

#[test]
fn preserves_ansi_state_across_crlf_and_cr() {
    assert_eq!(
        wrap_text_with_ansi("\x1b[31mfirst\r\nsecond\rthird\x1b[0m", 80),
        vec!["\x1b[31mfirst", "\x1b[31msecond", "\x1b[31mthird\x1b[0m"]
    );
}

#[test]
fn wraps_plain_text() {
    let wrapped = wrap_text_with_ansi("hello world this is a test", 10);
    assert!(wrapped.len() > 1);
    for line in wrapped {
        assert!(visible_width(&line) <= 10);
    }
}

#[test]
fn breaks_cjk_runs_at_grapheme_boundaries() {
    let wrapped = wrap_text_with_ansi("This is an example 中文汉字测试段落内容中文汉字测试段落内容.", 40);
    assert_eq!(wrapped, vec!["This is an example 中文汉字测试段落内容", "中文汉字测试段落内容."]);
}

#[test]
fn preserves_color_when_wrapping_cjk() {
    let wrapped = wrap_text_with_ansi("\x1b[31mThis is an example 中文汉字测试段落内容中文汉字测试段落内容.\x1b[0m", 40);
    assert_eq!(wrapped, vec!["\x1b[31mThis is an example 中文汉字测试段落内容", "\x1b[31m中文汉字测试段落内容.\x1b[0m"]);
}

#[test]
fn osc_markers_have_no_width() {
    assert_eq!(visible_width("\x1b]133;A\x07hello\x1b]133;B\x07"), 5);
    assert_eq!(visible_width("\x1b]133;A\x1b\\hello\x1b]133;B\x1b\\"), 5);
}

#[test]
fn truncates_trailing_whitespace_exceeding_width() {
    let wrapped = wrap_text_with_ansi("  ", 1);
    assert!(visible_width(&wrapped[0]) <= 1);
}

#[test]
fn preserves_color_codes_across_wraps() {
    let wrapped = wrap_text_with_ansi("\x1b[31mhello world this is red\x1b[0m", 10);
    for line in &wrapped[1..] {
        assert!(line.starts_with("\x1b[31m"));
    }
    for line in &wrapped[..wrapped.len() - 1] {
        assert!(!line.ends_with("\x1b[0m"));
    }
}

#[test]
fn osc8_reopened_and_closed_on_each_line() {
    let url = "https://example.com";
    let open = format!("\x1b]8;;{url}\x1b\\");
    let lines = wrap_text_with_ansi(&format!("{open}0123456789\x1b]8;;\x1b\\"), 6);
    for line in &lines {
        let stripped = strip_terminal_sequences(line);
        if !stripped.trim().is_empty() {
            assert!(line.contains(&open), "{line:?}");
        }
    }
    for line in &lines[..lines.len() - 1] {
        if line.contains(&open) {
            assert!(line.ends_with("\x1b]8;;\x1b\\"));
        }
    }
}

#[test]
fn preserves_bel_terminators_for_oauth_links() {
    let url = format!("https://example.com/oauth/{}", "a".repeat(32));
    let lines = wrap_text_with_ansi(&format!("\x1b]8;;{url}\x07{url}\x1b]8;;\x07"), 20);
    assert!(lines.len() > 1);
    for line in &lines {
        assert!(line.contains(&format!("\x1b]8;;{url}\x07")));
        assert!(!line.contains(&format!("\x1b]8;;{url}\x1b\\")));
    }
    for line in &lines[..lines.len() - 1] {
        assert!(line.ends_with("\x1b]8;;\x07"));
    }
}

#[test]
fn no_osc8_outside_hyperlink() {
    let lines = wrap_text_with_ansi("before \x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ after", 80);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].matches("\x1b]8;;https:").count(), 1);
    assert_eq!(lines[0].matches("\x1b]8;;\x1b\\").count(), 1);
}
