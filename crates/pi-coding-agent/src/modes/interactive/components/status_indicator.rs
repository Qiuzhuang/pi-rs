//! Port of `packages/coding-agent/src/modes/interactive/components/status-indicator.ts`.

use crate::core::extensions::types::WorkingIndicatorOptions;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
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

/// TS: `extends Loader`
pub struct StatusIndicator {
    // TODO(port): private state (interior mutability; methods take &self)
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
        todo!("port modes/interactive/components/status-indicator.ts:StatusIndicator.constructor")
    }

    pub fn kind(&self) -> StatusIndicatorKind {
        todo!("port modes/interactive/components/status-indicator.ts:StatusIndicator.kind")
    }

    pub fn render_in_border(&self, width: i64) -> String {
        todo!(
            "port modes/interactive/components/status-indicator.ts:StatusIndicator.renderInBorder"
        )
    }

    pub fn render_spinner_in_border(&self, width: i64) -> String {
        todo!(
            "port modes/interactive/components/status-indicator.ts:StatusIndicator.renderSpinnerInBorder"
        )
    }

    pub fn dispose(&self) {
        todo!("port modes/interactive/components/status-indicator.ts:StatusIndicator.dispose")
    }
}

// TODO(port): impl pi-tui::Loader for StatusIndicator

impl Component for StatusIndicator {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/status-indicator.ts:StatusIndicator.render")
    }
}

/// TS: `extends StatusIndicator`
pub struct WorkingStatusIndicator {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl WorkingStatusIndicator {
    pub fn new(
        ui: Arc<Tui>,
        message: &str,
        indicator: Option<&WorkingIndicatorOptions>,
        color_fn: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/status-indicator.ts:WorkingStatusIndicator.constructor"
        )
    }
}

impl Component for WorkingStatusIndicator {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/status-indicator.ts:WorkingStatusIndicator.render")
    }
}

/// TS: `extends StatusIndicator`
pub struct RetryStatusIndicator {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl RetryStatusIndicator {
    pub fn new(ui: Arc<Tui>, attempt: i64, max_attempts: i64, delay_ms: i64) -> Self {
        todo!(
            "port modes/interactive/components/status-indicator.ts:RetryStatusIndicator.constructor"
        )
    }

    pub fn dispose(&self) {
        todo!("port modes/interactive/components/status-indicator.ts:RetryStatusIndicator.dispose")
    }
}

impl Component for RetryStatusIndicator {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/status-indicator.ts:RetryStatusIndicator.render")
    }
}

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
    // TODO(port): private state (interior mutability; methods take &self)
}

impl CompactionStatusIndicator {
    pub fn new(
        ui: Arc<Tui>,
        reason: &CompactionStatusReason,
        color_fn: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/status-indicator.ts:CompactionStatusIndicator.constructor"
        )
    }
}

impl Component for CompactionStatusIndicator {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/status-indicator.ts:CompactionStatusIndicator.render"
        )
    }
}

/// TS: `extends StatusIndicator`
pub struct BranchSummaryStatusIndicator {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl BranchSummaryStatusIndicator {
    pub fn new(ui: Arc<Tui>) -> Self {
        todo!(
            "port modes/interactive/components/status-indicator.ts:BranchSummaryStatusIndicator.constructor"
        )
    }
}

impl Component for BranchSummaryStatusIndicator {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/status-indicator.ts:BranchSummaryStatusIndicator.render"
        )
    }
}

/// TS: `implements Component`
pub struct IdleStatus {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl IdleStatus {
    pub fn new() -> Self {
        todo!("port modes/interactive/components/status-indicator.ts:IdleStatus.constructor")
    }
}

impl Component for IdleStatus {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/status-indicator.ts:IdleStatus.render")
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/status-indicator.ts:IdleStatus.invalidate")
    }
}
