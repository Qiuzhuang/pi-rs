//! Port of `packages/coding-agent/src/modes/interactive/components/login-dialog.ts`.

use crate::modes::interactive::components::auth_url::AuthUrlComponent;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::key_hint;
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use crate::utils::open_browser::open_browser;
use parking_lot::Mutex;
use pi_ai::auth::extension_oauth_types::OAuthDeviceCodeInfo;
use pi_ai::auth::types::AuthInfoLink;
use pi_tui::components::input::Input;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult, Tui, same_component};
use std::sync::{Arc, Weak};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

type InputResolver = oneshot::Sender<anyhow::Result<String>>;

struct Inner {
    container: Container,
    content_container: Arc<Container>,
    input: Arc<Input>,
    tui: Arc<Tui>,
    /// The shown sign-in URL, which `app.message.copy` copies.
    auth_url: Mutex<Option<Arc<AuthUrlComponent>>>,
    abort_controller: CancellationToken,
    input_resolver: Mutex<Option<InputResolver>>,
    on_complete: Arc<dyn Fn(bool, Option<String>) + Send + Sync>,
    focused: Mutex<bool>,
}

impl Inner {
    fn replace_input_with_submitted_text(&self, value: &str) {
        let input: ComponentRef = self.input.clone();
        let children = self
            .content_container
            .children()
            .into_iter()
            .map(|child| {
                if same_component(&child, &input) {
                    Arc::new(Text::new(format!("> {value}"), 0, 0, None)) as ComponentRef
                } else {
                    child
                }
            })
            .collect();
        self.content_container.set_children(children);
    }

    fn submit(&self) {
        let resolver = self.input_resolver.lock().take();
        if let Some(resolver) = resolver {
            let value = self.input.get_value();
            self.replace_input_with_submitted_text(&value);
            let _ = resolver.send(Ok(value));
        }
    }

    fn cancel(&self) {
        self.abort_controller.cancel();
        let resolver = self.input_resolver.lock().take();
        if let Some(resolver) = resolver {
            let _ = resolver.send(Err(anyhow::anyhow!("Login cancelled")));
        }
        (self.on_complete)(false, Some("Login cancelled".to_string()));
    }
}

/// Login dialog component - replaces editor during OAuth login flow
/// TS: `extends Container implements Focusable`
pub struct LoginDialogComponent {
    inner: Arc<Inner>,
}

