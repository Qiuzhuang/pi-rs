//! Fullscreen (alternate screen) frame output while a response streams and tools run.
//!
//! Regression tests for flicker reported in fullscreen mode: every frame after the first must be a
//! differential update (only changed rows, no `\x1b[2J`), written in one write call wrapped in
//! synchronized output (DEC 2026), and a frame must never show a component that another thread is
//! rebuilding (container cleared, children not yet re-added).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pi_tui::components::text::Text;
use pi_tui::event_loop::{CurrentGuard, EventLoop};
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{Container, Tui, TuiOptions, TuiStopOptions};
use pi_tui::tui_alt_screen::TuiAltScreenOptions;

const BSU: &str = "\x1b[?2026h";
const ESU: &str = "\x1b[?2026l";

struct Alt {
    lp: EventLoop,
    _guard: CurrentGuard,
    terminal: VirtualTerminal,
    tui: Arc<Tui>,
}

impl Alt {
    fn new(cols: usize, rows: usize) -> Self {
        let lp = EventLoop::new_manual();
        let guard = lp.make_current();
        let terminal = VirtualTerminal::new(cols, rows);
        let tui = Tui::alt_screen_with(
            Arc::new(terminal.clone()),
            TuiOptions { event_loop: Some(lp.clone()), ..Default::default() },
            TuiAltScreenOptions { mouse: Some(false), ..Default::default() },
        );
        Self { lp, _guard: guard, terminal, tui }
    }

    /// Request a render and let the throttled frame run.
    fn frame(&self) {
        self.tui.request_render(false);
        self.lp.run_pending();
        self.lp.advance_ms(20);
        self.lp.run_pending();
    }

    fn viewport(&self) -> Vec<String> {
        self.terminal.get_viewport().iter().map(|l| l.trim_end().to_string()).collect()
    }
}

impl Drop for Alt {
    fn drop(&mut self) {
        self.tui.stop(TuiStopOptions::default());
    }
}

fn text(s: &str) -> Arc<Text> {
    Arc::new(Text::new(s, 0, 0, None))
}

