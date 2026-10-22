//! Port of `packages/tui/test/overlay-options.test.ts`: width overflow protection, width
//! percentages, anchors, margins, offsets, percentage and absolute positioning, maxHeight,
//! stacked overlays and hiding overlays after stop.

mod common;

use std::sync::{Arc, Mutex};

use common::Harness;
use pi_tui::tui::{
    Component, ComponentRef, OverlayAnchor, OverlayMargin, OverlayOptions, SizeValue, TuiStopOptions,
};

/// Renders fixed lines and records the width it was asked to render at.
struct StaticOverlay {
    lines: Vec<String>,
    requested_width: Mutex<Option<usize>>,
}

impl StaticOverlay {
    fn new(lines: &[&str]) -> Arc<Self> {
        Arc::new(Self { lines: lines.iter().map(|s| s.to_string()).collect(), requested_width: Mutex::new(None) })
    }
    fn from_lines(lines: Vec<String>) -> Arc<Self> {
        Arc::new(Self { lines, requested_width: Mutex::new(None) })
    }
    fn requested_width(&self) -> Option<usize> {
        *self.requested_width.lock().unwrap()
    }
}

impl Component for StaticOverlay {
    fn render(&self, width: usize) -> Vec<String> {
        *self.requested_width.lock().unwrap() = Some(width);
        self.lines.clone()
    }
}

struct EmptyContent;

impl Component for EmptyContent {
    fn render(&self, _width: usize) -> Vec<String> {
        Vec::new()
    }
}

/// Base content rendering lines built from the width.
struct WidthContent(fn(usize) -> Vec<String>);

impl Component for WidthContent {
    fn render(&self, width: usize) -> Vec<String> {
        (self.0)(width)
    }
}

fn abs(v: i64) -> Option<SizeValue> {
    Some(SizeValue::Abs(v))
}

fn pct(v: &str) -> Option<SizeValue> {
    Some(SizeValue::Percent(v.to_string()))
}

/// Add `base`, show the overlays (before start, as the TS tests do), start, render and flush.
fn render_with(cols: usize, rows: usize, base: ComponentRef, overlays: Vec<(ComponentRef, OverlayOptions)>) -> Harness {
    let h = Harness::new(cols, rows);
    h.tui.add_child(base);
    for (overlay, options) in overlays {
        h.tui.show_overlay(overlay, Some(options));
    }
    h.tui.start();
    flush(&h);
    h
}

fn render_one(cols: usize, rows: usize, overlay: ComponentRef, options: OverlayOptions) -> Vec<String> {
    let h = render_with(cols, rows, Arc::new(EmptyContent), vec![(overlay, options)]);
    let viewport = h.terminal.get_viewport();
    h.tui.stop(TuiStopOptions::default());
    viewport
}

fn flush(h: &Harness) {
    h.tui.request_render(true);
    h.wait_for_render();
}

fn col_of(line: &str, needle: &str) -> Option<usize> {
    line.find(needle).map(|byte| line[..byte].chars().count())
}

// ---- width overflow protection ------------------------------------------------------------

#[test]
fn should_truncate_overlay_lines_that_exceed_declared_width() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::from_lines(vec!["X".repeat(100)]),
        OverlayOptions { width: abs(20), ..Default::default() },
    );
    assert_eq!(viewport.len(), 24);
}

#[test]
fn should_handle_overlay_with_complex_ansi_sequences_without_crashing() {
    let complex = format!(
        "\x1b[48;2;40;50;40m \x1b[38;2;128;128;128mSome styled content\x1b[39m\x1b[49m\x1b]8;;http://example.com\x07link\x1b]8;;\x07{}",
        " more content ".repeat(10)
    );
    let viewport = render_one(
        80,
        24,
        StaticOverlay::from_lines(vec![complex.clone(), complex.clone(), complex]),
        OverlayOptions { width: abs(60), ..Default::default() },
    );
    assert!(!viewport.is_empty());
}

