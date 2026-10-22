//! Port of `packages/coding-agent/src/modes/interactive/components/status-indicator.ts`.
//!
//! TS subclasses of `StatusIndicator` are wrapper structs that deref to the shared
//! [`StatusIndicator`] (see [`WorkingStatusIndicator::indicator`]), so hosts can hold any of them
//! as `Arc<StatusIndicator>` like the TS base-class references. Subclass state that `dispose()`
//! must release (the retry countdown) lives in the base.

use crate::core::extensions::types::WorkingIndicatorOptions;
use crate::modes::interactive::components::countdown_timer::CountdownTimer;
use crate::modes::interactive::components::keybinding_hints::key_text;
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use parking_lot::Mutex;
use pi_tui::StyleFn;
use pi_tui::components::loader::{Loader, LoaderIndicatorOptions};
use pi_tui::tui::{Component, Tui};
use pi_tui::utils::{js_trim_end, truncate_to_width};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StatusIndicatorKind {
    #[serde(rename = "working")]
    Working,
    #[serde(rename = "retry")]
    Retry,
    #[serde(rename = "compaction")]
    Compaction,
    #[serde(rename = "branchSummary")]
    BranchSummary,
}

impl StatusIndicatorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Retry => "retry",
            Self::Compaction => "compaction",
            Self::BranchSummary => "branchSummary",
        }
    }
}

type ColorFn = Arc<dyn Fn(String) -> String + Send + Sync>;

fn style_fn(f: ColorFn) -> StyleFn {
    Arc::new(move |s: &str| f(s.to_string()))
}

/// TS: `extends Loader`
pub struct StatusIndicator {
    kind: StatusIndicatorKind,
    loader: Arc<Loader>,
    countdown: Mutex<Option<CountdownTimer>>,
}

impl StatusIndicator {
    pub fn new(
        kind: &StatusIndicatorKind,
        ui: Arc<Tui>,
        spinner_color_fn: Arc<dyn Fn(String) -> String + Send + Sync>,
        message_color_fn: Arc<dyn Fn(String) -> String + Send + Sync>,
        message: &str,
        indicator: Option<&WorkingIndicatorOptions>,
    ) -> Self {
        let indicator = indicator
            .map(|i| LoaderIndicatorOptions { frames: i.frames.clone(), interval_ms: i.interval_ms });
        let loader = Loader::new(&ui, style_fn(spinner_color_fn), style_fn(message_color_fn), message, indicator);
        Self { kind: *kind, loader, countdown: Mutex::new(None) }
    }

    pub fn kind(&self) -> StatusIndicatorKind {
        self.kind
    }

    /// The underlying loader.
    pub fn loader(&self) -> &Arc<Loader> {
        &self.loader
    }

    pub fn set_message(&self, message: &str) {
        self.loader.set_message(message);
    }

    pub fn render_in_border(&self, width: i64) -> String {
        let lines = self.loader.render((width + 2).max(0) as usize);
        let line = lines.get(1).cloned().unwrap_or_default();
        let trimmed = match line.strip_prefix(' ') {
            Some(rest) => js_trim_end(rest),
            None => js_trim_end(&line),
        };
        truncate_to_width(trimmed, width.max(0) as usize, "", false)
    }

    pub fn render_spinner_in_border(&self, width: i64) -> String {
        truncate_to_width(&self.loader.rendered_indicator(), width.max(0) as usize, "", false)
    }

    pub fn dispose(&self) {
        if let Some(countdown) = self.countdown.lock().take() {
            countdown.dispose();
        }
        self.loader.stop();
    }
}

impl Component for StatusIndicator {
    fn render(&self, width: usize) -> Vec<String> {
        self.loader.render(width)
    }

    fn invalidate(&self) {
        self.loader.invalidate();
    }
}

macro_rules! status_indicator_subclass {
    ($ty:ident) => {
        impl $ty {
            /// This indicator as its base class.
            pub fn indicator(&self) -> Arc<StatusIndicator> {
                self.inner.clone()
            }
        }

        impl Deref for $ty {
            type Target = StatusIndicator;
            fn deref(&self) -> &StatusIndicator {
                &self.inner
            }
        }

        impl Component for $ty {
            fn render(&self, width: usize) -> Vec<String> {
                self.inner.render(width)
            }

            fn invalidate(&self) {
                self.inner.invalidate();
            }
        }
    };
}

/// TS: `extends StatusIndicator`
pub struct WorkingStatusIndicator {
    inner: Arc<StatusIndicator>,
}