/// Rows a frame rewrites (`CSI row;1H CSI 2K` per changed row), in order.
fn rows_written(frame: &str) -> Vec<usize> {
    let mut rows = Vec::new();
    let mut rest = frame;
    while let Some(i) = rest.find("\x1b[") {
        rest = &rest[i + 2..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && rest[digits.len()..].starts_with(";1H\x1b[2K") {
            rows.push(digits.parse().unwrap());
        }
    }
    rows
}

/// The write calls since the last check, asserting each one is a whole synchronized frame.
fn take_frames(h: &Alt) -> Vec<String> {
    let calls = h.terminal.write_calls();
    h.terminal.clear_writes();
    for call in &calls {
        assert!(call.starts_with(BSU), "frame not opened with synchronized output: {call:?}");
        assert!(call.ends_with(ESU), "frame not closed with synchronized output: {call:?}");
        assert_eq!(call.matches(BSU).count(), 1, "one synchronized block per write: {call:?}");
        assert!(!call.contains("\x1b[2J"), "differential frame cleared the screen: {call:?}");
        assert!(!call.contains("\x1b[?25h"), "frame showed the hidden cursor: {call:?}");
    }
    calls
}

#[test]
fn streaming_text_spinner_and_tool_updates_are_differential_frames() {
    let h = Alt::new(50, 12);
    let user = text("Please run the check");
    let assistant = text("…");
    let tool = Arc::new(Container::new());
    let spinner = text("⠋ Working");
    h.tui.add_child(user.clone());
    h.tui.add_child(assistant.clone());
    h.tui.add_child(tool.clone());
    h.tui.add_child(spinner.clone());
    h.tui.start();
    h.frame();
    assert_eq!(h.tui.full_redraws(), 1);
    let first = h.terminal.write_calls().concat();
    assert!(first.contains("\x1b[2J"), "the first frame clears the alternate screen");
    h.terminal.clear_writes();

    // Text streams in word by word: one frame per delta, rewriting only the assistant row.
    let mut streamed = String::new();
    for word in ["I", " will", " run", " a", " command"] {
        streamed.push_str(word);
        assistant.set_text(streamed.clone());
        h.frame();
        let frames = take_frames(&h);
        assert_eq!(frames.len(), 1, "one write per frame");
        assert_eq!(rows_written(&frames[0]), [2], "only the streaming row changes: {:?}", frames[0]);
    }

    // A spinner tick rewrites only its own row (the empty tool container renders no rows yet).
    spinner.set_text("⠙ Working");
    h.frame();
    let frames = take_frames(&h);
    assert_eq!(frames.len(), 1);
    assert_eq!(rows_written(&frames[0]), [3]);

    // The tool row appears (its rows and the rows pushed down by it are rewritten).
    tool.add_child(text("$ for i in 1 2 3"));
    h.frame();
    let frames = take_frames(&h);
    assert_eq!(rows_written(&frames[0]), [3, 4]);

    // Tool output updates rebuild the tool's children; only rows that differ are written.
    for n in 1..=3 {
        tool.clear();
        tool.add_child(text("$ for i in 1 2 3"));
        tool.add_child(text(&(1..=n).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")));
        h.frame();
        let frames = take_frames(&h);
        assert_eq!(frames.len(), 1);
        let expected: Vec<usize> = (3 + n..=4 + n).collect();
        assert_eq!(rows_written(&frames[0]), expected, "frame {n}: {:?}", frames[0]);
    }
    assert_eq!(
        h.viewport()[..8],
        ["Please run the check", "I will run a command", "$ for i in 1 2 3", "line 1", "line 2", "line 3", "⠙ Working", ""]
    );

    // A render with nothing changed writes no rows (cursor bookkeeping only).
    h.frame();
    let frames = take_frames(&h);
    assert!(frames.iter().all(|f| rows_written(f).is_empty()), "{frames:?}");
    assert_eq!(h.tui.full_redraws(), 1, "no full redraw while streaming");
}

#[test]
fn output_growing_past_the_viewport_scrolls_without_clearing() {
    let h = Alt::new(30, 6);
    let output = text("");
    h.tui.add_child(output.clone());
    h.tui.start();
    h.frame();
    h.terminal.clear_writes();
    let mut lines: Vec<String> = Vec::new();
    for i in 1..=20 {
        lines.push(format!("out {i}"));
        output.set_text(lines.join("\n"));
        h.frame();
        let frames = take_frames(&h);
        assert_eq!(frames.len(), 1);
        let rows = rows_written(&frames[0]);
        if i <= 6 {
            assert_eq!(rows, [i], "line {i} fills the next empty row");
        } else {
            // Following the end: every visible row shifts by one line.
            assert_eq!(rows, [1, 2, 3, 4, 5, 6]);
        }
    }
    assert_eq!(h.viewport(), ["out 15", "out 16", "out 17", "out 18", "out 19", "out 20"]);
    assert_eq!(h.tui.full_redraws(), 1);
}

/// A component rebuilt on another thread (as agent events do) must never be rendered half done
/// when the update holds the UI lock: the frame waits for the rebuild instead.
#[test]
fn frames_never_show_a_component_rebuilt_under_the_ui_lock() {
    let h = Alt::new(40, 8);
    let tool = Arc::new(Container::new());
    tool.add_child(text("write big.py"));
    tool.add_child(text("def f(x):"));
    h.tui.add_child(text("I'll write a file."));
    h.tui.add_child(tool.clone());
    h.tui.start();
    h.frame();
    assert_eq!(h.viewport()[..3], ["I'll write a file.", "write big.py", "def f(x):"]);

    let done = Arc::new(AtomicBool::new(false));
    let updater = {
        let tool = tool.clone();
        let done = done.clone();
        std::thread::spawn(move || {
            for i in 0..2000 {
                let _ui = pi_tui::ui_lock::lock_ui();
                tool.clear();
                std::thread::yield_now();
                tool.add_child(text("write big.py"));
                std::thread::yield_now();
                tool.add_child(text(&format!("def f(x): # {i}")));
            }
            done.store(true, Ordering::SeqCst);
        })
    };
    let mut frames = 0;
    while !done.load(Ordering::SeqCst) {
        h.tui.render_now(false);
        frames += 1;
        let screen = h.viewport();
        assert_eq!(screen[1], "write big.py", "frame {frames} rendered a half-rebuilt tool: {screen:?}");
        assert!(screen[2].starts_with("def f(x):"), "frame {frames}: {screen:?}");
    }
    updater.join().unwrap();
    assert_eq!(h.tui.full_redraws(), 1);
}
