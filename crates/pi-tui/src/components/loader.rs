//! Animated loading indicator. Port of `components/loader.ts` and
//! `components/cancellable-loader.ts`.

use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use crate::StyleFn;
use crate::components::text::Text;
use crate::event_loop::{AbortController, EventLoop, TimerId};
pub use crate::event_loop::AbortSignal;
use crate::keybindings::get_keybindings;
use crate::tui::{Component, Tui};

const DEFAULT_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const DEFAULT_INTERVAL_MS: u64 = 80;

/// Loader indicator options.
#[derive(Debug, Clone, Default)]
pub struct LoaderIndicatorOptions {
    /// Animation frames. Use an empty list to hide the indicator.
    pub frames: Option<Vec<String>>,
    /// Frame interval in milliseconds for animated indicators.
    pub interval_ms: Option<u64>,
}

struct State {
    frames: Vec<String>,
    interval: Duration,
    current_frame: usize,
    interval_id: Option<TimerId>,
    render_indicator_verbatim: bool,
    message: String,
}

/// Loader component that updates with an optional spinning animation.
pub struct Loader {
    weak_self: Weak<Loader>,
    text: Text,
    ui: Weak<Tui>,
    event_loop: EventLoop,
    spinner_color: StyleFn,
    message_color: StyleFn,
    st: Mutex<State>,
}

impl Loader {
    pub fn new(
        ui: &Arc<Tui>,
        spinner_color: StyleFn,
        message_color: StyleFn,
        message: impl Into<String>,
        indicator: Option<LoaderIndicatorOptions>,
    ) -> Arc<Self> {
        let loader = Arc::new_cyclic(|weak| Loader {
            weak_self: weak.clone(),
            text: Text::new("", 1, 0, None),
            ui: Arc::downgrade(ui),
            event_loop: ui.event_loop().clone(),
            spinner_color,
            message_color,
            st: Mutex::new(State {
                frames: DEFAULT_FRAMES.iter().map(|s| s.to_string()).collect(),
                interval: Duration::from_millis(DEFAULT_INTERVAL_MS),
                current_frame: 0,
                interval_id: None,
                render_indicator_verbatim: false,
                message: message.into(),
            }),
        });
        loader.set_indicator(indicator);
        loader
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn start(&self) {
        self.update_display();
        self.restart_animation();
    }

    pub fn stop(&self) {
        let id = self.lock().interval_id.take();
        if let Some(id) = id {
            self.event_loop.clear_timer(id);
        }
    }

    pub fn set_message(&self, message: impl Into<String>) {
        self.lock().message = message.into();
        self.update_display();
    }

    pub fn message(&self) -> String {
        self.lock().message.clone()
    }

    pub fn set_indicator(&self, indicator: Option<LoaderIndicatorOptions>) {
        {
            let mut s = self.lock();
            s.render_indicator_verbatim = indicator.is_some();
            s.frames = match indicator.as_ref().and_then(|i| i.frames.clone()) {
                Some(f) => f,
                None => DEFAULT_FRAMES.iter().map(|s| s.to_string()).collect(),
            };
            s.interval = Duration::from_millis(
                indicator.as_ref().and_then(|i| i.interval_ms).filter(|ms| *ms > 0).unwrap_or(DEFAULT_INTERVAL_MS),
            );
            s.current_frame = 0;
        }
        self.start();
    }

    fn restart_animation(&self) {
        self.stop();
        let (frames, interval) = {
            let s = self.lock();
            (s.frames.len(), s.interval)
        };
        if frames <= 1 {
            return;
        }
        let weak = self.weak_self.clone();
        let id = self.event_loop.set_interval(interval, move || {
            if let Some(me) = weak.upgrade() {
                {
                    let mut s = me.lock();
                    let n = s.frames.len().max(1);
                    s.current_frame = (s.current_frame + 1) % n;
                }
                me.update_display();
            }
        });
        self.lock().interval_id = Some(id);
    }

    /// The indicator for the current frame (styled unless custom frames were given).
    pub fn rendered_indicator(&self) -> String {
        let (frame, verbatim) = {
            let s = self.lock();
            (s.frames.get(s.current_frame).cloned().unwrap_or_default(), s.render_indicator_verbatim)
        };
        if verbatim { frame } else { (self.spinner_color)(&frame) }
    }

    fn update_display(&self) {
        let frame = self.rendered_indicator();
        let indicator = if frame.is_empty() { String::new() } else { format!("{frame} ") };
        let message = self.lock().message.clone();
        self.text.set_text(format!("{indicator}{}", (self.message_color)(&message)));
        if let Some(ui) = self.ui.upgrade() {
            ui.request_render(false);
        }
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        if let Some(id) = self.st.get_mut().map(|s| s.interval_id.take()).unwrap_or(None) {
            self.event_loop.clear_timer(id);
        }
    }
}

impl Component for Loader {
    fn render(&self, width: usize) -> Vec<String> {
        let mut lines = vec![String::new()];
        lines.extend(self.text.render(width));
        lines
    }
    fn invalidate(&self) {
        self.text.invalidate();
        self.update_display();
    }
}

/// Loader that can be cancelled with Escape.
pub struct CancellableLoader {
    loader: Arc<Loader>,
    controller: AbortController,
    on_abort: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl CancellableLoader {
    pub fn new(
        ui: &Arc<Tui>,
        spinner_color: StyleFn,
        message_color: StyleFn,
        message: impl Into<String>,
        indicator: Option<LoaderIndicatorOptions>,
    ) -> Arc<Self> {
        Arc::new(Self {
            loader: Loader::new(ui, spinner_color, message_color, message, indicator),
            controller: AbortController::new(),
            on_abort: Mutex::new(None),
        })
    }

    pub fn loader(&self) -> &Arc<Loader> {
        &self.loader
    }

    /// Called when the user presses Escape.
    pub fn set_on_abort(&self, f: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.on_abort.lock().unwrap_or_else(|e| e.into_inner()) = f;
    }

    pub fn signal(&self) -> AbortSignal {
        self.controller.signal()
    }

    pub fn aborted(&self) -> bool {
        self.controller.signal().is_aborted()
    }

    pub fn dispose(&self) {
        self.loader.stop();
    }
}

impl Component for CancellableLoader {
    fn render(&self, width: usize) -> Vec<String> {
        self.loader.render(width)
    }
    fn invalidate(&self) {
        self.loader.invalidate();
    }
    fn handles_input(&self) -> bool {
        true
    }
    fn handle_input(&self, data: &str) {
        if get_keybindings().matches(data, "tui.select.cancel") {
            self.controller.abort();
            let cb = self.on_abort.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(cb) = cb {
                cb();
            }
        }
    }
}
