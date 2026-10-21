//! Port of `packages/coding-agent/src/modes/interactive/components/auth-url.ts`.

use crate::modes::interactive::components::keybinding_hints::key_hint;
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use crate::utils::clipboard::copy_to_clipboard;
use pi_tui::components::text::Text;
use pi_tui::terminal_image::hyperlink;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult, Tui};
use std::sync::{Arc, Weak};

/// A sign-in URL with a click hint and a copy hint. Hosts call `copy()` when `app.message.copy` is
/// pressed, since a long URL wraps and often cannot be selected or clicked as a whole (SSH, tmux).
/// TS: `extends Container`
pub struct AuthUrlComponent {
    container: Container,
    url: String,
    tui: Weak<Tui>,
    hint: Arc<Text>,
}

impl AuthUrlComponent {
    pub fn new(tui: Arc<Tui>, url: &str) -> Self {
        let container = Container::new();
        let t = theme();
        container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Accent, &hyperlink(url, url)), 1, 0, None)));
        let hint = Arc::new(Text::new("", 1, 0, None));
        container.add_child(hint.clone() as ComponentRef);
        let component = Self { container, url: url.to_string(), tui: Arc::downgrade(&tui), hint };
        component.set_hint(&key_hint("app.message.copy".to_string(), "to copy"));
        component
    }

    pub fn url(&self) -> String {
        self.url.clone()
    }

    fn set_hint(&self, suffix: &str) {
        let t = theme();
        let click_hint = if cfg!(target_os = "macos") { "Cmd+click to open" } else { "Ctrl+click to open" };
        self.hint.set_text(format!(
            "{} {} {suffix}",
            t.fg(&ThemeColor::Dim, &hyperlink(click_hint, &self.url)),
            t.fg(&ThemeColor::Dim, "•")
        ));
        if let Some(tui) = self.tui.upgrade() {
            tui.request_render(false);
        }
    }

    pub async fn copy(&self) -> anyhow::Result<()> {
        match copy_to_clipboard(&self.url).await {
            Ok(()) => self.set_hint(&theme().fg(&ThemeColor::Success, "Copied URL to clipboard")),
            Err(error) => self.set_hint(&theme().fg(&ThemeColor::Error, &error.to_string())),
        }
        Ok(())
    }
}

impl Component for AuthUrlComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn invalidate(&self) {
        self.container.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.container.child_components()
    }

    fn uses_container_mouse(&self) -> bool {
        true
    }
}
