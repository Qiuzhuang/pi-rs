//! Port of `test/theme-controller.test.ts`. The TS test stubs the TUI's color query; here the real
//! TUI runs on a `VirtualTerminal` with a manual event loop, and the terminal's replies (OSC 10/11
//! colors, DA1, DEC 2031 color scheme reports) are sent as input.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, MutexGuard};
use pi_coding_agent::core::settings_manager::SettingsManager;
use pi_coding_agent::modes::interactive::theme::theme::{
    TerminalTheme, ThemeAppearance, ThemeColor, init_theme, set_terminal_color_scheme, set_terminal_colors, theme,
};
use pi_coding_agent::modes::interactive::theme::theme_controller::{
    InteractiveThemeController, InteractiveThemeControllerOptions,
};
use pi_tui::event_loop::EventLoop;
use pi_tui::terminal_colors::TerminalColors;
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{Tui, TuiOptions};
use serde_json::json;

static SERIAL: Mutex<()> = Mutex::new(());

const DARK_REPLY: &str = "\x1b]10;rgb:f8f8/f8f8/f2f2\x07\x1b]11;rgb:2828/2a2a/3636\x07\x1b[?62c";
const LIGHT_REPLY: &str = "\x1b]10;rgb:1e1e/1e1e/1e1e\x07\x1b]11;rgb:fafa/fafa/fafa\x07\x1b[?62c";
const EMPTY_REPLY: &str = "\x1b[?62c";

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    terminal: VirtualTerminal,
    event_loop: EventLoop,
    tui: Arc<Tui>,
}

impl Fixture {
    fn new() -> Fixture {
        let guard = SERIAL.lock();
        // Truecolor, so reported colors show up as RGB escapes (TS checks `theme.colors.error.kind`).
        pi_tui::terminal_image::set_capabilities(pi_tui::terminal_image::TerminalCapabilities {
            images: None,
            true_color: true,
            hyperlinks: false,
        });
        reset_theme_state();
        let terminal = VirtualTerminal::new(80, 24);
        let event_loop = EventLoop::new_manual();
        let tui = Tui::main_screen_with(
            Arc::new(terminal.clone()),
            TuiOptions { event_loop: Some(event_loop.clone()), ..Default::default() },
        );
        tui.start();
        event_loop.run_pending();
        Fixture { _guard: guard, terminal, event_loop, tui }
    }

    fn controller(
        &self,
        manager: impl Fn() -> Arc<SettingsManager> + Send + Sync + 'static,
        initial: Option<&str>,
    ) -> Arc<InteractiveThemeController> {
        InteractiveThemeController::new(
            self.tui.clone(),
            &InteractiveThemeControllerOptions {
                get_settings_manager: Arc::new(manager),
                show_error: Arc::new(|_| {}),
                on_changed: Arc::new(|| {}),
                initial_theme_setting: initial.map(str::to_string),
            },
        )
    }

    /// Answer the pending color query like a terminal would, one reply sequence at a time (a real
    /// terminal's input is split into sequences before it reaches the TUI).
    fn reply(&self, reply: &str) {
        for sequence in reply.split_inclusive('\x07') {
            self.terminal.send_input(sequence);
        }
        self.flush();
    }

