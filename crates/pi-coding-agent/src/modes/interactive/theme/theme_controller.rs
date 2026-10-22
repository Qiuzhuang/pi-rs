//! Port of `packages/coding-agent/src/modes/interactive/theme/theme-controller.ts`.

use crate::core::settings_manager::SettingsManager;
use crate::modes::interactive::theme::theme::{
    SYSTEM_THEME_NAME, TerminalTheme, Theme, get_terminal_theme, init_theme, mark_terminal_colors_pending,
    parse_auto_theme_setting, resolve_theme_setting, set_terminal_color_scheme, set_terminal_colors, set_theme,
};
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use parking_lot::Mutex;
use pi_tui::terminal_colors::{RgbColor, TerminalColorScheme, TerminalColors};
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Weak};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeResult {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// How long the system theme stays grayscale before falling back to palette indices. Terminals answer
/// the trailing DA1 request right after the color replies, so this only matters for terminals that
/// answer neither. Replies arriving later still apply.
const TERMINAL_QUERY_TIMEOUT_MS: u64 = 100;

/// Query the terminal's colors and pass them to `apply` when the query completes or times out, and again
/// if the terminal answers after the timeout. A failed query applies no colors. Settles after the first apply.
pub async fn request_terminal_colors(
    ui: Arc<Tui>,
    apply: Arc<dyn Fn(Arc<TerminalColors>) + Send + Sync>,
) -> anyhow::Result<()> {
    let late = apply.clone();
    let deferred = ui.query_terminal_colors(
        Duration::from_millis(TERMINAL_QUERY_TIMEOUT_MS),
        Some(Arc::new(move |colors: TerminalColors| late(Arc::new(colors)))),
    );
    let colors = deferred.await;
    apply(Arc::new(colors));
    Ok(())
}

fn same_rgb(a: Option<&RgbColor>, b: Option<&RgbColor>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.r == b.r && a.g == b.g && a.b == b.b,
        _ => false,
    }
}

fn same_terminal_colors(a: &TerminalColors, b: &TerminalColors) -> bool {
    if !same_rgb(a.foreground.as_ref(), b.foreground.as_ref()) || !same_rgb(a.background.as_ref(), b.background.as_ref()) {
        return false;
    }
    match (&a.palette, &b.palette) {
        (None, None) => true,
        (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_rgb(Some(x), Some(y))),
        _ => false,
    }
}

#[derive(Clone)]
pub struct InteractiveThemeControllerOptions {
    pub get_settings_manager: Arc<dyn Fn() -> Arc<SettingsManager> + Send + Sync>,
    pub show_error: Arc<dyn Fn(String) + Send + Sync>,
    pub on_changed: Arc<dyn Fn() + Send + Sync>,
    pub initial_theme_setting: Option<String>,
}

struct State {
    current_theme_setting: Option<String>,
    // Last reported colors; a query that times out keeps them instead of erasing them.
    terminal_colors: Option<TerminalColors>,
    active_theme_name: Option<String>,
    auto_sync_enabled: bool,
    terminal_color_scheme_listener: Option<u64>,
    // Settles when the latest color query completed or timed out, and its colors applied.
    terminal_color_query: Shared<BoxFuture<'static, ()>>,
}

/// Applies the theme setting and keeps it in sync with the terminal. The theme applies immediately, and the
/// terminal's colors update it when they arrive; the system theme renders in grayscale until then. Callers
/// that bake theme colors into content can wait for the colors with `waitForTerminalColors()`.
pub struct InteractiveThemeController {
    weak_self: Weak<InteractiveThemeController>,
    ui: Mutex<Arc<Tui>>,
    get_settings_manager: Arc<dyn Fn() -> Arc<SettingsManager> + Send + Sync>,
    show_error: Arc<dyn Fn(String) + Send + Sync>,
    on_changed: Arc<dyn Fn() + Send + Sync>,
    state: Mutex<State>,
    render_requests: std::sync::atomic::AtomicUsize,
}

impl InteractiveThemeController {
    pub fn new(ui: Arc<Tui>, options: &InteractiveThemeControllerOptions) -> Arc<Self> {
        let controller = Arc::new_cyclic(|weak| InteractiveThemeController {
            weak_self: weak.clone(),
            ui: Mutex::new(ui),
            get_settings_manager: options.get_settings_manager.clone(),
            show_error: options.show_error.clone(),
            on_changed: options.on_changed.clone(),
            state: Mutex::new(State {
                current_theme_setting: options.initial_theme_setting.clone(),
                terminal_colors: None,
                active_theme_name: None,
                auto_sync_enabled: false,
                terminal_color_scheme_listener: None,
                terminal_color_query: async {}.boxed().shared(),
            }),
            render_requests: std::sync::atomic::AtomicUsize::new(0),
        });
        let name = controller.resolve_theme_name();
        controller.state.lock().active_theme_name = Some(name.clone());
        // The system theme starts in grayscale; color follows once the terminal reports its colors.
        mark_terminal_colors_pending();
        init_theme(Some(&name), Some(true));
        controller.bind_terminal_color_scheme_listener();
        controller
    }

