//! Assorted ported tests: program status, terminal colors (parsing and TUI queries),
//! escape timeouts, shrinking content and truncated text.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use common::Harness;
use pi_tui::components::truncated_text::TruncatedText;
use pi_tui::event_loop::{CurrentGuard, EventLoop};
use pi_tui::program_status::*;
use pi_tui::stdin_buffer::{StdinBuffer, StdinEvent};
use pi_tui::terminal::{InputHandler, ResizeHandler, Terminal, resolve_escape_timeout_ms};
use pi_tui::terminal_colors::*;
use pi_tui::tui::{Component, Tui, TuiOptions, TuiStopOptions};
use pi_tui::utils::visible_width;

fn decode_message(sequence: &str) -> Option<String> {
    let start = sequence.find(":msg=")? + 5;
    let rest = &sequence[start..];
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')).unwrap_or(rest.len());
    let bytes = base64::engine::general_purpose::STANDARD.decode(&rest[..end]).ok()?;
    String::from_utf8(bytes).ok()
}

#[test]
fn format_program_status_encodes_fields() {
    let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
    assert_eq!(
        format_program_status(&ProgramStatus {
            state: ProgramState::Blocked,
            app: Some("pi".into()),
            kind: Some(BlockedKind::Permission),
            message: Some("Allow bash?".into()),
        }),
        format!("\x1b]7501;state=blocked:app=pi:kind=permission:msg={}\x1b\\", b64("Allow bash?"))
    );
    assert_eq!(
        format_program_status(&ProgramStatus { state: ProgramState::Clear, app: None, kind: None, message: None }),
        "\x1b]7501;state=clear\x1b\\"
    );
    assert_eq!(
        format_program_status(&ProgramStatus {
            state: ProgramState::Working,
            app: Some("my app".into()),
            kind: Some(BlockedKind::Auth),
            message: Some(" \n ".into()),
        }),
        "\x1b]7501;state=working\x1b\\"
    );
    let idle = |app: String| format_program_status(&ProgramStatus { state: ProgramState::Idle, app: Some(app), kind: None, message: None });
    assert!(idle("a".repeat(32)).contains(&format!(":app={}\x1b", "a".repeat(32))));
    assert!(!idle("a".repeat(33)).contains("app="));

    let seq = format_program_status(&ProgramStatus {
        state: ProgramState::Error,
        app: None,
        kind: None,
        message: Some("first\nsecond\x1b[31m\u{9b}third\t".into()),
    });
    assert_eq!(decode_message(&seq).unwrap(), "first second [31m third");

    let seq = format_program_status(&ProgramStatus {
        state: ProgramState::Working,
        app: Some("pi".into()),
        kind: None,
        message: Some("é".repeat(2000)),
    });
    let message = decode_message(&seq).unwrap();
    assert_eq!(message, "é".repeat(1024));
    assert!(message.len() <= 2048);
    assert!(seq.len() <= 4096);

    assert!(is_program_status_reply("\x1b]7501;?\x1b\\"));
    assert!(is_program_status_reply("\x1b]7501;?\x07"));
    assert!(is_program_status_reply("\x1b]7501;?version=2\x1b\\"));
    assert!(!is_program_status_reply("\x1b]7501;state=idle\x1b\\"));
    assert!(!is_program_status_reply("\x1b]11;rgb:0000/0000/0000\x07"));
}

#[test]
fn parses_color_scheme_reports_and_osc_colors() {
    assert_eq!(parse_terminal_color_scheme_report("\x1b[?997;1n"), Some(TerminalColorScheme::Dark));
    assert_eq!(parse_terminal_color_scheme_report("\x1b[?997;2n"), Some(TerminalColorScheme::Light));
    assert_eq!(parse_terminal_color_scheme_report("\x1b[?997;2n\x1b[?997;1n\x1b[?997;1n"), Some(TerminalColorScheme::Dark));
    assert_eq!(parse_terminal_color_scheme_report("\x1b[?997;1n\x1b[?997;2n\x1b[?997;2n"), Some(TerminalColorScheme::Light));
    assert_eq!(parse_terminal_color_scheme_report("\x1b[?997;3n"), None);
    assert_eq!(parse_terminal_color_scheme_report("\x1b[?996n"), None);
    assert_eq!(parse_terminal_color_scheme_report("x\x1b[?997;1n"), None);

    let fg = parse_osc_color_response("\x1b]10;rgb:ffff/ffff/ffff\x07").unwrap();
    assert_eq!(fg.target, OscColorTarget::Foreground);
    assert_eq!(fg.rgb, Some(RgbColor { r: 255.0, g: 255.0, b: 255.0 }));
    let p = parse_osc_color_response("\x1b]4;13;#ff0080\x1b\\").unwrap();
    assert_eq!(p.target, OscColorTarget::Palette(13));
    assert_eq!(p.rgb, Some(RgbColor { r: 255.0, g: 0.0, b: 128.0 }));
    let bogus = parse_osc_color_response("\x1b]4;1;bogus\x07").unwrap();
    assert_eq!(bogus.target, OscColorTarget::Palette(1));
    assert_eq!(bogus.rgb, None);
    assert!(parse_osc_color_response("\x1b]12;#ffffff\x07").is_none());
}

