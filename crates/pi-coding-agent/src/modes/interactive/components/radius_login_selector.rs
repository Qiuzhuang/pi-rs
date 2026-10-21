//! Port of `packages/coding-agent/src/modes/interactive/components/radius-login-selector.ts`.
//!
//! The `/login` menu with the animated "Sign in with Radius" option. Internal to the interactive mode: the shimmer
//! is Radius-only and is not exposed to other selectors.

use crate::modes::interactive::components::extension_selector::ExtensionSelectorComponent;
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use pi_tui::colors::{Color, ColorMixSpace, foreground_ansi, mix_colors, parse_color};
use pi_tui::components::text::Text;
use pi_tui::tui::{Component, ComponentRef, MouseEvent, MouseResult};
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

/// The four colors of the Radius logo, in the order they stream across the text.
static RADIUS_COLORS: LazyLock<[Color; 4]> = LazyLock::new(|| {
    ["#4d9abf", "#83ccd2", "#f1be57", "#f09082"].map(|hex| parse_color(hex).expect("valid radius color"))
});
/// Width of each color band, in characters.
const CHARS_PER_COLOR: f64 = 4.0;
const CHARS_PER_SECOND: f64 = 10.0;
const ANIMATION_FRAME_MS: u64 = 50;

/// Color `text` with the Radius logo colors flowing left to right; `elapsed_ms` is the animation time.
fn radius_shimmer(text: &str, elapsed_ms: f64) -> String {
    let mode = theme().get_color_mode();
    let colors = &*RADIUS_COLORS;
    let cycle = colors.len() as f64 * CHARS_PER_COLOR;
    let offset = (elapsed_ms / 1000.0) * CHARS_PER_SECOND;
    let mut result = String::new();
    for (index, ch) in text.chars().enumerate() {
        let position = (((index as f64 - offset) % cycle) + cycle) % cycle;
        let band = (position / CHARS_PER_COLOR).floor();
        let t = position / CHARS_PER_COLOR - band;
        // Smoothstep keeps each band recognizable while still blending into the next one.
        let amount = (t * t * (3.0 - 2.0 * t)).clamp(0.0, 1.0);
        let band = (band as usize) % colors.len();
        let from = &colors[band];
        let to = &colors[(band + 1) % colors.len()];
        let mixed = mix_colors(from, to, amount, ColorMixSpace::Srgb).unwrap_or(*from);
        result.push_str(&foreground_ansi(&mixed, mode));
        result.push(ch);
    }
    format!("{result}\x1b[39m")
}

/// The "Sign in with Radius" option: `label` is the full option, starting with the animated `text`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadiusOption {
    pub label: String,
    pub text: String,
}

/// Swaps the line `ExtensionSelectorComponent` draws for the selected Radius option with the animated one. When the
/// selector's row style changes or the label wraps, the line no longer matches and the option renders normally.
/// TS: `extends ExtensionSelectorComponent`
pub struct RadiusLoginMenuComponent {
    base: ExtensionSelectorComponent,
    radius_option: RadiusOption,
    animation_start: Instant,
    animating: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
}

impl RadiusLoginMenuComponent {
    pub fn new(
        tui: Arc<Tui>,
        title: &str,
        options: &[String],
        radius_option: &RadiusOption,
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let animating = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));

        let stop = {
            let animating = animating.clone();
            let stopped = stopped.clone();
            move || {
                stopped.store(true, Ordering::SeqCst);
                animating.store(false, Ordering::SeqCst);
            }
        };
        let stop_select = stop.clone();
        let base = ExtensionSelectorComponent::new(
            title,
            options,
            Arc::new(move |option: String| {
                stop_select();
                on_select(option);
            }),
            Arc::new(move || {
                stop();
                on_cancel();
            }),
            None,
        );

        // Animation timer (TS: setInterval(..., ANIMATION_FRAME_MS).unref()).
        {
            let animating = animating.clone();
            let stopped = stopped.clone();
            let tui = Arc::downgrade(&tui);
            std::thread::Builder::new()
                .name("radius-login-shimmer".into())
                .spawn(move || {
                    loop {
                        std::thread::sleep(Duration::from_millis(ANIMATION_FRAME_MS));
                        if stopped.load(Ordering::SeqCst) {
                            break;
                        }
                        let Some(tui) = tui.upgrade() else { break };
                        if animating.load(Ordering::SeqCst) {
                            tui.request_render(false);
                        }
                    }
                })
                .ok();
        }

        Self {
            base,
            radius_option: radius_option.clone(),
            animation_start: Instant::now(),
            animating,
            stopped,
        }
    }

    fn stop_animation(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.animating.store(false, Ordering::SeqCst);
    }

    pub fn dispose(&self) {
        self.stop_animation();
        self.base.dispose();
    }

    /// The underlying extension selector.
    pub fn base(&self) -> &ExtensionSelectorComponent {
        &self.base
    }
}

impl Drop for RadiusLoginMenuComponent {
    fn drop(&mut self) {
        self.stop_animation();
    }
}

impl Component for RadiusLoginMenuComponent {
    fn render(&self, width: usize) -> Vec<String> {
        let mut lines = self.base.render(width);
        let RadiusOption { label, text } = &self.radius_option;
        let t = theme();
        let selected_line = Text::new(
            format!("{}{}", t.fg(&ThemeColor::Accent, "→ "), t.fg(&ThemeColor::Accent, label)),
            1,
            0,
            None,
        )
        .render(width)
        .into_iter()
        .next();
        let index = selected_line.and_then(|selected| lines.iter().position(|l| *l == selected));
        let animating = index.is_some() && !self.stopped.load(Ordering::SeqCst);
        self.animating.store(animating, Ordering::SeqCst);
        if let Some(index) = index {
            let elapsed = self.animation_start.elapsed().as_secs_f64() * 1000.0;
            let shimmer = radius_shimmer(text, elapsed);
            let rest: String = label.chars().skip(text.chars().count()).collect();
            let animated_line = format!("{}{shimmer}{rest}", t.fg(&ThemeColor::Accent, "→ "));
            lines[index] = Text::new(animated_line, 1, 0, None).render(width).into_iter().next().unwrap_or_default();
        }
        lines
    }

    fn handle_input(&self, data: &str) {
        self.base.handle_input(data);
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        self.base.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.base.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.base.child_components()
    }

    fn is_focusable(&self) -> bool {
        self.base.is_focusable()
    }

    fn set_focused(&self, focused: bool) {
        self.base.set_focused(focused);
    }

    fn focused(&self) -> bool {
        self.base.focused()
    }
}

/// Top-level `/login` selector whose Radius option shimmers in the Radius logo colors while it is selected.
///
/// Rust: returns the concrete menu component (TS returns it typed as its `ExtensionSelectorComponent` base).
pub fn create_login_menu_selector(
    tui: Arc<Tui>,
    title: &str,
    options: &[String],
    radius_option: &RadiusOption,
    on_select: Arc<dyn Fn(String) + Send + Sync>,
    on_cancel: Arc<dyn Fn() + Send + Sync>,
) -> Arc<RadiusLoginMenuComponent> {
    Arc::new(RadiusLoginMenuComponent::new(tui, title, options, radius_option, on_select, on_cancel))
}
