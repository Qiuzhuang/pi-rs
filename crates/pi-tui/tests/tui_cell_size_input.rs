//! Port of `packages/tui/test/tui-cell-size-input.test.ts`: the TUI queries the cell size
//! (`CSI 16 t`) on start when the terminal supports images, consumes the `CSI 6 ; h ; w t`
//! reply instead of forwarding it to the focused component, and still forwards other input.

use std::sync::{Arc, Mutex, MutexGuard};

use pi_tui::event_loop::EventLoop;
use pi_tui::terminal_image::{CellDimensions, get_cell_dimensions, reset_capabilities_cache, set_cell_dimensions};
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{Component, Tui, TuiOptions, TuiStopOptions};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct InputRecorder {
    inputs: Mutex<Vec<String>>,
}

impl InputRecorder {
    fn inputs(&self) -> Vec<String> {
        self.inputs.lock().unwrap().clone()
    }
}

impl Component for InputRecorder {
    fn render(&self, _width: usize) -> Vec<String> {
        vec![String::new()]
    }
    fn handles_input(&self) -> bool {
        true
    }
    fn handle_input(&self, data: &str) {
        self.inputs.lock().unwrap().push(data.to_string());
    }
}

/// `withImageTerminal`: Ghostty (kitty images) for the duration of `f`, environment restored after.
fn with_image_terminal<T>(f: impl FnOnce() -> T) -> T {
    let _lock: MutexGuard<'_, ()> = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let saved: Vec<(&str, Option<String>)> = ["TERM_PROGRAM", "TERM", "GHOSTTY_RESOURCES_DIR"]
        .into_iter()
        .map(|name| (name, std::env::var(name).ok()))
        .collect();
    // SAFETY: environment access in this binary is serialized by ENV_LOCK.
    unsafe {
        std::env::set_var("TERM_PROGRAM", "ghostty");
        std::env::remove_var("TERM");
        std::env::remove_var("GHOSTTY_RESOURCES_DIR");
    }
    reset_capabilities_cache();
    struct Restore(Vec<(&'static str, Option<String>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (name, value) in &self.0 {
                // SAFETY: as above.
                unsafe {
                    match value {
                        Some(v) => std::env::set_var(name, v),
                        None => std::env::remove_var(name),
                    }
                }
            }
            reset_capabilities_cache();
        }
    }
    let _restore = Restore(saved);
    f()
}

fn start_tui(terminal: &Arc<VirtualTerminal>, lp: &EventLoop) -> (Arc<Tui>, Arc<InputRecorder>) {
    let tui = Tui::main_screen_with(terminal.clone(), TuiOptions { event_loop: Some(lp.clone()), ..Default::default() });
    let recorder = Arc::new(InputRecorder::default());
    tui.set_focus(Some(recorder.clone()));
    tui.start();
    (tui, recorder)
}

#[test]
fn forwards_bare_escape_even_when_a_cell_size_query_was_sent_at_startup() {
    with_image_terminal(|| {
        let lp = EventLoop::new_manual();
        let _guard = lp.make_current();
        let terminal = Arc::new(VirtualTerminal::new(80, 24));
        let (tui, recorder) = start_tui(&terminal, &lp);
        assert!(terminal.get_writes().contains("\x1b[16t"), "the cell size query is sent at startup");

        terminal.send_input("\x1b");
        lp.run_pending();

        assert_eq!(recorder.inputs(), vec!["\x1b".to_string()]);
        tui.stop(TuiStopOptions::default());
    });
}

#[test]
fn consumes_cell_size_responses_and_still_forwards_later_user_input() {
    with_image_terminal(|| {
        set_cell_dimensions(CellDimensions { width_px: 9, height_px: 18 });
        let lp = EventLoop::new_manual();
        let _guard = lp.make_current();
        let terminal = Arc::new(VirtualTerminal::new(80, 24));
        let (tui, recorder) = start_tui(&terminal, &lp);

        terminal.send_input("\x1b[6;20;10t");
        lp.run_pending();
        assert!(recorder.inputs().is_empty());
        assert_eq!(get_cell_dimensions(), CellDimensions { width_px: 10, height_px: 20 });

        terminal.send_input("q");
        lp.run_pending();
        assert_eq!(recorder.inputs(), vec!["q".to_string()]);
        tui.stop(TuiStopOptions::default());
    });
}
