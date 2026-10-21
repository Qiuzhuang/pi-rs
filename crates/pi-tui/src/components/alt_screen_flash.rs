//! Transient messages composited by the alternate-screen renderer.
//! Port of `components/alt-screen-flash.ts`.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::event_loop::{EventLoop, TimerId};
use crate::tui::Component;
use crate::utils::truncate_to_width;

const DEFAULT_DURATION_MS: u64 = 1000;

struct FlashEntry {
    id: u64,
    message: String,
    timer: TimerId,
}

#[derive(Default)]
struct State {
    entries: Vec<FlashEntry>,
    next_id: u64,
}

/// A stack of transient flash messages.
pub struct AltScreenFlashContainer {
    st: Arc<Mutex<State>>,
    request_render: Arc<dyn Fn() + Send + Sync>,
    event_loop: EventLoop,
}

fn lock(st: &Mutex<State>) -> MutexGuard<'_, State> {
    st.lock().unwrap_or_else(|e| e.into_inner())
}

impl AltScreenFlashContainer {
    pub fn new(request_render: Arc<dyn Fn() + Send + Sync>, event_loop: EventLoop) -> Self {
        Self { st: Arc::new(Mutex::new(State::default())), request_render, event_loop }
    }

    /// Show `message` for `duration_ms` (default 1000 ms).
    pub fn flash(&self, message: &str, duration_ms: Option<u64>) {
        let id = {
            let mut s = lock(&self.st);
            let id = s.next_id;
            s.next_id += 1;
            id
        };
        let st = Arc::downgrade(&self.st);
        let request_render = self.request_render.clone();
        let timer = self.event_loop.set_timeout(Duration::from_millis(duration_ms.unwrap_or(DEFAULT_DURATION_MS)), move || {
            let Some(st) = st.upgrade() else { return };
            let removed = {
                let mut s = lock(&st);
                match s.entries.iter().position(|e| e.id == id) {
                    Some(index) => {
                        s.entries.remove(index);
                        true
                    }
                    None => false,
                }
            };
            if removed {
                request_render();
            }
        });
        lock(&self.st).entries.push(FlashEntry { id, message: message.to_string(), timer });
        (self.request_render)();
    }

    pub fn dispose(&self) {
        let entries = std::mem::take(&mut lock(&self.st).entries);
        for entry in entries {
            self.event_loop.clear_timer(entry.timer);
        }
    }
}

impl Component for AltScreenFlashContainer {
    fn render(&self, width: usize) -> Vec<String> {
        lock(&self.st)
            .entries
            .iter()
            .map(|entry| {
                let message = truncate_to_width(&format!(" {} ", entry.message), width, "", false);
                format!("\x1b[7m{message}\x1b[27m")
            })
            .collect()
    }
}
