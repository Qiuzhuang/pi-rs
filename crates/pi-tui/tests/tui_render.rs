//! Port of tui-render.test.ts (tests that do not depend on environment variables).

mod common;

use std::sync::Arc;

use common::*;
use pi_tui::terminal::Terminal;
use pi_tui::tui::{CURSOR_MARKER, OverlayOptions, SizeValue, render_fake_cursor};
use pi_tui::utils::slice_by_column;

#[test]
fn renders_keyboard_input_without_waiting_for_throttled_frame() {
    let h = Harness::new(40, 10);
    let component = Arc::new(InputComponent::default());
    component.inner.set(&["initial"]);
    let c = h.add(component.clone());
    h.tui.set_focus(Some(c));
    h.tui.start();
    h.tui.render_now(false);
    let before = component.inner.renders();
    component.inner.set(&["pending"]);
    h.tui.request_render(false);
    h.terminal.send_input("first");
    h.terminal.send_input("second");
    h.terminal.send_input("typed");
    h.lp.run_pending();
    assert_eq!(component.inner.renders(), before + 1);
    assert_eq!(component.inner.get(), vec!["typed"]);
    h.tui.stop(Default::default());
}

#[derive(Default)]
struct BoundedWriteTerminal {
    writes: std::sync::Mutex<Vec<String>>,
}

impl Terminal for BoundedWriteTerminal {
    fn start(&self, _: pi_tui::terminal::InputHandler, _: pi_tui::terminal::ResizeHandler) {}
    fn stop(&self) {}
    fn write(&self, data: &str) {
        self.writes.lock().unwrap().push(data.to_string());
    }
    fn columns(&self) -> usize {
        80
    }
    fn rows(&self) -> usize {
        24
    }
    fn kitty_protocol_active(&self) -> bool {
        false
    }
    fn hide_cursor(&self) {}
    fn show_cursor(&self) {}
}

const MAX_RENDER_WRITE_CHARS: usize = 1024 * 1024;

#[test]
fn splits_large_full_render_without_changing_output() {
    let terminal = Arc::new(BoundedWriteTerminal::default());
    let lp = pi_tui::event_loop::EventLoop::new_manual();
    let tui = pi_tui::tui::Tui::main_screen_with(
        terminal.clone(),
        pi_tui::tui::TuiOptions { event_loop: Some(lp), ..Default::default() },
    );
    let component = TestComponent::new(&[]);
    let kitty_line = format!("\x1b_Ga=T,f=100;{}\x1b\\", "A".repeat(1_200_000));
    component.set_owned(vec![kitty_line.clone(), kitty_line.clone()]);
    tui.add_child(component);
    tui.render_now(false);
    let writes = terminal.writes.lock().unwrap().clone();
    assert!(writes.len() > 2);
    assert!(writes.iter().all(|w| w.len() <= MAX_RENDER_WRITE_CHARS));
    assert_eq!(writes.concat(), format!("\x1b[?2026h{kitty_line}\r\n{kitty_line}\x1b[?2026l"));
}

#[test]
fn splits_large_differential_updates_without_full_redraw() {
    let terminal = Arc::new(BoundedWriteTerminal::default());
    let lp = pi_tui::event_loop::EventLoop::new_manual();
    let tui = pi_tui::tui::Tui::main_screen_with(
        terminal.clone(),
        pi_tui::tui::TuiOptions { event_loop: Some(lp), ..Default::default() },
    );
    let component = TestComponent::new(&["before"]);
    tui.add_child(component.clone());
    tui.render_now(false);
    terminal.writes.lock().unwrap().clear();
    let kitty_line = format!("\x1b_Ga=T,f=100;{}\x1b\\", "A".repeat(1_200_000));
    component.set_owned(vec!["before".into(), kitty_line.clone(), kitty_line]);
    tui.render_now(false);
    let writes = terminal.writes.lock().unwrap().clone();
    assert!(writes.len() > 2);
    assert!(writes.iter().all(|w| w.len() <= MAX_RENDER_WRITE_CHARS));
    let output = writes.concat();
    assert!(output.starts_with("\x1b[?2026h"));
    assert!(output.ends_with("\x1b[?2026l"));
    assert!(!output.contains("\x1b[2J"));
}

