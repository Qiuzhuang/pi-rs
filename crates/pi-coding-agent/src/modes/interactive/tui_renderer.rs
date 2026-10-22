//! Port of `packages/coding-agent/src/modes/interactive/tui-renderer.ts`.

use crate::compat::WheelScrollLines;
use crate::core::settings_schema::TuiMode;
use crate::modes::interactive::components::keybinding_hints::key_display_text;
use crate::modes::interactive::theme::theme::{ThemeBg, ThemeColor, theme};
use futures::FutureExt;
use pi_tui::terminal::{ProcessTerminal, Terminal};
use pi_tui::tui::{Tui, TuiOptions};
use pi_tui::tui_alt_screen::{CopySelectionResult, TuiAltScreenOptions};
use std::path::PathBuf;
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

/// TS returns a `TuiMainScreen` or a `TuiAltScreen`; in Rust both are a [`Tui`] (see [`Tui::mode`]).
pub type CreateInteractiveTuiResult = Arc<Tui>;

pub(crate) fn to_tui_wheel_lines(value: &Option<WheelScrollLines>) -> pi_tui::wheel_scroll::WheelScrollLines {
    match value {
        Some(WheelScrollLines::Lines(n)) => pi_tui::wheel_scroll::WheelScrollLines::Lines(*n as f64),
        _ => pi_tui::wheel_scroll::WheelScrollLines::Auto,
    }
}

pub fn create_interactive_tui(options: &InteractiveTuiOptions) -> CreateInteractiveTuiResult {
    let terminal: Arc<dyn Terminal> = options.terminal.clone().unwrap_or_else(|| Arc::new(ProcessTerminal::new()));
    let tui_options = TuiOptions {
        show_hardware_cursor: Some(options.show_hardware_cursor),
        log_directory: Some(PathBuf::from(&options.log_directory)),
        event_loop: None,
    };
    if options.tui_mode == TuiMode::Fullscreen {
        let style_search_match =
            |text: &str| theme().bg(&ThemeBg::SearchMatchBg, &theme().fg(&ThemeColor::SearchMatchText, text));
        let alt = TuiAltScreenOptions {
            wheel_scroll_lines: Some(to_tui_wheel_lines(&options.fullscreen_wheel_scroll_lines)),
            mouse: None,
            search_match_style: Some(Arc::new(move |text: &str| theme().underline(&style_search_match(text)))),
            search_current_match_style: Some(Arc::new(move |text: &str| {
                theme().bold(&theme().inverse(&style_search_match(text)))
            })),
            search_navigation_button_style: Some(Arc::new(|text: &str, hovered: bool| {
                if hovered { theme().underline(text) } else { text.to_string() }
            })),
            scroll_to_end_indicator: Some(Arc::new(|| {
                let shortcut = key_display_text("tui.altScreen.bottom".to_string());
                let label = format!(
                    " ↓ Jump to latest message{} ",
                    if shortcut.is_empty() { String::new() } else { format!(" · {shortcut}") }
                );
                theme().bg(&ThemeBg::SelectedBg, &theme().fg(&ThemeColor::Text, &label))
            })),
            open_url: Some(Arc::new(|url: &str| crate::utils::open_browser::open_browser(url))),
            on_right_click_paste: options.on_right_click_paste.clone(),
            copy_on_select: options.fullscreen_copy_on_select,
            copy_selection: Some(Arc::new(|text: &str| {
                let text = text.to_string();
                async move {
                    match crate::utils::clipboard::copy_to_clipboard(&text).await {
                        Ok(()) => CopySelectionResult::Copied,
                        Err(error) => CopySelectionResult::Error(error.to_string()),
                    }
                }
                .boxed()
            })),
        };
        return Tui::alt_screen_with(terminal, tui_options, alt);
    }
    Tui::main_screen_with(terminal, tui_options)
}

/// Stable reference for components while InteractiveMode replaces the active renderer.
///
/// TS returns a proxy that always forwards to the current renderer. A Rust `Arc<Tui>` cannot forward,
/// so this returns the current renderer; InteractiveMode rebuilds the components that hold the TUI
/// when it switches renderers.
pub fn create_interactive_tui_reference(get_tui: Arc<dyn Fn() -> Arc<Tui> + Send + Sync>) -> anyhow::Result<Arc<Tui>> {
    Ok(get_tui())
}
