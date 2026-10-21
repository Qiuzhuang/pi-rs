//! Small regression suites: wheel-scroll.test.ts, tab-width.test.ts,
//! regression-overlay-cjk-boundary.test.ts, overlay-short-content.test.ts,
//! tui-overlay-style-leak.test.ts and bug-regression-isimageline-startswith-bug.test.ts.

mod common;

use common::{Harness, TestComponent};
use pi_tui::terminal_image::is_image_line;
use pi_tui::tui::{OverlayOptions, SizeValue, composite_tui_line};
use pi_tui::utils::{extract_segments, normalize_terminal_output, slice_by_column, slice_with_width, visible_width};
use pi_tui::wheel_scroll::{WheelScrollAccelerator, WheelScrollLines};

// ---------------------------------------------------------------------------
// WheelScrollAccelerator
// ---------------------------------------------------------------------------

fn scroll(acc: &mut WheelScrollAccelerator, times: &[f64], direction: i32) -> Vec<i64> {
    times.iter().map(|&t| acc.next(direction, t)).collect()
}

#[test]
fn wheel_uses_fixed_line_counts_regardless_of_timing() {
    let mut acc = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Lines(3.0), true);
    assert_eq!(scroll(&mut acc, &[0.0, 10.0, 20.0, 1000.0], 1), vec![3, 3, 3, 3]);
    acc.set_lines(WheelScrollLines::Lines(0.5));
    assert_eq!(acc.next(1, 2000.0), 1);
}

#[test]
fn wheel_auto_keeps_one_line_when_terminal_accelerates() {
    let mut acc = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Auto, false);
    assert_eq!(scroll(&mut acc, &[0.0, 10.0, 20.0, 30.0], 1), vec![1, 1, 1, 1]);
}

#[test]
fn wheel_auto_scales_with_velocity() {
    let mut acc = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Auto, true);
    assert_eq!(scroll(&mut acc, &[0.0, 150.0, 300.0, 450.0], 1), vec![1, 1, 1, 1]);
    assert_eq!(scroll(&mut acc, &[1000.0, 1050.0, 1100.0, 1150.0], 1), vec![1, 2, 2, 2]);
    assert_eq!(scroll(&mut acc, &[2000.0, 2020.0, 2040.0, 2060.0], 1), vec![1, 5, 5, 5]);
    assert_eq!(scroll(&mut acc, &[3000.0, 3010.0, 3020.0, 3030.0], 1), vec![1, 6, 6, 6]);
}

#[test]
fn wheel_does_not_accelerate_single_notch_bursts() {
    let mut acc = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Auto, true);
    assert_eq!(scroll(&mut acc, &[0.0, 3.0, 6.0, 9.0], 1), vec![1, 1, 1, 1]);
}

#[test]
fn wheel_resets_on_direction_change_and_pause() {
    let mut acc = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Auto, true);
    assert_eq!(scroll(&mut acc, &[0.0, 20.0, 40.0], 1), vec![1, 5, 5]);
    assert_eq!(acc.next(-1, 60.0), 1);
    assert_eq!(scroll(&mut acc, &[500.0, 520.0], 1), vec![1, 5]);
}

#[test]
fn wheel_carries_fractional_lines() {
    let mut acc = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Auto, true);
    assert_eq!(scroll(&mut acc, &[0.0, 40.0, 80.0, 120.0, 160.0], 1), vec![1, 2, 3, 2, 3]);
}

// ---------------------------------------------------------------------------
// Tab width accounting
// ---------------------------------------------------------------------------

#[test]
fn tab_slice_widths_match_visible_width() {
    let text = "out 192M\t.pi/skill-tests/results-ha";
    let (slice, width) = slice_with_width(text, 0, 10, true);
    assert_eq!(slice, "out 192M");
    assert_eq!(width, 8);
    assert_eq!(visible_width(&slice), width);
}