#[test]
fn deletes_changed_image_ids_before_drawing_moved_placements() {
    use pi_tui::terminal_image::{KittyEncodeOptions, delete_kitty_image, encode_kitty};
    let h = Harness::new(40, 10);
    let component = TestComponent::new(&[]);
    h.add(component.clone());
    let opts = |rows| KittyEncodeOptions { columns: Some(2), rows: Some(rows), image_id: Some(42), move_cursor: Some(false) };
    let old_image = encode_kitty("AAAA", opts(2));
    component.set_owned(vec!["top".into(), old_image]);
    h.tui.start();
    h.wait_for_render();
    h.terminal.clear_writes();
    let new_image = encode_kitty("BBBB", opts(1));
    component.set_owned(vec![new_image.clone(), String::new()]);
    h.tui.request_render(false);
    h.wait_for_render();
    let writes = h.terminal.get_writes();
    let del = writes.find(&delete_kitty_image(42)).expect("delete");
    let draw = writes.find(&new_image).expect("draw");
    assert!(del < draw);
    h.tui.stop(Default::default());
}

#[test]
fn redraws_image_lines_when_earlier_reserved_row_changes() {
    use pi_tui::terminal_image::{KittyEncodeOptions, delete_kitty_image, encode_kitty};
    let h = Harness::new(40, 10);
    let component = TestComponent::new(&[]);
    h.add(component.clone());
    let image = encode_kitty("AAAA", KittyEncodeOptions { columns: Some(2), rows: Some(2), image_id: Some(88), move_cursor: Some(false) });
    component.set_owned(vec![String::new(), image.clone()]);
    h.tui.start();
    h.wait_for_render();
    h.terminal.clear_writes();
    component.set_owned(vec!["covered".into(), image.clone()]);
    h.tui.request_render(false);
    h.wait_for_render();
    let writes = h.terminal.get_writes();
    let del = writes.find(&delete_kitty_image(88)).expect("delete");
    let draw = writes.find(&image).expect("draw");
    assert!(del < draw);
    assert!(!writes.contains("\x1b[2J"));
    h.tui.stop(Default::default());
}

#[test]
fn deletes_previous_image_ids_during_full_redraws() {
    use pi_tui::terminal_image::{KittyEncodeOptions, delete_kitty_image, encode_kitty};
    let h = Harness::new(40, 10);
    let component = TestComponent::new(&[]);
    h.add(component.clone());
    component.set_owned(vec![encode_kitty(
        "AAAA",
        KittyEncodeOptions { columns: Some(2), rows: Some(2), image_id: Some(77), move_cursor: Some(false) },
    )]);
    h.tui.start();
    h.wait_for_render();
    h.terminal.clear_writes();
    component.set(&["plain text"]);
    h.tui.request_render(true);
    h.wait_for_render();
    let writes = h.terminal.get_writes();
    let del = writes.find(&delete_kitty_image(77)).expect("delete");
    let clear = writes.find("\x1b[2J").expect("clear");
    assert!(del < clear);
    h.tui.stop(Default::default());
}

#[test]
fn full_rerender_on_width_change() {
    let h = Harness::new(40, 10);
    let component = TestComponent::new(&["Line 0", "Line 1", "Line 2"]);
    h.add(component);
    h.tui.start();
    h.wait_for_render();
    let initial = h.tui.full_redraws();
    h.terminal.resize(60, 10);
    h.wait_for_render();
    assert!(h.tui.full_redraws() > initial);
    h.tui.stop(Default::default());
}

fn lines(n: usize, prefix: &str) -> Vec<String> {
    (0..n).map(|i| format!("{prefix} {i}")).collect()
}

#[test]
fn clears_empty_rows_when_content_shrinks() {
    let h = Harness::new(40, 10);
    h.tui.set_clear_on_shrink(true);
    let component = TestComponent::new(&[]);
    component.set_owned(lines(6, "Line"));
    h.add(component.clone());
    h.tui.start();
    h.wait_for_render();
    let initial = h.tui.full_redraws();
    component.set(&["Line 0", "Line 1"]);
    h.tui.request_render(false);
    h.wait_for_render();
    assert!(h.tui.full_redraws() > initial);
    let v = h.terminal.get_viewport();
    assert!(v[0].contains("Line 0") && v[1].contains("Line 1"));
    assert_eq!(v[2].trim(), "");
    assert_eq!(v[3].trim(), "");
    h.tui.stop(Default::default());
}

#[test]
fn handles_shrink_to_single_line_and_empty() {
    let h = Harness::new(40, 10);
    h.tui.set_clear_on_shrink(true);
    let component = TestComponent::new(&["Line 0", "Line 1", "Line 2", "Line 3"]);
    h.add(component.clone());
    h.tui.start();
    h.wait_for_render();
    component.set(&["Only line"]);
    h.tui.request_render(false);
    h.wait_for_render();
    let v = h.terminal.get_viewport();
    assert!(v[0].contains("Only line"));
    assert_eq!(v[1].trim(), "");
    component.set(&[]);
    h.tui.request_render(false);
    h.wait_for_render();
    let v = h.terminal.get_viewport();
    assert_eq!(v[0].trim(), "");
    assert_eq!(v[1].trim(), "");
    h.tui.stop(Default::default());
}

