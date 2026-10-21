//! Port of `packages/coding-agent/src/modes/interactive/bug-report.ts`.

use crate::core::agent_session::AgentSession;
use crate::core::keybindings::KeybindingsManager;
use pi_tui::components::editor::EditorComponent;
use pi_tui::tui::ComponentRef;
use pi_tui::tui::Container;
use pi_tui::tui::MouseEvent;
use pi_tui::tui::MouseResult;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct BugReportContext {
    pub session: Arc<AgentSession>,
    pub ui: Arc<Tui>,
    pub editor_container: Arc<Container>,
    pub editor: Arc<dyn EditorComponent>,
    pub keybindings: Arc<KeybindingsManager>,
    pub show_status: Arc<dyn Fn(String) + Send + Sync>,
    pub show_error: Arc<dyn Fn(String) + Send + Sync>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BugReportOptionsDelivery {
    #[serde(rename = "upload")]
    Upload,
    #[serde(rename = "zip")]
    Zip,
}

impl BugReportOptionsDelivery {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Zip => "zip",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub include_session: bool,
    pub include_summary: bool,
    pub delivery: BugReportOptionsDelivery,
}

#[derive(Clone)]
pub struct OverlayMouseLayoutChildren {
    pub component: ComponentRef,
    pub height: i64,
}

#[derive(Clone)]
pub struct OverlayMouseLayout {
    pub width: i64,
    pub children: Vec<OverlayMouseLayoutChildren>,
}

// TS: Container & { dispose?: () => void }
#[derive(Clone)]
pub struct Overlay {
    pub children: Vec<ComponentRef>,
    pub mouse_layout: Option<OverlayMouseLayout>,
    pub add_child: Arc<dyn Fn(ComponentRef) + Send + Sync>,
    pub remove_child: Arc<dyn Fn(ComponentRef) + Send + Sync>,
    pub clear: Arc<dyn Fn() + Send + Sync>,
    pub invalidate: Arc<dyn Fn() + Send + Sync>,
    pub handle_mouse: Arc<dyn Fn(MouseEvent) -> Option<Arc<MouseResult>> + Send + Sync>,
    pub render: Arc<dyn Fn(i64) -> Vec<String> + Send + Sync>,
    pub dispose: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// Run the `/bug` flow: consent, optional summary, then upload or export.
pub async fn report_bug(
    context: &BugReportContext,
    initial_hint: Option<&str>,
) -> anyhow::Result<()> {
    todo!("port modes/interactive/bug-report.ts:reportBug")
}