#[test]
fn tab_overlay_segment_widths_match_visible_width() {
    let text = "out 192M\t.pi/skill-tests/results-ha";
    let segments = extract_segments(text, 10, 13, 10, true);
    assert_eq!(segments.before, "out 192M");
    assert_eq!(segments.before_width, 8);
    assert_eq!(visible_width(&segments.before), segments.before_width);

    let fits = extract_segments(text, 11, 13, 10, true);
    assert_eq!(fits.before, "out 192M\t");
    assert_eq!(fits.before_width, 11);
    assert_eq!(visible_width(&fits.before), fits.before_width);
}

#[test]
fn tabs_inside_control_sequences_stay_byte_identical() {
    for seq in ["\x1b]8;;https://example.test/a\tb\x07", "\x1b]0;window\ttitle\x1b\\", "\x1b_payload\tdata\x1b\\"] {
        assert_eq!(normalize_terminal_output(&format!("{seq}label\ttext")), format!("{seq}label   text"));
    }
}

#[test]
fn tab_containing_overlays_stay_on_one_row() {
    let h = Harness::new(16, 3);
    h.add(TestComponent::new(&[
        "base 0          ",
        "base 1          ",
        "base 2          ",
    ]));
    h.tui.show_overlay(
        TestComponent::new(&["\tX"]),
        Some(OverlayOptions {
            width: Some(SizeValue::Abs(4)),
            row: Some(SizeValue::Abs(1)),
            col: Some(SizeValue::Abs(4)),
            ..Default::default()
        }),
    );
    h.tui.start();
    h.wait_for_render();
    assert_eq!(h.terminal.get_viewport(), vec!["base 0          ", "base   X        ", "base 2          "]);
    assert!(!h.terminal.get_writes().contains('\t'));
    h.tui.stop(Default::default());
}

// ---------------------------------------------------------------------------
// Overlay CJK boundary
// ---------------------------------------------------------------------------

#[test]
fn cjk_excludes_wide_grapheme_when_overlay_starts_inside_it() {
    let s = extract_segments("abcd让EFGH", 5, 9, 11, true);
    assert_eq!(s.before, "abcd");
    assert_eq!(s.before_width, 4);
    assert_eq!(visible_width(&s.before), s.before_width);
    assert_eq!(s.after, "H");
    assert_eq!(s.after_width, 1);
}

#[test]
fn cjk_ascii_before_segment_at_same_boundary() {
    let s = extract_segments("abcdG EFGH", 5, 9, 11, true);
    assert_eq!(s.before, "abcdG");
    assert_eq!(s.before_width, 5);
    assert_eq!(visible_width(&s.before), s.before_width);
}

#[test]
fn cjk_composites_overlay_starting_inside_wide_grapheme() {
    let out = composite_tui_line("abcd让EFGH", "│XX│", 5, 4, 20);
    let prefix = slice_by_column(&out, 0, 5, true);
    let overlay = slice_by_column(&out, 5, 4, true);
    assert!(!out.contains('让'));
    assert_eq!(visible_width(&out), 20);
    assert_eq!(visible_width(&prefix), 5);
    assert_eq!(visible_width(&overlay), 4);
    assert!(overlay.contains("│XX│"));
}

#[test]
fn cjk_composites_overlay_at_wide_grapheme_boundary() {
    let out = composite_tui_line("abcd让EFGH", "│XX│", 4, 4, 20);
    let overlay = slice_by_column(&out, 4, 4, true);
    assert!(!out.contains('让'));
    assert_eq!(visible_width(&out), 20);
    assert_eq!(visible_width(&overlay), 4);
    assert!(overlay.contains("│XX│"));
}

// ---------------------------------------------------------------------------
// Overlay with short content
// ---------------------------------------------------------------------------

#[test]
fn renders_overlay_when_content_is_shorter_than_terminal() {
    let h = Harness::new(80, 24);
    h.add(TestComponent::new(&["Line 1", "Line 2", "Line 3"]));
    h.tui.show_overlay(TestComponent::new(&["OVERLAY_TOP", "OVERLAY_MID", "OVERLAY_BOT"]), None);
    h.tui.start();
    h.wait_for_render();
    let viewport = h.terminal.get_viewport();
    assert!(viewport.iter().any(|l| l.contains("OVERLAY")), "{viewport:?}");
    h.tui.stop(Default::default());
}