    fn ui(&self) -> Arc<Tui> {
        self.ui.lock().clone()
    }

    /// Rebind to a new TUI (after InteractiveMode switched renderers). The color scheme listener moves
    /// with `rebind_tui()`.
    pub fn set_tui(&self, ui: Arc<Tui>) {
        let previous = self.state.lock().terminal_color_scheme_listener.take();
        let old = std::mem::replace(&mut *self.ui.lock(), ui);
        if let Some(id) = previous {
            old.remove_terminal_color_scheme_listener(id);
        }
    }

    /// How many renders the controller requested (TS tests count `ui.requestRender` calls).
    #[doc(hidden)]
    pub fn render_request_count(&self) -> usize {
        self.render_requests.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn request_render(&self, ui: &Tui) {
        self.render_requests.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        ui.request_render(false);
    }

    pub fn rebind_tui(&self) {
        let previous = self.state.lock().terminal_color_scheme_listener.take();
        if let Some(id) = previous {
            self.ui().remove_terminal_color_scheme_listener(id);
        }
        self.bind_terminal_color_scheme_listener();
        let enabled = self.state.lock().auto_sync_enabled;
        self.ui().set_terminal_color_scheme_notifications(enabled);
    }

    /// Apply the theme setting now and query the terminal's colors, which update the theme when they arrive.
    /// Theme pairs and the system theme follow terminal appearance changes.
    pub fn apply_from_settings(&self) {
        let theme_setting = self.get_theme_setting();
        let theme_name = self.resolve_theme_name();
        self.set_auto_sync(
            parse_auto_theme_setting(theme_setting.as_deref()).is_some() || theme_name == SYSTEM_THEME_NAME,
        );
        self.apply_theme_name(&theme_name, theme_setting.is_some());
        self.query_terminal_colors();
    }

    /// Wait until the latest color query completed or timed out.
    pub async fn wait_for_terminal_colors(&self) -> anyhow::Result<()> {
        let query = self.state.lock().terminal_color_query.clone();
        query.await;
        Ok(())
    }

    pub fn get_theme_selection(&self) -> Option<String> {
        let (current, active) = {
            let s = self.state.lock();
            (s.current_theme_setting.clone(), s.active_theme_name.clone())
        };
        current.or_else(|| (self.get_settings_manager)().get_theme_setting()).or(active)
    }

    pub fn set_theme_name(&self, theme_name: &str, show_error: Option<bool>) -> ThemeResult {
        self.set_auto_sync(theme_name == SYSTEM_THEME_NAME);
        let result = self.apply_theme_name(theme_name, show_error.unwrap_or(false));
        if result.success {
            self.state.lock().current_theme_setting = Some(theme_name.to_string());
        }
        result
    }

    pub fn set_theme_setting(&self, theme_setting: &str) {
        self.state.lock().current_theme_setting = Some(theme_setting.to_string());
        self.apply_from_settings();
    }

    pub fn set_theme_instance(&self, theme_instance: Arc<Theme>) -> ThemeResult {
        self.set_auto_sync(false);
        crate::modes::interactive::theme::theme::set_theme_instance(theme_instance);
        self.state.lock().active_theme_name = Some("<in-memory>".to_string());
        self.notify_changed();
        ThemeResult { success: true, error: None }
    }

    pub fn preview(&self, theme_setting_or_name: &str) {
        let active = self.state.lock().active_theme_name.clone();
        let Some(theme_name) = resolve_theme_setting(Some(theme_setting_or_name), &get_terminal_theme()).or(active)
        else {
            return;
        };
        if set_theme(&theme_name, Some(true)).success {
            let ui = self.ui();
            ui.invalidate();
            self.request_render(&ui);
        }
    }

    pub fn disable_auto_sync(&self) {
        self.set_auto_sync(false);
    }

    pub fn dispose(&self) {
        self.set_auto_sync(false);
        let listener = self.state.lock().terminal_color_scheme_listener.take();
        if let Some(id) = listener {
            self.ui().remove_terminal_color_scheme_listener(id);
        }
    }

    pub fn get_terminal_theme(&self) -> TerminalTheme {
        get_terminal_theme()
    }

    fn get_theme_setting(&self) -> Option<String> {
        let current = self.state.lock().current_theme_setting.clone();
        current.or_else(|| (self.get_settings_manager)().get_theme_setting())
    }

    /// The theme for the current setting and terminal appearance. Without a setting, pi uses the system theme.
    fn resolve_theme_name(&self) -> String {
        resolve_theme_setting(self.get_theme_setting().as_deref(), &get_terminal_theme())
            .unwrap_or_else(|| SYSTEM_THEME_NAME.to_string())
    }

    fn apply_theme_name(&self, theme_name: &str, show_error: bool) -> ThemeResult {
        let result = set_theme(theme_name, Some(true));
        self.state.lock().active_theme_name =
            Some(if result.success { theme_name.to_string() } else { SYSTEM_THEME_NAME.to_string() });
        self.notify_changed();
        if !result.success && show_error {
            (self.show_error)(format!(
                "Failed to load theme \"{theme_name}\": {}\nFell back to the system theme.",
                result.error.clone().unwrap_or_default()
            ));
        }
        ThemeResult { success: result.success, error: result.error }
    }

    /// Query the terminal's colors without waiting for them; `waitForTerminalColors()` waits for this query.
    fn query_terminal_colors(&self) {
        let weak = self.weak_self.clone();
        let apply: Arc<dyn Fn(Arc<TerminalColors>) + Send + Sync> = Arc::new(move |colors: Arc<TerminalColors>| {
            if let Some(this) = weak.upgrade() {
                this.apply_terminal_colors(&colors);
            }
        });
        let ui = self.ui();
        let late = apply.clone();
        // Start the query now (the request is written synchronously), wait for it in the shared future.
        let deferred = ui.query_terminal_colors(
            Duration::from_millis(TERMINAL_QUERY_TIMEOUT_MS),
            Some(Arc::new(move |colors: TerminalColors| late(Arc::new(colors)))),
        );
        let query = async move {
            let colors = deferred.await;
            apply(Arc::new(colors));
        }
        .boxed()
        .shared();
        self.state.lock().terminal_color_query = query.clone();
        // Drive the query even if nobody waits for it.
        crate::modes::interactive::theme::theme_controller::spawn_detached(query);
    }

    /// Record reported colors. Re-renders only when they changed.
    fn apply_terminal_colors(&self, reported: &TerminalColors) {
        let next = {
            let mut s = self.state.lock();
            let previous = s.terminal_colors.clone();
            let next = TerminalColors {
                foreground: reported.foreground.or(previous.as_ref().and_then(|p| p.foreground)),
                background: reported.background.or(previous.as_ref().and_then(|p| p.background)),
                palette: reported.palette.clone().or_else(|| previous.as_ref().and_then(|p| p.palette.clone())),
            };
            // Re-rendering rebuilds every component, so skip it when nothing changed (including timeouts).
            if let Some(previous) = &previous
                && same_terminal_colors(previous, &next)
            {
                return;
            }
            s.terminal_colors = Some(next.clone());
            next
        };
        set_terminal_colors(Arc::new(next));
        self.reapply_for_terminal();
        let ui = self.ui();
        ui.invalidate();
        self.request_render(&ui);
    }

    /// Re-apply the setting after the terminal's colors or appearance changed.
    fn reapply_for_terminal(&self) {
        let active = self.state.lock().active_theme_name.clone();
        if active.as_deref() == Some("<in-memory>") {
            return;
        }
        let theme_name = self.resolve_theme_name();
        if theme_name == SYSTEM_THEME_NAME || Some(theme_name.as_str()) != active.as_deref() {
            self.apply_theme_name(&theme_name, false);
        }
    }

    fn set_auto_sync(&self, enabled: bool) {
        {
            let mut s = self.state.lock();
            if s.auto_sync_enabled == enabled {
                return;
            }
            s.auto_sync_enabled = enabled;
        }
        self.ui().set_terminal_color_scheme_notifications(enabled);
    }

    fn bind_terminal_color_scheme_listener(&self) {
        let weak = self.weak_self.clone();
        let id = self.ui().on_terminal_color_scheme_change(Arc::new(move |scheme: TerminalColorScheme| {
            if let Some(this) = weak.upgrade() {
                let theme = match scheme {
                    TerminalColorScheme::Dark => TerminalTheme::Dark,
                    TerminalColorScheme::Light => TerminalTheme::Light,
                };
                this.apply_terminal_color_scheme_change(theme);
            }
        }));
        self.state.lock().terminal_color_scheme_listener = Some(id);
    }

    /// The terminal reported a light/dark switch. Its colors changed too, so query them again.
    fn apply_terminal_color_scheme_change(&self, terminal_theme: TerminalTheme) {
        if !self.state.lock().auto_sync_enabled {
            return;
        }
        let previous = get_terminal_theme();
        set_terminal_color_scheme(Some(&terminal_theme));
        if get_terminal_theme() != previous {
            self.reapply_for_terminal();
        }
        self.query_terminal_colors();
    }

    fn notify_changed(&self) {
        self.ui().invalidate();
        (self.on_changed)();
    }
}

/// Poll a future to completion in the background: on the tokio runtime when one is running, otherwise
/// on the pi-tui event loop.
pub(crate) fn spawn_detached(future: impl std::future::Future<Output = ()> + Send + 'static) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        crate::utils::uncaught::spawn_on(&handle, future);
    } else {
        pi_tui::event_loop::current().spawn_future(future);
    }
}