#[test]
fn should_handle_overlay_composited_on_styled_base_content() {
    let styled = WidthContent(|width| {
        let line = format!("\x1b[1m\x1b[38;2;255;0;0m{}\x1b[0m", "X".repeat(width));
        vec![line.clone(), line.clone(), line]
    });
    let h = render_with(
        80,
        24,
        Arc::new(styled),
        vec![(
            StaticOverlay::new(&["OVERLAY"]),
            OverlayOptions { width: abs(20), anchor: Some(OverlayAnchor::Center), ..Default::default() },
        )],
    );
    assert!(h.terminal.get_viewport().iter().any(|l| l.contains("OVERLAY")), "Overlay should be visible");
    h.tui.stop(TuiStopOptions::default());
}

#[test]
fn should_handle_wide_characters_at_overlay_boundary() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["中文日本語한글テスト漢字"]),
        OverlayOptions { width: abs(15), ..Default::default() },
    );
    assert!(!viewport.is_empty());
}

#[test]
fn should_handle_overlay_positioned_at_terminal_edge() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::from_lines(vec!["X".repeat(50)]),
        OverlayOptions { col: abs(60), width: abs(20), ..Default::default() },
    );
    assert!(!viewport.is_empty());
}

#[test]
fn should_handle_overlay_on_base_content_with_osc_sequences() {
    let hyperlinks = WidthContent(|width| {
        let link = "\x1b]8;;file:///path/to/file.ts\x07file.ts\x1b]8;;\x07";
        let line = format!("See {link} for details {}", "X".repeat(width - 30));
        vec![line.clone(), line.clone(), line]
    });
    let h = render_with(
        80,
        24,
        Arc::new(hyperlinks),
        vec![(
            StaticOverlay::new(&["OVERLAY-TEXT"]),
            OverlayOptions { anchor: Some(OverlayAnchor::Center), width: abs(20), ..Default::default() },
        )],
    );
    assert!(!h.terminal.get_viewport().is_empty());
    h.tui.stop(TuiStopOptions::default());
}

// ---- width percentage ---------------------------------------------------------------------

#[test]
fn should_render_overlay_at_percentage_of_terminal_width() {
    let overlay = StaticOverlay::new(&["test"]);
    render_one(100, 24, overlay.clone(), OverlayOptions { width: pct("50%"), ..Default::default() });
    assert_eq!(overlay.requested_width(), Some(50));
}

#[test]
fn should_respect_min_width_when_width_percent_results_in_smaller_width() {
    let overlay = StaticOverlay::new(&["test"]);
    render_one(100, 24, overlay.clone(), OverlayOptions { width: pct("10%"), min_width: Some(30), ..Default::default() });
    assert_eq!(overlay.requested_width(), Some(30));
}

// ---- anchor positioning -------------------------------------------------------------------

#[test]
fn should_position_overlay_at_top_left() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["TOP-LEFT"]),
        OverlayOptions { anchor: Some(OverlayAnchor::TopLeft), width: abs(10), ..Default::default() },
    );
    assert!(viewport[0].starts_with("TOP-LEFT"), "Expected TOP-LEFT at start, got: {}", viewport[0]);
}

#[test]
fn should_position_overlay_at_bottom_right() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["BTM-RIGHT"]),
        OverlayOptions { anchor: Some(OverlayAnchor::BottomRight), width: abs(10), ..Default::default() },
    );
    let last = &viewport[23];
    assert!(last.contains("BTM-RIGHT"), "Expected BTM-RIGHT on last row, got: {last}");
    assert!(last.trim_end().ends_with("BTM-RIGHT"), "Expected BTM-RIGHT at end, got: {last}");
}

#[test]
fn should_position_overlay_at_top_center() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["CENTERED"]),
        OverlayOptions { anchor: Some(OverlayAnchor::TopCenter), width: abs(10), ..Default::default() },
    );
    let first = &viewport[0];
    assert!(first.contains("CENTERED"), "Expected CENTERED on first row, got: {first}");
    let col = col_of(first, "CENTERED").unwrap();
    assert!((30..=40).contains(&col), "Expected centered, got col {col}");
}

// ---- margin -------------------------------------------------------------------------------