// ---------------------------------------------------------------------------
// Overlay style leak. The virtual terminal keeps no SGR state, so instead of reading the
// italic attribute of row 1 this checks that italics are closed before "INPUT" is written.
// ---------------------------------------------------------------------------

fn italic_active_before(writes: &str, needle: &str) -> bool {
    assert!(writes.contains("\x1b[3m"), "italic base line written");
    let end = writes.rfind(needle).expect("needle written");
    let before = &writes[..end];
    let Some(on) = before.rfind("\x1b[3m") else { return false };
    let after_on = &before[on..];
    !(after_on.contains("\x1b[0m") || after_on.contains("\x1b[23m") || after_on.contains("\x1b[m"))
}

#[test]
fn no_style_leak_from_trailing_reset_beyond_last_column() {
    let width = 20;
    let base = format!("\x1b[3m{}\x1b[23m", "X".repeat(width));
    let h = Harness::new(width, 6);
    h.add(TestComponent::new(&[&base, "INPUT"]));
    h.tui.start();
    h.tui.request_render(true);
    h.wait_for_render();
    assert!(!italic_active_before(&h.terminal.get_writes(), "INPUT"));
    h.tui.stop(Default::default());
}

#[test]
fn no_style_leak_when_overlay_slicing_drops_trailing_resets() {
    let width = 20;
    let base = format!("\x1b[3m{}\x1b[23m", "X".repeat(width));
    let h = Harness::new(width, 6);
    h.add(TestComponent::new(&[&base, "INPUT"]));
    h.tui.show_overlay(
        TestComponent::new(&["OVR"]),
        Some(OverlayOptions {
            row: Some(SizeValue::Abs(0)),
            col: Some(SizeValue::Abs(5)),
            width: Some(SizeValue::Abs(3)),
            ..Default::default()
        }),
    );
    h.tui.start();
    h.tui.request_render(true);
    h.wait_for_render();
    let writes = h.terminal.get_writes();
    assert!(writes.contains("OVR"));
    assert!(!italic_active_before(&writes, "INPUT"));
    h.tui.stop(Default::default());
}

// ---------------------------------------------------------------------------
// isImageLine detects image sequences anywhere in the line
// ---------------------------------------------------------------------------

#[test]
fn is_image_line_detects_sequences_anywhere() {
    assert!(is_image_line("Read image file [image/jpeg]\x1b]1337;File=size=800,600;inline=1:base64data...\x07"));
    let long = format!("Text before \x1b_Ga=T,f=100{} text after", "A".repeat(300_000));
    for line in [
        "At start: \x1b_Ga=T,f=100,data...\x1b\\",
        "Prefix \x1b_Ga=T,data...\x1b\\",
        "Suffix text \x1b_Ga=T,data...\x1b\\ suffix",
        "Middle \x1b_Ga=T,data...\x1b\\ more text",
        long.as_str(),
    ] {
        assert!(is_image_line(line), "{:?}", &line[..line.len().min(40)]);
    }
    for line in [
        "\x1b]1337;File=inline=1:data==\x07",
        "Text \x1b]1337;File=inline=1:data==\x07",
        "Text\x1b]1337;File=inline=1:data==\x07 more",
        "\x1b_Ga=T,f=100,t=f,d=base64data...\x1b\\\x1b_Gm=i=1;\x1b\\",
        "\x1b[31mError\x1b[0m: \x1b]1337;File=inline=1:base64==\x07",
        "\x1b[33mWarning\x1b[0m: \x1b_Ga=T,data...\x1b\\",
        "\x1b[1mBold\x1b[0m \x1b]1337;File=:base64==\x07\x1b[0m",
    ] {
        assert!(is_image_line(line));
    }
    let long_text = "A".repeat(100_000);
    for line in [
        long_text.as_str(),
        "/path/to/1337/image.jpg",
        "/usr/local/bin/File_converter",
        "~/Documents/1337File_backup.png",
        "./_G_test_file.txt",
        "Just plain text",
"\x1b[31mRed text\x1b[0m", "Text with \x1b]8;;https://x\x07link\x1b]8;;\x07", ""] {
        assert!(!is_image_line(line), "{line:?}");
    }
}