#[test]
fn tracks_cursor_when_content_shrinks_with_unchanged_lines() {
    let h = Harness::new(40, 10);
    let component = TestComponent::new(&[]);
    component.set_owned(lines(5, "Line"));
    h.add(component.clone());
    h.tui.start();
    h.wait_for_render();
    component.set(&["Line 0", "Line 1", "Line 2"]);
    h.tui.request_render(false);
    h.wait_for_render();
    component.set(&["Line 0", "CHANGED", "Line 2"]);
    h.tui.request_render(false);
    h.wait_for_render();
    assert!(h.terminal.get_viewport()[1].contains("CHANGED"));
    h.tui.stop(Default::default());
}

#[test]
fn spinner_middle_line_changes() {
    let h = Harness::new(40, 10);
    let component = TestComponent::new(&["Header", "Working...", "Footer"]);
    h.add(component.clone());
    h.tui.start();
    h.wait_for_render();
    for frame in ["|", "/", "-", "\\"] {
        component.set_owned(vec!["Header".into(), format!("Working {frame}"), "Footer".into()]);
        h.tui.request_render(false);
        h.wait_for_render();
        let v = h.terminal.get_viewport();
        assert!(v[0].contains("Header"));
        assert!(v[1].contains(&format!("Working {frame}")));
        assert!(v[2].contains("Footer"));
    }
    h.tui.stop(Default::default());
}

#[test]
fn first_last_and_non_adjacent_line_changes() {
    let h = Harness::new(40, 10);
    let component = TestComponent::new(&["Line 0", "Line 1", "Line 2", "Line 3"]);
    h.add(component.clone());
    h.tui.start();
    h.wait_for_render();
    component.set(&["CHANGED", "Line 1", "Line 2", "Line 3"]);
    h.tui.request_render(false);
    h.wait_for_render();
    assert_eq!(&h.terminal.get_viewport()[..4], ["CHANGED", "Line 1", "Line 2", "Line 3"]);
    component.set(&["Line 0", "Line 1", "Line 2", "CHANGED"]);
    h.tui.request_render(false);
    h.wait_for_render();
    assert_eq!(&h.terminal.get_viewport()[..4], ["Line 0", "Line 1", "Line 2", "CHANGED"]);
    component.set(&["Line 0", "CHANGED 1", "Line 2", "CHANGED 3", "Line 4"]);
    h.tui.request_render(false);
    h.wait_for_render();
    assert_eq!(&h.terminal.get_viewport()[..5], ["Line 0", "CHANGED 1", "Line 2", "CHANGED 3", "Line 4"]);
    h.tui.stop(Default::default());
}

#[test]
fn content_to_empty_and_back() {
    let h = Harness::new(40, 10);
    let component = TestComponent::new(&["Line 0", "Line 1", "Line 2"]);
    h.add(component.clone());
    h.tui.start();
    h.wait_for_render();
    assert!(h.terminal.get_viewport()[0].contains("Line 0"));
    component.set(&[]);
    h.tui.request_render(false);
    h.wait_for_render();
    component.set(&["New Line 0", "New Line 1"]);
    h.tui.request_render(false);
    h.wait_for_render();
    let v = h.terminal.get_viewport();
    assert!(v[0].contains("New Line 0"), "{v:?}");
    assert!(v[1].contains("New Line 1"));
    h.tui.stop(Default::default());
}

#[test]
fn full_rerender_when_deleted_lines_move_viewport_up() {
    let h = Harness::new(20, 5);
    let component = TestComponent::new(&[]);
    component.set_owned(lines(12, "Line"));
    h.add(component.clone());
    h.tui.start();
    h.wait_for_render();
    let initial = h.tui.full_redraws();
    component.set_owned(lines(7, "Line"));
    h.tui.request_render(false);
    h.wait_for_render();
    assert!(h.tui.full_redraws() > initial);
    assert_eq!(h.terminal.get_viewport(), ["Line 2", "Line 3", "Line 4", "Line 5", "Line 6"]);
    h.tui.stop(Default::default());
}

#[test]
fn appends_after_shrink_without_another_full_redraw() {
    let h = Harness::new(20, 5);
    let component = TestComponent::new(&[]);
    component.set_owned(lines(8, "Line"));
    h.add(component.clone());
    h.tui.start();
    h.wait_for_render();
    let initial = h.tui.full_redraws();
    component.set(&["Line 0", "Line 1"]);
    h.tui.request_render(false);
    h.wait_for_render();
    assert!(h.tui.full_redraws() > initial);
    let after_shrink = h.tui.full_redraws();
    component.set(&["Line 0", "Line 1", "Line 2"]);
    h.tui.request_render(false);
    h.wait_for_render();
    assert_eq!(h.tui.full_redraws(), after_shrink);
    assert_eq!(h.terminal.get_viewport(), ["Line 0", "Line 1", "Line 2", "", ""]);
    h.tui.stop(Default::default());
}