#[test]
fn should_clamp_negative_margins_to_zero() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["NEG-MARGIN"]),
        OverlayOptions {
            anchor: Some(OverlayAnchor::TopLeft),
            width: abs(12),
            margin: Some(OverlayMargin { top: Some(-5), left: Some(-10), right: Some(0), bottom: Some(0) }),
            ..Default::default()
        },
    );
    assert!(viewport[0].starts_with("NEG-MARGIN"), "Expected NEG-MARGIN at start of row 0, got: {}", viewport[0]);
}

#[test]
fn should_respect_margin_as_number() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["MARGIN"]),
        OverlayOptions {
            anchor: Some(OverlayAnchor::TopLeft),
            width: abs(10),
            margin: Some(OverlayMargin::all(5)),
            ..Default::default()
        },
    );
    assert!(!viewport[0].contains("MARGIN"), "Should not be on row 0");
    assert!(!viewport[4].contains("MARGIN"), "Should not be on row 4");
    assert!(viewport[5].contains("MARGIN"), "Expected MARGIN on row 5, got: {}", viewport[5]);
    assert_eq!(col_of(&viewport[5], "MARGIN"), Some(5));
}

#[test]
fn should_respect_margin_object() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["MARGIN"]),
        OverlayOptions {
            anchor: Some(OverlayAnchor::TopLeft),
            width: abs(10),
            margin: Some(OverlayMargin { top: Some(2), left: Some(3), right: Some(0), bottom: Some(0) }),
            ..Default::default()
        },
    );
    assert!(viewport[2].contains("MARGIN"), "Expected MARGIN on row 2, got: {}", viewport[2]);
    assert_eq!(col_of(&viewport[2], "MARGIN"), Some(3));
}

// ---- offset -------------------------------------------------------------------------------

#[test]
fn should_apply_offset_x_and_offset_y_from_anchor_position() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["OFFSET"]),
        OverlayOptions {
            anchor: Some(OverlayAnchor::TopLeft),
            width: abs(10),
            offset_x: Some(10),
            offset_y: Some(5),
            ..Default::default()
        },
    );
    assert!(viewport[5].contains("OFFSET"), "Expected OFFSET on row 5, got: {}", viewport[5]);
    assert_eq!(col_of(&viewport[5], "OFFSET"), Some(10));
}

// ---- percentage positioning ---------------------------------------------------------------

#[test]
fn should_position_with_row_percent_and_col_percent() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["PCT"]),
        OverlayOptions { width: abs(10), row: pct("50%"), col: pct("50%"), ..Default::default() },
    );
    let row = viewport.iter().position(|l| l.contains("PCT")).expect("PCT rendered");
    assert!((10..=13).contains(&row), "Expected centered row, got {row}");
}

#[test]
fn row_percent_0_should_position_at_top() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["TOP"]),
        OverlayOptions { width: abs(10), row: pct("0%"), ..Default::default() },
    );
    assert!(viewport[0].contains("TOP"), "Expected TOP on row 0, got: {}", viewport[0]);
}

#[test]
fn row_percent_100_should_position_at_bottom() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["BOTTOM"]),
        OverlayOptions { width: abs(10), row: pct("100%"), ..Default::default() },
    );
    assert!(viewport[23].contains("BOTTOM"), "Expected BOTTOM on last row, got: {}", viewport[23]);
}

// ---- maxHeight ----------------------------------------------------------------------------

#[test]
fn should_truncate_overlay_to_max_height() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["Line 1", "Line 2", "Line 3", "Line 4", "Line 5"]),
        OverlayOptions { max_height: abs(3), ..Default::default() },
    );
    let content = viewport.join("\n");
    for line in ["Line 1", "Line 2", "Line 3"] {
        assert!(content.contains(line), "Should include {line}");
    }
    for line in ["Line 4", "Line 5"] {
        assert!(!content.contains(line), "Should NOT include {line}");
    }
}

#[test]
fn should_truncate_overlay_to_max_height_percent() {
    let viewport = render_one(
        80,
        10,
        StaticOverlay::new(&["L1", "L2", "L3", "L4", "L5", "L6", "L7", "L8", "L9", "L10"]),
        OverlayOptions { max_height: pct("50%"), ..Default::default() },
    );
    let content = viewport.join("\n");
    assert!(content.contains("L1"), "Should include L1");
    assert!(content.contains("L5"), "Should include L5");
    assert!(!content.contains("L6"), "Should NOT include L6");
}