    /// TS `await flush()`: run due callbacks.
    fn flush(&self) {
        for _ in 0..5 {
            self.event_loop.run_pending();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Let the color query time out.
    fn time_out(&self) {
        self.event_loop.advance(Duration::from_secs(5));
        self.flush();
    }

    fn color_queries(&self) -> usize {
        self.terminal.get_writes().matches("\x1b]10;?").count()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.tui.stop(Default::default());
        reset_theme_state();
    }
}

/// TS `afterEach`: forget terminal colors, the reported scheme and the theme.
fn reset_theme_state() {
    set_terminal_colors(Arc::new(TerminalColors::default()));
    set_terminal_color_scheme(None);
    init_theme(Some("dark"), Some(false));
}

fn in_memory(settings: serde_json::Value) -> Arc<SettingsManager> {
    let map = settings.as_object().cloned().unwrap_or_default();
    SettingsManager::in_memory_json(map, None)
}

fn theme_name() -> String {
    theme().name().unwrap_or_default()
}

#[test]
fn uses_the_initial_theme_without_persisting_it() {
    let fx = Fixture::new();
    let manager = in_memory(json!({ "theme": "dark" }));
    let m = manager.clone();
    let controller = fx.controller(move || m.clone(), Some("light"));

    assert_eq!(theme_name(), "light");
    assert_eq!(controller.get_theme_selection().as_deref(), Some("light"));
    controller.apply_from_settings();
    fx.flush();

    assert_eq!(fx.color_queries(), 1);
    assert_eq!(manager.get_theme_setting().as_deref(), Some("dark"));
}

#[tokio::test]
async fn applies_the_theme_immediately_and_lets_startup_wait_for_the_colors() {
    let fx = Fixture::new();
    let controller = fx.controller(|| in_memory(json!({})), None);
    controller.apply_from_settings();

    // Grayscale until the terminal answers.
    assert_eq!(theme_name(), "system");
    assert_eq!(theme().get_fg_ansi(&ThemeColor::Error), "\x1b[39m");

    fx.reply(DARK_REPLY);
    controller.wait_for_terminal_colors().await.unwrap();
    assert!(theme().get_fg_ansi(&ThemeColor::Error).starts_with("\x1b[38;"), "{:?}", theme().get_fg_ansi(&ThemeColor::Error));
}

#[test]
fn falls_back_to_palette_indices_then_applies_colors_that_arrive_after_the_timeout() {
    let fx = Fixture::new();
    let controller = fx.controller(|| in_memory(json!({})), None);
    controller.apply_from_settings();
    fx.time_out();
    assert_eq!(theme().get_fg_ansi(&ThemeColor::Error), "\x1b[38;5;1m");

    fx.reply(DARK_REPLY);
    assert!(theme().get_fg_ansi(&ThemeColor::Error).starts_with("\x1b[38;2;"), "{:?}", theme().get_fg_ansi(&ThemeColor::Error));
}

#[test]
fn re_queries_the_colors_on_appearance_changes_and_lets_them_decide() {
    let fx = Fixture::new();
    let controller = fx.controller(|| in_memory(json!({})), Some("light/dark"));
    controller.apply_from_settings();
    assert!(fx.terminal.get_writes().contains("\x1b[?2031h"));
    fx.reply(LIGHT_REPLY);
    assert_eq!(theme_name(), "light");

    // The report says light, but the terminal renders dark.
    fx.terminal.send_input("\x1b[?997;2n");
    fx.flush();
    fx.reply(DARK_REPLY);
    assert_eq!(theme_name(), "dark");
}

#[test]
fn uses_the_reported_scheme_for_the_system_theme_when_the_terminal_reports_no_colors() {
    let fx = Fixture::new();
    let previous = std::env::var("COLORFGBG").ok();
    // SAFETY: serialized by SERIAL; restored below.
    unsafe { std::env::set_var("COLORFGBG", "") };
    let controller = fx.controller(|| in_memory(json!({})), None);
    controller.apply_from_settings();
    fx.reply(EMPTY_REPLY);
    assert_eq!(theme().appearance(), ThemeAppearance::Dark);

    fx.terminal.send_input("\x1b[?997;2n");
    fx.flush();
    assert_eq!(theme().appearance(), ThemeAppearance::Light);
    assert_eq!(controller.get_terminal_theme(), TerminalTheme::Light);
    unsafe {
        match previous {
            Some(v) => std::env::set_var("COLORFGBG", v),
            None => std::env::remove_var("COLORFGBG"),
        }
    }
}

#[test]
fn disables_terminal_appearance_updates_when_disposed() {
    let fx = Fixture::new();
    let controller = fx.controller(|| in_memory(json!({ "theme": "light/dark" })), None);
    controller.apply_from_settings();
    fx.flush();
    fx.terminal.clear_writes();

    controller.dispose();

    assert!(fx.terminal.get_writes().ends_with("\x1b[?2031l"), "{:?}", fx.terminal.get_writes());
    // Unsubscribed: a later report no longer re-queries the colors.
    fx.terminal.send_input("\x1b[?997;2n");
    fx.flush();
    assert_eq!(fx.color_queries(), 0);
}

#[test]
fn lets_an_explicit_selection_replace_the_initial_theme() {
    let fx = Fixture::new();
    let first = in_memory(json!({ "theme": "dark" }));
    let second = in_memory(json!({ "theme": "light" }));
    let manager = Arc::new(Mutex::new(first));
    let m = manager.clone();
    let controller = fx.controller(move || m.lock().clone(), Some("light"));
    controller.apply_from_settings();

    let result = controller.set_theme_name("dark", None);
    assert!(result.success, "{:?}", result.error);
    *manager.lock() = second;
    controller.apply_from_settings();
    fx.flush();

    assert_eq!(controller.get_theme_selection().as_deref(), Some("dark"));
    assert_eq!(theme_name(), "dark");
}

#[test]
fn reloads_theme_settings_when_no_initial_theme_was_supplied() {
    let fx = Fixture::new();
    let first = in_memory(json!({ "theme": "dark" }));
    let second = in_memory(json!({ "theme": "light" }));
    let manager = Arc::new(Mutex::new(first.clone()));
    let m = manager.clone();
    let controller = fx.controller(move || m.lock().clone(), None);
    controller.apply_from_settings();

    first.apply_overrides_json(json!({ "theme": "light" }).as_object().unwrap());
    controller.apply_from_settings();
    assert_eq!(theme_name(), "light");

    second.apply_overrides_json(json!({ "theme": "dark" }).as_object().unwrap());
    *manager.lock() = second;
    controller.apply_from_settings();
    assert_eq!(theme_name(), "dark");
}