/// A terminal that records writes and delivers input synchronously.
#[derive(Default)]
struct TestTerminal {
    input: Mutex<Option<InputHandler>>,
    writes: Mutex<Vec<String>>,
}

impl TestTerminal {
    fn send(&self, data: &str) {
        let handler = self.input.lock().unwrap().clone();
        if let Some(h) = handler {
            h(data.to_string());
        }
    }
}

impl Terminal for TestTerminal {
    fn start(&self, on_input: InputHandler, _on_resize: ResizeHandler) {
        *self.input.lock().unwrap() = Some(on_input);
    }
    fn stop(&self) {
        *self.input.lock().unwrap() = None;
    }
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
}

#[derive(Default)]
struct InputRecorder {
    inputs: Mutex<Vec<String>>,
}

impl Component for InputRecorder {
    fn render(&self, _width: usize) -> Vec<String> {
        Vec::new()
    }
    fn handles_input(&self) -> bool {
        true
    }
    fn handle_input(&self, data: &str) {
        self.inputs.lock().unwrap().push(data.to_string());
    }
}

struct ColorSetup {
    lp: EventLoop,
    _guard: CurrentGuard,
    terminal: Arc<TestTerminal>,
    tui: Arc<Tui>,
    component: Arc<InputRecorder>,
}

impl ColorSetup {
    fn new() -> Self {
        let lp = EventLoop::new_manual();
        let guard = lp.make_current();
        let terminal = Arc::new(TestTerminal::default());
        let tui = Tui::main_screen_with(terminal.clone(), TuiOptions { event_loop: Some(lp.clone()), ..Default::default() });
        let component = Arc::new(InputRecorder::default());
        tui.add_child(component.clone());
        tui.set_focus(Some(component.clone()));
        tui.start();
        Self { lp, _guard: guard, terminal, tui, component }
    }
    fn send(&self, data: &str) {
        self.terminal.send(data);
        self.lp.run_pending();
    }
    fn inputs(&self) -> Vec<String> {
        self.component.inputs.lock().unwrap().clone()
    }
}

impl Drop for ColorSetup {
    fn drop(&mut self) {
        self.tui.stop(TuiStopOptions::default());
    }
}

const DA1: &str = "\x1b[?62;22c";
const BLACK: RgbColor = RgbColor { r: 0.0, g: 0.0, b: 0.0 };
const WHITE: RgbColor = RgbColor { r: 255.0, g: 255.0, b: 255.0 };

fn palette_replies() -> Vec<String> {
    (0..16).map(|i| format!("\x1b]4;{i};#000000\x07")).collect()
}

#[test]
fn queries_all_colors_in_one_write_and_consumes_replies() {
    let s = ColorSetup::new();
    let query = s.tui.query_terminal_colors(Duration::from_millis(1000), None);
    let written = s.terminal.writes.lock().unwrap().last().cloned().unwrap_or_default();
    assert!(written.starts_with("\x1b]10;?\x07\x1b]11;?\x07\x1b]4;0;?\x07") && written.ends_with("\x1b[c"));
    s.send("x");
    s.send("\x1b]10;#ffffff\x07");
    s.send("\x1b]11;rgb:0000/0000/0000\x1b\\");
    for reply in palette_replies() {
        s.send(&reply);
    }
    let colors = query.try_take().expect("resolved without DA1");
    assert_eq!(colors, TerminalColors { foreground: Some(WHITE), background: Some(BLACK), palette: Some(vec![BLACK; 16]) });
    s.send(DA1);
    assert_eq!(s.inputs(), vec!["x"]);
}

#[test]
fn consumes_delayed_ghostty_color_fragments() {
    let s = ColorSetup::new();
    let query = s.tui.query_terminal_colors(Duration::from_millis(1000), None);
    let mut buffer = StdinBuffer::new(Default::default());
    let deliver = |events: Vec<StdinEvent>, s: &ColorSetup| {
        for e in events {
            if let StdinEvent::Data(d) = e {
                s.send(&d);
            }
        }
    };
    deliver(buffer.process("\x1b]4;1;rgb:eaea/6c"), &s);
    // Let the buffer's escape timeout pass without flushing (as in the reference test).
    deliver(buffer.process(&format!("6c/6d6d\x07{DA1}")), &s);
    assert_eq!(query.try_take(), Some(TerminalColors { foreground: None, background: None, palette: None }));
    assert!(s.inputs().is_empty());
    deliver(buffer.process("\x07"), &s);
    assert_eq!(s.inputs(), vec!["\x07"]);
}

