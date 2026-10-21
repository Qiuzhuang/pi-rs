//! Port of `packages/coding-agent/src/modes/interactive/tui-renderer.ts`.

use crate::core::settings_schema::TuiMode;
use pi_tui::terminal::Terminal;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct InteractiveTuiOptions {
    pub tui_mode: TuiMode,
    pub show_hardware_cursor: bool,
    pub log_directory: String,
    pub terminal: Option<Arc<dyn Terminal>>,
    pub on_right_click_paste: Option<Arc<dyn Fn() + Send + Sync>>,
    pub fullscreen_copy_on_select: Option<bool>,
    pub fullscreen_wheel_scroll_lines: Option<Value /* TS: pi-tui::WheelScrollLines */>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CreateInteractiveTuiCreateInteractiveTuiResult {
    TuiMainScreen(Value /* TS: pi-tui::TuiMainScreen */),
    TuiAltScreen(Value /* TS: pi-tui::TuiAltScreen */),
}

pub fn create_interactive_tui(
    options: &InteractiveTuiOptions,
) -> CreateInteractiveTuiCreateInteractiveTuiResult {
    todo!("port modes/interactive/tui-renderer.ts:createInteractiveTui")
}

/// Stable reference for components while InteractiveMode replaces the active renderer.
pub fn create_interactive_tui_reference(
    get_tui: Arc<dyn Fn() -> Arc<Tui> + Send + Sync>,
) -> anyhow::Result<Arc<Tui>> {
    todo!("port modes/interactive/tui-renderer.ts:createInteractiveTuiReference")
}
