//! Shared helpers for TUI integration tests.
#![allow(dead_code)]

pub mod chalk;

use std::sync::{Arc, Mutex};

use pi_tui::event_loop::{CurrentGuard, EventLoop};
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{Component, ComponentRef, Tui, TuiOptions};

/// A component rendering fixed lines.
#[derive(Default)]
pub struct TestComponent {
    pub lines: Mutex<Vec<String>>,
    pub render_count: Mutex<usize>,
}

impl TestComponent {
    pub fn new(lines: &[&str]) -> Arc<Self> {
        let c = Arc::new(Self::default());
        c.set(lines);
        c
    }
    pub fn set(&self, lines: &[&str]) {
        *self.lines.lock().unwrap() = lines.iter().map(|s| s.to_string()).collect();
    }
    pub fn set_owned(&self, lines: Vec<String>) {
        *self.lines.lock().unwrap() = lines;
    }
    pub fn get(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }
    pub fn renders(&self) -> usize {
        *self.render_count.lock().unwrap()
    }
}

impl Component for TestComponent {
    fn render(&self, _width: usize) -> Vec<String> {
        *self.render_count.lock().unwrap() += 1;
        self.lines.lock().unwrap().clone()
    }
}

/// A component that records input and shows the last input as its only line.
#[derive(Default)]
pub struct InputComponent {
    pub inner: TestComponent,
    pub inputs: Mutex<Vec<String>>,
}

impl Component for InputComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.inner.render(width)
    }
    fn handles_input(&self) -> bool {
        true
    }
    fn handle_input(&self, data: &str) {
        self.inputs.lock().unwrap().push(data.to_string());
        self.inner.set(&[data]);
    }
}

pub struct Harness {
    pub lp: EventLoop,
    pub _guard: CurrentGuard,
    pub terminal: VirtualTerminal,
    pub tui: Arc<Tui>,
}

impl Harness {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_options(cols, rows, TuiOptions::default())
    }

    pub fn with_options(cols: usize, rows: usize, mut options: TuiOptions) -> Self {
        let lp = EventLoop::new_manual();
        let guard = lp.make_current();
        let terminal = VirtualTerminal::new(cols, rows);
        options.event_loop = Some(lp.clone());
        let tui = Tui::main_screen_with(Arc::new(terminal.clone()), options);
        Self { lp, _guard: guard, terminal, tui }
    }

    /// Let pending ticks and the throttled render pipeline settle (~20ms).
    pub fn wait_for_render(&self) {
        self.lp.run_pending();
        self.lp.advance_ms(20);
    }

    pub fn add(&self, c: Arc<impl Component>) -> ComponentRef {
        let c: ComponentRef = c;
        self.tui.add_child(c.clone());
        c
    }
}

pub fn as_ref<T: Component>(c: &Arc<T>) -> ComponentRef {
    c.clone()
}