impl WorkingStatusIndicator {
    pub fn new(
        ui: Arc<Tui>,
        message: &str,
        indicator: Option<&WorkingIndicatorOptions>,
        color_fn: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    ) -> Self {
        let spinner: ColorFn =
            color_fn.clone().unwrap_or_else(|| Arc::new(|text: String| theme().fg(&ThemeColor::Accent, &text)));
        let message_color: ColorFn =
            color_fn.unwrap_or_else(|| Arc::new(|text: String| theme().fg(&ThemeColor::Muted, &text)));
        Self {
            inner: Arc::new(StatusIndicator::new(
                &StatusIndicatorKind::Working,
                ui,
                spinner,
                message_color,
                message,
                indicator,
            )),
        }
    }
}

status_indicator_subclass!(WorkingStatusIndicator);

/// TS: `extends StatusIndicator`
pub struct RetryStatusIndicator {
    inner: Arc<StatusIndicator>,
}

impl RetryStatusIndicator {
    pub fn new(ui: Arc<Tui>, attempt: i64, max_attempts: i64, delay_ms: i64) -> Self {
        let retry_message = move |seconds: i64| {
            format!(
                "Retrying ({attempt}/{max_attempts}) in {seconds}s... ({} to cancel)",
                key_text("app.interrupt".to_string())
            )
        };
        let inner = Arc::new(StatusIndicator::new(
            &StatusIndicatorKind::Retry,
            ui.clone(),
            Arc::new(|spinner: String| theme().fg(&ThemeColor::Warning, &spinner)),
            Arc::new(|text: String| theme().fg(&ThemeColor::Muted, &text)),
            &retry_message((delay_ms as f64 / 1000.0).ceil() as i64),
            None,
        ));
        let loader = Arc::downgrade(&inner.loader);
        let countdown = CountdownTimer::new(
            delay_ms,
            Some(ui),
            Arc::new(move |seconds| {
                if let Some(loader) = loader.upgrade() {
                    loader.set_message(retry_message(seconds));
                }
            }),
            // The countdown disposes itself when it expires.
            Arc::new(|| {}),
        );
        *inner.countdown.lock() = Some(countdown);
        Self { inner }
    }

    pub fn dispose(&self) {
        self.inner.dispose();
    }
}

status_indicator_subclass!(RetryStatusIndicator);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompactionStatusReason {
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "threshold")]
    Threshold,
    #[serde(rename = "overflow")]
    Overflow,
}

impl CompactionStatusReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Threshold => "threshold",
            Self::Overflow => "overflow",
        }
    }
}

/// TS: `extends StatusIndicator`
pub struct CompactionStatusIndicator {
    inner: Arc<StatusIndicator>,
}

impl CompactionStatusIndicator {
    pub fn new(
        ui: Arc<Tui>,
        reason: &CompactionStatusReason,
        color_fn: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    ) -> Self {
        let cancel_hint = format!("({} to cancel)", key_text("app.interrupt".to_string()));
        let label = match reason {
            CompactionStatusReason::Manual => format!("Compacting context... {cancel_hint}"),
            CompactionStatusReason::Overflow => {
                format!("Context overflow detected, Auto-compacting... {cancel_hint}")
            }
            CompactionStatusReason::Threshold => format!("Auto-compacting... {cancel_hint}"),
        };
        let spinner: ColorFn =
            color_fn.clone().unwrap_or_else(|| Arc::new(|text: String| theme().fg(&ThemeColor::Accent, &text)));
        let message_color: ColorFn =
            color_fn.unwrap_or_else(|| Arc::new(|text: String| theme().fg(&ThemeColor::Muted, &text)));
        Self {
            inner: Arc::new(StatusIndicator::new(
                &StatusIndicatorKind::Compaction,
                ui,
                spinner,
                message_color,
                &label,
                None,
            )),
        }
    }
}

status_indicator_subclass!(CompactionStatusIndicator);

/// TS: `extends StatusIndicator`
pub struct BranchSummaryStatusIndicator {
    inner: Arc<StatusIndicator>,
}

impl BranchSummaryStatusIndicator {
    pub fn new(ui: Arc<Tui>) -> Self {
        Self {
            inner: Arc::new(StatusIndicator::new(
                &StatusIndicatorKind::BranchSummary,
                ui,
                Arc::new(|spinner: String| theme().fg(&ThemeColor::Accent, &spinner)),
                Arc::new(|text: String| theme().fg(&ThemeColor::Muted, &text)),
                &format!("Summarizing branch... ({} to cancel)", key_text("app.interrupt".to_string())),
                None,
            )),
        }
    }
}

status_indicator_subclass!(BranchSummaryStatusIndicator);

/// TS: `implements Component`
#[derive(Default)]
pub struct IdleStatus {}

impl IdleStatus {
    pub fn new() -> Self {
        Self {}
    }
}

impl Component for IdleStatus {
    fn render(&self, width: usize) -> Vec<String> {
        let empty_line = " ".repeat(width);
        vec![empty_line.clone(), empty_line]
    }

    fn invalidate(&self) {
        // No cached state to invalidate.
    }
}