impl LoginDialogComponent {
    pub fn new(
        tui: Arc<Tui>,
        provider_id: &str,
        on_complete: Arc<dyn Fn(bool, Option<String>) + Send + Sync>,
        provider_name_override: Option<&str>,
        title_override: Option<&str>,
    ) -> Self {
        let provider_name = provider_name_override.filter(|s| !s.is_empty()).unwrap_or(provider_id);
        let title = title_override.map(str::to_string).unwrap_or_else(|| format!("Login to {provider_name}"));

        let inner = Arc::new_cyclic(|weak: &Weak<Inner>| {
            let t = theme();
            let container = Container::new();
            // Top border
            container.add_child(Arc::new(DynamicBorder::new(None)));
            // Title
            container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Accent, &t.bold(&title)), 1, 0, None)));
            // Dynamic content area
            let content_container = Arc::new(Container::new());
            container.add_child(content_container.clone());

            // Input (always present, used when needed)
            let input = Arc::new(Input::default());
            let w = weak.clone();
            input.set_on_submit(Some(Arc::new(move |_v: String| {
                if let Some(inner) = w.upgrade() {
                    inner.submit();
                }
            })));
            let w = weak.clone();
            input.set_on_escape(Some(Arc::new(move || {
                if let Some(inner) = w.upgrade() {
                    inner.cancel();
                }
            })));

            // Bottom border
            container.add_child(Arc::new(DynamicBorder::new(None)));

            Inner {
                container,
                content_container,
                input,
                tui,
                auth_url: Mutex::new(None),
                abort_controller: CancellationToken::new(),
                input_resolver: Mutex::new(None),
                on_complete,
                focused: Mutex::new(false),
            }
        });
        Self { inner }
    }

    pub fn signal(&self) -> CancellationToken {
        self.inner.abort_controller.clone()
    }

    fn content(&self) -> &Container {
        &self.inner.content_container
    }

    fn request_render(&self) {
        self.inner.tui.request_render(false);
    }

    fn wait_for_input(&self) -> oneshot::Receiver<anyhow::Result<String>> {
        let (tx, rx) = oneshot::channel();
        // A previous pending prompt is superseded (TS overwrites the resolver).
        *self.inner.input_resolver.lock() = Some(tx);
        rx
    }

    /// Called by onAuth callback - show URL and optional instructions
    pub fn show_auth(&self, url: &str, instructions: Option<&str>) {
        let c = self.content();
        c.clear();
        c.add_child(Arc::new(Spacer::new(1)));
        let auth_url = Arc::new(AuthUrlComponent::new(self.inner.tui.clone(), url));
        *self.inner.auth_url.lock() = Some(auth_url.clone());
        c.add_child(auth_url);

        if let Some(instructions) = instructions.filter(|s| !s.is_empty()) {
            c.add_child(Arc::new(Spacer::new(1)));
            c.add_child(Arc::new(Text::new(theme().fg(&ThemeColor::Warning, instructions), 1, 0, None)));
        }

        open_browser(url);
        self.request_render();
    }

    /// Called by onDeviceCode callback - show URL and user code.
    pub fn show_device_code(&self, info: OAuthDeviceCodeInfo) {
        *self.inner.auth_url.lock() = None;
        let t = theme();
        let c = self.content();
        c.clear();
        c.add_child(Arc::new(Spacer::new(1)));
        let uri = &info.verification_uri;
        let linked_url = format!("\x1b]8;;{uri}\x07{uri}\x1b]8;;\x07");
        c.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Accent, &linked_url), 1, 0, None)));

        let click_hint = if cfg!(target_os = "macos") { "Cmd+click to open" } else { "Ctrl+click to open" };
        let hyperlink = format!("\x1b]8;;{uri}\x07{click_hint}\x1b]8;;\x07");
        c.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Dim, &hyperlink), 1, 0, None)));
        c.add_child(Arc::new(Spacer::new(1)));
        c.add_child(Arc::new(Text::new(
            t.fg(&ThemeColor::Warning, &format!("Enter code: {}", info.user_code)),
            1,
            0,
            None,
        )));

        self.request_render();
    }

    /// Show input for manual code/URL entry (for callback server providers)
    pub async fn show_manual_input(&self, prompt: &str) -> anyhow::Result<String> {
        self.inner.input.set_value("");
        let c = self.content();
        c.add_child(Arc::new(Spacer::new(1)));
        c.add_child(Arc::new(Text::new(theme().fg(&ThemeColor::Dim, prompt), 1, 0, None)));
        c.add_child(self.inner.input.clone());
        c.add_child(Arc::new(Text::new(
            format!("({})", key_hint("tui.select.cancel".into(), "to cancel")),
            1,
            0,
            None,
        )));
        self.request_render();

        let rx = self.wait_for_input();
        rx.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Login cancelled")))
    }

    /// Called by onPrompt callback - show prompt and wait for input
    /// Note: Does NOT clear content, appends to existing (preserves URL from showAuth)
    pub async fn show_prompt(&self, message: &str, placeholder: Option<&str>) -> anyhow::Result<String> {
        let t = theme();
        let c = self.content();
        c.add_child(Arc::new(Spacer::new(1)));
        c.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Text, message), 1, 0, None)));
        if let Some(placeholder) = placeholder.filter(|s| !s.is_empty()) {
            c.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Dim, &format!("e.g., {placeholder}")), 1, 0, None)));
        }
        c.add_child(self.inner.input.clone());
        c.add_child(Arc::new(Text::new(
            format!(
                "({} {})",
                key_hint("tui.select.cancel".into(), "to cancel,"),
                key_hint("tui.select.confirm".into(), "to submit")
            ),
            1,
            0,
            None,
        )));

        self.inner.input.set_value("");
        self.request_render();

        let rx = self.wait_for_input();
        rx.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Login cancelled")))
    }

    /// Show informational text before another login step.
    pub fn show_details(&self, lines: &[String]) {
        *self.inner.auth_url.lock() = None;
        let c = self.content();
        c.clear();
        c.add_child(Arc::new(Spacer::new(1)));
        for line in lines {
            c.add_child(Arc::new(Text::new(line.clone(), 1, 0, None)));
        }
        self.request_render();
    }

    /// Show provider-owned information and links without starting an auth callback flow.
    pub fn show_info(&self, message: &str, links: Option<&[AuthInfoLink]>, show_close_hint: Option<bool>) {
        let t = theme();
        let c = self.content();
        c.add_child(Arc::new(Spacer::new(1)));
        c.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Text, message), 1, 0, None)));
        for link in links.unwrap_or(&[]) {
            let text = match link.label.as_deref().filter(|l| !l.is_empty()) {
                Some(label) => format!("{label}: {}", link.url),
                None => link.url.clone(),
            };
            let hyperlink = format!("\x1b]8;;{}\x07{text}\x1b]8;;\x07", link.url);
            c.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Accent, &hyperlink), 1, 0, None)));
        }
        if show_close_hint.unwrap_or(false) {
            c.add_child(Arc::new(Spacer::new(1)));
            c.add_child(Arc::new(Text::new(
                format!("({})", key_hint("tui.select.cancel".into(), "to close")),
                1,
                0,
                None,
            )));
        }
        self.request_render();
    }

    /// Show waiting message (for polling flows like GitHub Copilot)
    pub fn show_waiting(&self, message: &str) {
        let c = self.content();
        c.add_child(Arc::new(Spacer::new(1)));
        c.add_child(Arc::new(Text::new(theme().fg(&ThemeColor::Dim, message), 1, 0, None)));
        c.add_child(Arc::new(Text::new(
            format!("({})", key_hint("tui.select.cancel".into(), "to cancel")),
            1,
            0,
            None,
        )));
        self.request_render();
    }

    /// Called by onProgress callback
    pub fn show_progress(&self, message: &str) {
        self.content().add_child(Arc::new(Text::new(theme().fg(&ThemeColor::Dim, message), 1, 0, None)));
        self.request_render();
    }
}

impl Component for LoginDialogComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.inner.container.render(width)
    }

    fn handle_input(&self, data: &str) {
        let kb = get_keybindings();
        if kb.matches(data, "tui.select.cancel") {
            self.inner.cancel();
            return;
        }
        let auth_url = self.inner.auth_url.lock().clone();
        if let Some(auth_url) = auth_url {
            if kb.matches(data, "app.message.copy") {
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.spawn(async move {
                        let _ = auth_url.copy().await;
                    });
                }
                return;
            }
        }

        // Pass to input
        self.inner.input.handle_input(data);
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        self.inner.container.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.inner.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.inner.container.child_components()
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        *self.inner.focused.lock() = focused;
        self.inner.input.set_focused(focused);
    }

    fn focused(&self) -> bool {
        *self.inner.focused.lock()
    }
}
