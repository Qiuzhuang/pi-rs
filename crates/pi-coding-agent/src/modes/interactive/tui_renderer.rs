//! Port of `packages/coding-agent/src/modes/interactive/tui-renderer.ts`.

use crate::compat::WheelScrollLines;
use crate::core::settings_schema::TuiMode;
use pi_tui::terminal::Terminal;
use pi_tui::tui::Tui;
use std::sync::Arc;

#[derive(Clone)]
pub struct InteractiveTuiOptions {
    pub tui_mode: TuiMode,
    pub show_hardware_cursor: bool,
    pub log_directory: String,
    pub terminal: Option<Arc<dyn Terminal>>,
    pub on_right_click_paste: Option<Arc<dyn Fn() + Send + Sync>>,
    pub fullscreen_copy_on_select: Option<bool>,
    pub fullscreen_wheel_scroll_lines: Option<WheelScrollLines>,
}

#[derive(Clone)]
pub enum EmptyCreateInteractiveTuiResult {
    Arc(Arc<Tui>),
    Arc2(Arc<Tui>),
}

pub fn create_interactive_tui(options: &InteractiveTuiOptions) -> EmptyCreateInteractiveTuiResult {
    todo!("port modes/interactive/tui-renderer.ts:createInteractiveTui")
}

/// Stable reference for components while InteractiveMode replaces the active renderer.
pub fn create_interactive_tui_reference(
    get_tui: Arc<dyn Fn() -> Arc<Tui> + Send + Sync>,
) -> anyhow::Result<Arc<Tui>> {
    todo!("port modes/interactive/tui-renderer.ts:createInteractiveTuiReference")
}