#[test]
fn clears_stale_content_after_transient_component() {
    let h = Harness::new(40, 10);
    let chat = TestComponent::new(&[]);
    let editor = TestComponent::new(&[]);
    h.add(chat.clone());
    h.add(editor.clone());
    let editor_lines = vec!["Editor 0".to_string(), "Editor 1".into(), "Editor 2".into()];
    chat.set_owned(lines(15, "Chat"));
    editor.set_owned(editor_lines.clone());
    h.tui.start();
    h.wait_for_render();
    editor.set_owned(lines(8, "Selector"));
    h.tui.request_render(false);
    h.wait_for_render();
    editor.set_owned(editor_lines);
    h.tui.request_render(false);
    h.wait_for_render();
    let before = h.tui.full_redraws();
    chat.set_owned(lines(12, "Chat"));
    h.tui.request_render(false);
    h.wait_for_render();
    assert!(h.tui.full_redraws() > before);
    assert_eq!(
        h.terminal.get_viewport(),
        ["Chat 5", "Chat 6", "Chat 7", "Chat 8", "Chat 9", "Chat 10", "Chat 11", "Editor 0", "Editor 1", "Editor 2"]
    );
    h.tui.stop(Default::default());
}

#[test]
fn omits_focused_fake_cursor_when_truncation_dropped_end_marker() {
    let h = Harness::with_options(20, 5, pi_tui::tui::TuiOptions { show_hardware_cursor: Some(true), ..Default::default() });
    let base = TestComponent::new(&[]);
    base.set_owned(vec![slice_by_column(&format!("ab{CURSOR_MARKER}{}def", render_fake_cursor("c")), 0, 3, true)]);
    h.add(base);
    h.tui.start();
    h.wait_for_render();
    let writes = h.terminal.get_writes();
    assert!(!writes.contains("\x1b[7m"));
    assert!(writes.ends_with("\x1b[3G"), "{writes:?}");
    h.tui.stop(Default::default());
}

#[test]
fn keeps_cursor_when_overlay_ends_directly_left_of_it() {
    for show_hw in [false, true] {
        let h = Harness::with_options(20, 5, pi_tui::tui::TuiOptions { show_hardware_cursor: Some(show_hw), ..Default::default() });
        let base = TestComponent::new(&[]);
        base.set_owned(vec![format!("abc{CURSOR_MARKER}{}ef", render_fake_cursor("d"))]);
        h.add(base);
        let overlay = TestComponent::new(&["XXX"]);
        h.tui.show_overlay(
            overlay,
            Some(OverlayOptions {
                row: Some(SizeValue::Abs(0)),
                col: Some(SizeValue::Abs(0)),
                width: Some(SizeValue::Abs(3)),
                ..Default::default()
            }),
        );
        h.tui.start();
        h.wait_for_render();
        let writes = h.terminal.get_writes();
        assert!(writes.contains("XXX"));
        assert_eq!(writes.contains("\x1b[7md"), !show_hw);
        assert!(writes.ends_with("\x1b[4G"), "{writes:?}");
        h.tui.stop(Default::default());
    }
}

#[test]
fn fake_cursor_reverse_video_and_hardware_cursor() {
    use pi_tui::components::input::Input;
    let h = Harness::new(20, 5);
    let input = Arc::new(Input::default());
    input.set_value("abc");
    let c = h.add(input);
    h.tui.set_focus(Some(c));
    h.tui.start();
    h.wait_for_render();
    assert!(h.terminal.get_writes().contains("> \x1b[7ma\x1b[27mbc"));
    h.terminal.clear_writes();
    h.tui.set_show_hardware_cursor(true);
    h.wait_for_render();
    let writes = h.terminal.get_writes();
    assert!(!writes.contains("\x1b[7m"));
    assert!(!writes.contains("\x1b_pi:"));
    h.tui.stop(Default::default());
}

#[test]
fn keeps_fake_cursor_of_unfocused_input() {
    use pi_tui::components::input::Input;
    let h = Harness::with_options(20, 5, pi_tui::tui::TuiOptions { show_hardware_cursor: Some(true), ..Default::default() });
    let input = Arc::new(Input::default());
    input.set_value("abc");
    h.add(input);
    h.tui.start();
    h.wait_for_render();
    assert!(h.terminal.get_writes().contains("> \x1b[7ma\x1b[27mbc"));
    h.tui.stop(Default::default());
}