#[test]
fn resolves_on_da1_with_replies_in_query_order() {
    let s = ColorSetup::new();
    let first = s.tui.query_terminal_colors(Duration::from_millis(1000), None);
    let second = s.tui.query_terminal_colors(Duration::from_millis(1000), None);
    s.send("\x1b]11;#000000\x07");
    for reply in palette_replies().iter().take(8) {
        s.send(reply);
    }
    s.send(DA1);
    s.send(DA1);
    assert_eq!(first.try_take(), Some(TerminalColors { foreground: None, background: Some(BLACK), palette: None }));
    assert_eq!(second.try_take(), Some(TerminalColors { foreground: None, background: None, palette: None }));
}

#[test]
fn reports_late_replies_after_timeout() {
    let s = ColorSetup::new();
    let late = Arc::new(Mutex::new(Vec::new()));
    let l = late.clone();
    let query = s.tui.query_terminal_colors(Duration::from_millis(1), Some(Arc::new(move |c| l.lock().unwrap().push(c))));
    s.lp.advance_ms(5);
    assert_eq!(query.try_take().unwrap().background, None);
    s.send("\x1b]11;#ffffff\x07");
    s.send(DA1);
    assert_eq!(*late.lock().unwrap(), vec![TerminalColors { foreground: None, background: Some(WHITE), palette: None }]);
    assert!(s.inputs().is_empty());
    s.send("\x1b]11;#ffffff\x07");
    assert_eq!(s.inputs(), vec!["\x1b]11;#ffffff\x07"]);
}

#[test]
fn resolves_escape_timeouts() {
    let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
    assert_eq!(resolve_escape_timeout_ms(&env(&[("PI_TUI_ESC_TIMEOUT", "250")])), 250.0);
    assert_eq!(resolve_escape_timeout_ms(&env(&[("PI_TUI_ESC_TIMEOUT", "bogus")])), 10.0);
    assert_eq!(resolve_escape_timeout_ms(&env(&[("SSH_CONNECTION", "1 2 3 4")])), 100.0);
    assert_eq!(resolve_escape_timeout_ms(&env(&[])), 10.0);
}

#[test]
fn clears_all_rendered_lines_when_content_shrinks_to_zero() {
    let h = Harness::new(40, 10);
    let c = common::TestComponent::new(&["Line 0", "Line 1", "Line 2", "Line 3", "Line 4"]);
    h.tui.add_child(c.clone());
    h.tui.start();
    h.wait_for_render();
    assert!(h.terminal.get_viewport().iter().any(|l| l.contains("Line 4")));
    c.set(&[]);
    h.tui.request_render(false);
    h.wait_for_render();
    assert!(h.terminal.get_viewport().iter().all(|l| l.trim().is_empty()), "{:?}", h.terminal.get_viewport());
}

#[test]
fn truncated_text_pads_and_truncates() {
    let t = TruncatedText::new("Hello world", 1, 0);
    let lines = t.render(50);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 50);

    let t = TruncatedText::new("Hello", 0, 2);
    let lines = t.render(40);
    assert_eq!(lines.len(), 5);
    assert!(lines.iter().all(|l| visible_width(l) == 40));

    let long = "This is a very long piece of text that will definitely exceed the available width";
    let t = TruncatedText::new(long, 1, 0);
    let lines = t.render(30);
    assert_eq!(visible_width(&lines[0]), 30);
    assert!(lines[0].contains("..."));

    let t = TruncatedText::new("\x1b[31mHello\x1b[0m \x1b[34mworld\x1b[0m", 1, 0);
    let lines = t.render(40);
    assert_eq!(visible_width(&lines[0]), 40);
    assert!(lines[0].contains("\x1b["));

    let t = TruncatedText::new(format!("\x1b[31m{}\x1b[0m", "a".repeat(100)), 1, 0);
    let lines = t.render(20);
    assert_eq!(visible_width(&lines[0]), 20);
    assert!(lines[0].contains("\x1b[0m..."));

    let t = TruncatedText::new("Hello world", 1, 0);
    let lines = t.render(13);
    assert_eq!(visible_width(&lines[0]), 13);
    assert!(!lines[0].contains("..."));

    let t = TruncatedText::new("", 1, 0);
    let lines = t.render(30);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 30);

    let t = TruncatedText::new("First line\nSecond line\nThird line", 1, 0);
    let lines = t.render(50);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("First line") && !lines[0].contains("Second line"));

    let t = TruncatedText::new(format!("{}\nSecond line", "a".repeat(100)), 1, 0);
    let lines = t.render(25);
    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 25);
    assert!(lines[0].contains("...") && !lines[0].contains("Second"));
}