// ---- absolute positioning -----------------------------------------------------------------

#[test]
fn row_and_col_should_override_anchor() {
    let viewport = render_one(
        80,
        24,
        StaticOverlay::new(&["ABSOLUTE"]),
        OverlayOptions {
            anchor: Some(OverlayAnchor::BottomRight),
            row: abs(3),
            col: abs(5),
            width: abs(10),
            ..Default::default()
        },
    );
    assert!(viewport[3].contains("ABSOLUTE"), "Expected ABSOLUTE on row 3, got: {}", viewport[3]);
    assert_eq!(col_of(&viewport[3], "ABSOLUTE"), Some(5));
}

// ---- stacked overlays ---------------------------------------------------------------------

fn top_left(width: i64) -> OverlayOptions {
    OverlayOptions { anchor: Some(OverlayAnchor::TopLeft), width: abs(width), ..Default::default() }
}

#[test]
fn should_render_multiple_overlays_with_later_ones_on_top() {
    let h = render_with(
        80,
        24,
        Arc::new(EmptyContent),
        vec![
            (StaticOverlay::new(&["FIRST-OVERLAY"]), top_left(20)),
            (StaticOverlay::new(&["SECOND"]), top_left(10)),
        ],
    );
    let viewport = h.terminal.get_viewport();
    assert!(viewport[0].contains("SECOND"), "Expected SECOND on row 0, got: {}", viewport[0]);
    h.tui.stop(TuiStopOptions::default());
}

#[test]
fn should_handle_overlays_at_different_positions_without_interference() {
    let h = render_with(
        80,
        24,
        Arc::new(EmptyContent),
        vec![
            (StaticOverlay::new(&["TOP-LEFT"]), top_left(15)),
            (
                StaticOverlay::new(&["BTM-RIGHT"]),
                OverlayOptions { anchor: Some(OverlayAnchor::BottomRight), width: abs(15), ..Default::default() },
            ),
        ],
    );
    let viewport = h.terminal.get_viewport();
    assert!(viewport[0].contains("TOP-LEFT"), "Expected TOP-LEFT on row 0, got: {}", viewport[0]);
    assert!(viewport[23].contains("BTM-RIGHT"), "Expected BTM-RIGHT on row 23, got: {}", viewport[23]);
    h.tui.stop(TuiStopOptions::default());
}

#[test]
fn should_properly_hide_overlays_in_stack_order() {
    let h = render_with(
        80,
        24,
        Arc::new(EmptyContent),
        vec![(StaticOverlay::new(&["FIRST"]), top_left(10)), (StaticOverlay::new(&["SECOND"]), top_left(10))],
    );
    assert!(h.terminal.get_viewport()[0].contains("SECOND"), "SECOND should be visible initially");
    h.tui.hide_overlay();
    flush(&h);
    assert!(h.terminal.get_viewport()[0].contains("FIRST"), "FIRST should be visible after hiding SECOND");
    h.tui.stop(TuiStopOptions::default());
}

// ---- hiding after stop (issue #10026) -----------------------------------------------------

#[test]
fn hide_overlay_after_stop_leaves_the_cursor_visible() {
    let h = Harness::new(80, 24);
    h.tui.start();
    h.tui.show_overlay(StaticOverlay::new(&["OVERLAY"]), Some(OverlayOptions { non_capturing: true, ..Default::default() }));
    h.tui.stop(TuiStopOptions::default());
    h.tui.hide_overlay();
    h.lp.run_pending();
    assert!(h.terminal.is_cursor_visible());
}

#[test]
fn overlay_handle_hide_after_stop_leaves_the_cursor_visible() {
    let h = Harness::new(80, 24);
    h.tui.start();
    let handle =
        h.tui.show_overlay(StaticOverlay::new(&["OVERLAY"]), Some(OverlayOptions { non_capturing: true, ..Default::default() }));
    h.tui.stop(TuiStopOptions::default());
    handle.hide();
    h.lp.run_pending();
    assert!(h.terminal.is_cursor_visible());
}
