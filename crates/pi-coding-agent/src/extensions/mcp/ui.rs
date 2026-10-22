//! Port of `packages/coding-agent/src/extensions/mcp/ui.ts`.
//!
//! The `/mcp` manager view: menus that rebuild while servers connect, a read-only status screen, and
//! the sign-in screen that accepts a pasted redirect URL.

use crate::core::extensions::types::{
    CustomComponentFactory, CustomUiOptions, ExtensionCommandContext, NotifyType,
};
use crate::core::keybindings::KeybindingsManager;
use crate::modes::interactive::components::auth_url::AuthUrlComponent;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::key_hint;
use crate::modes::interactive::theme::theme::{Theme, ThemeColor, get_select_list_theme};
use futures::future::BoxFuture;
use parking_lot::Mutex;
use pi_tui::components::input::Input;
use pi_tui::components::select_list::{SelectItem, SelectList, SelectListLayoutOptions};
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::tui::{Component, ComponentRef, Container, Tui};
use pi_tui::utils::{truncate_to_width, visible_width};
use std::sync::{Arc, Weak};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct McpMenu {
    pub title: String,
    /// Shown below the title.
    pub details: Option<String>,
    /// Shown below the details in the error color.
    pub error: Option<String>,
    pub items: Vec<SelectItem>,
    /// Shown when there are no items.
    pub empty: Option<String>,
    /// Value of the item selected when the menu opens.
    pub selected: Option<String>,
    /// What the confirm key does, for the key hint.
    pub confirm_label: String,
    /// What the cancel key does, for the key hint.
    pub cancel_label: String,
}

/// Rebuild listener of [`McpUi::menu`]: registers a listener and returns its unsubscribe function.
pub type McpMenuSubscribe =
    Arc<dyn Fn(Arc<dyn Fn() + Send + Sync>) -> Arc<dyn Fn() + Send + Sync> + Send + Sync>;

#[async_trait::async_trait]
pub trait McpUi: Send + Sync {
    /// Show a menu and resolve to the chosen item's value, or undefined when cancelled. `subscribe`
    /// rebuilds the menu on every change, keeping the selected item.
    async fn menu(
        &self,
        build: Arc<dyn Fn() -> McpMenu + Send + Sync>,
        subscribe: Option<McpMenuSubscribe>,
    ) -> anyhow::Result<Option<String>>;
    /// Show a message while an operation runs. With `onCancel`, the cancel key calls it.
    fn status(&self, title: &str, message: &str, on_cancel: Option<Arc<dyn Fn() + Send + Sync>>);
    /// Show the authorization URL and wait for a pasted redirect URL. Resolves to undefined when
    /// cancelled or when `signal` aborts (the browser reached the callback).
    async fn redirect_url(
        &self,
        title: &str,
        authorization_url: &str,
        signal: CancellationToken,
    ) -> anyhow::Result<Option<String>>;
}

type InputHandler = Arc<dyn Fn(&str) + Send + Sync>;

fn matches_key(data: &str, keybinding: &str) -> bool {
    pi_tui::keybindings::get_keybindings().matches(data, keybinding)
}

fn frame(theme: &Arc<Theme>, title: &str, body: Vec<ComponentRef>, footer: Option<&str>) -> Arc<Container> {
    let container = Arc::new(Container::new());
    let border = |theme: &Arc<Theme>| {
        let theme = theme.clone();
        Arc::new(DynamicBorder::new(Some(Arc::new(move |text: String| {
            theme.fg(&ThemeColor::Accent, &text)
        })))) as ComponentRef
    };
    container.add_child(border(theme));
    container.add_child(Arc::new(Text::new(
        theme.fg(&ThemeColor::Accent, &theme.bold(title)),
        1,
        0,
        None,
    )));
    for child in body {
        container.add_child(child);
    }
    if let Some(footer) = footer.filter(|footer| !footer.is_empty()) {
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(Text::new(
            theme.fg(&ThemeColor::Dim, footer),
            1,
            0,
            None,
        )));
    }
    container.add_child(border(theme));
    container
}

const MAX_VISIBLE_ITEMS: usize = 12;

struct ViewState {
    content: Arc<Container>,
    input_handler: Option<InputHandler>,
    input_target: Option<ComponentRef>,
    focused: bool,
}

/// TS: `implements McpUi, Component, Focusable`
pub struct McpManagerView {
    this: Weak<McpManagerView>,
    tui: Option<Arc<Tui>>,
    theme: Arc<Theme>,
    #[allow(dead_code)]
    keybindings: Arc<KeybindingsManager>,
    state: Mutex<ViewState>,
}

impl McpManagerView {
    pub fn new(tui: Arc<Tui>, theme: Arc<Theme>, keybindings: Arc<KeybindingsManager>) -> Arc<Self> {
        Self::create(Some(tui), theme, keybindings)
    }

    /// A view without a TUI to request renders from, for tests.
    pub fn create(
        tui: Option<Arc<Tui>>,
        theme: Arc<Theme>,
        keybindings: Arc<KeybindingsManager>,
    ) -> Arc<Self> {
        let content = frame(
            &theme,
            "MCP servers",
            vec![Arc::new(Text::new(
                theme.fg(&ThemeColor::Muted, "Loading…"),
                1,
                1,
                None,
            ))],
            None,
        );
        Arc::new_cyclic(|this| Self {
            this: this.clone(),
            tui,
            theme,
            keybindings,
            state: Mutex::new(ViewState {
                content,
                input_handler: None,
                input_target: None,
                focused: false,
            }),
        })
    }

    fn request_render(&self) {
        if let Some(tui) = &self.tui {
            tui.request_render(false);
        }
    }

    fn set_content(
        &self,
        content: Arc<Container>,
        input_handler: Option<InputHandler>,
        input_target: Option<ComponentRef>,
    ) {
        let (previous, next, focused) = {
            let mut state = self.state.lock();
            let previous = state.input_target.take();
            state.content = content;
            state.input_handler = input_handler;
            state.input_target = input_target.clone();
            (previous, input_target, state.focused)
        };
        if let Some(previous) = previous {
            previous.set_focused(false);
        }
        if let Some(next) = next {
            next.set_focused(focused);
        }
        self.request_render();
    }

    /// Build and show one state of a menu. Returns the selected value it shows.
    fn render_menu(
        &self,
        menu: McpMenu,
        selected: &Arc<Mutex<Option<String>>>,
        finish: &Arc<dyn Fn(Option<String>) + Send + Sync>,
    ) {
        let theme = &self.theme;
        let wanted = selected.lock().clone().or_else(|| menu.selected.clone());
        let mut body: Vec<ComponentRef> = Vec::new();
        if let Some(details) = menu.details.as_deref().filter(|details| !details.is_empty()) {
            body.push(Arc::new(Text::new(theme.fg(&ThemeColor::Muted, details), 1, 0, None)));
        }
        if let Some(error) = menu.error.as_deref().filter(|error| !error.is_empty()) {
            body.push(Arc::new(Text::new(theme.fg(&ThemeColor::Error, error), 1, 0, None)));
        }
        body.push(Arc::new(Spacer::new(1)));
        let footer = format!(
            "{} • {}",
            key_hint("tui.select.confirm".to_string(), &menu.confirm_label),
            key_hint("tui.select.cancel".to_string(), &menu.cancel_label)
        );
        if menu.items.is_empty() {
            body.push(Arc::new(Text::new(
                theme.fg(
                    &ThemeColor::Muted,
                    menu.empty.as_deref().unwrap_or("Nothing to show."),
                ),
                1,
                0,
                None,
            )));
            let finish = finish.clone();
            self.set_content(
                frame(
                    theme,
                    &menu.title,
                    body,
                    Some(&key_hint("tui.select.cancel".to_string(), &menu.cancel_label)),
                ),
                Some(Arc::new(move |data: &str| {
                    if matches_key(data, "tui.select.cancel") {
                        finish(None);
                    }
                })),
                None,
            );
            return;
        }
        let list = Arc::new(SelectList::new(
            menu.items.clone(),
            menu.items.len().min(MAX_VISIBLE_ITEMS),
            (*get_select_list_theme()).clone(),
            SelectListLayoutOptions::default(),
        ));
        if let Some(index) = menu
            .items
            .iter()
            .position(|item| Some(&item.value) == wanted.as_ref())
        {
            list.set_selected_index(index);
        }
        *selected.lock() = list.get_selected_item().map(|item| item.value);
        {
            let selected = selected.clone();
            list.set_on_selection_change(Some(Arc::new(move |item: SelectItem| {
                *selected.lock() = Some(item.value);
            })));
        }
        {
            let finish = finish.clone();
            list.set_on_select(Some(Arc::new(move |item: SelectItem| finish(Some(item.value)))));
        }
        {
            let finish = finish.clone();
            list.set_on_cancel(Some(Arc::new(move || finish(None))));
        }
        body.push(list.clone());
        let handler_list = list.clone();
        self.set_content(
            frame(theme, &menu.title, body, Some(&footer)),
            Some(Arc::new(move |data: &str| handler_list.handle_input(data))),
            None,
        );
    }
}

#[async_trait::async_trait]
impl McpUi for McpManagerView {
    async fn menu(
        &self,
        build: Arc<dyn Fn() -> McpMenu + Send + Sync>,
        subscribe: Option<McpMenuSubscribe>,
    ) -> anyhow::Result<Option<String>> {
        let (sender, receiver) = oneshot::channel::<Option<String>>();
        let sender = Arc::new(Mutex::new(Some(sender)));
        let unsubscribe: Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>> = Arc::default();
        let settled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finish: Arc<dyn Fn(Option<String>) + Send + Sync> = {
            let (sender, unsubscribe, settled) = (sender.clone(), unsubscribe.clone(), settled.clone());
            Arc::new(move |value| {
                if settled.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let unsubscribe = unsubscribe.lock().take();
                if let Some(unsubscribe) = unsubscribe {
                    unsubscribe();
                }
                if let Some(sender) = sender.lock().take() {
                    let _ = sender.send(value);
                }
            })
        };
        let selected: Arc<Mutex<Option<String>>> = Arc::default();
        self.render_menu(build(), &selected, &finish);
        if let Some(subscribe) = subscribe {
            let this = self.this.clone();
            let (build, selected, finish, is_settled) =
                (build.clone(), selected.clone(), finish.clone(), settled.clone());
            let listener: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if is_settled.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                if let Some(view) = this.upgrade() {
                    view.render_menu(build(), &selected, &finish);
                }
            });
            let remove = subscribe(listener);
            if settled.load(std::sync::atomic::Ordering::SeqCst) {
                remove();
            } else {
                *unsubscribe.lock() = Some(remove);
            }
        }
        Ok(receiver.await.unwrap_or(None))
    }

    fn status(&self, title: &str, message: &str, on_cancel: Option<Arc<dyn Fn() + Send + Sync>>) {
        let body: Vec<ComponentRef> = vec![
            Arc::new(Spacer::new(1)),
            Arc::new(Text::new(self.theme.fg(&ThemeColor::Muted, message), 1, 0, None)),
        ];
        let Some(on_cancel) = on_cancel else {
            self.set_content(frame(&self.theme, title, body, None), None, None);
            return;
        };
        self.set_content(
            frame(
                &self.theme,
                title,
                body,
                Some(&key_hint("tui.select.cancel".to_string(), "cancel")),
            ),
            Some(Arc::new(move |data: &str| {
                if matches_key(data, "tui.select.cancel") {
                    on_cancel();
                }
            })),
            None,
        );
    }

    async fn redirect_url(
        &self,
        title: &str,
        authorization_url: &str,
        signal: CancellationToken,
    ) -> anyhow::Result<Option<String>> {
        if signal.is_cancelled() {
            return Ok(None);
        }
        let (sender, receiver) = oneshot::channel::<Option<String>>();
        let sender = Arc::new(Mutex::new(Some(sender)));
        let finish = {
            let sender = sender.clone();
            move |value: Option<String>| {
                if let Some(sender) = sender.lock().take() {
                    let _ = sender.send(value);
                }
            }
        };
        let input = Arc::new(Input::default());
        let link: Option<Arc<AuthUrlComponent>> = self
            .tui
            .as_ref()
            .map(|tui| Arc::new(AuthUrlComponent::new(tui.clone(), authorization_url)));
        let link_component: ComponentRef = match &link {
            Some(link) => link.clone(),
            None => Arc::new(Text::new(authorization_url, 1, 0, None)),
        };
        let theme = &self.theme;
        let body: Vec<ComponentRef> = vec![
            Arc::new(Spacer::new(1)),
            Arc::new(Text::new(
                theme.fg(
                    &ThemeColor::Muted,
                    "Approve access in your browser. If it did not open, visit:",
                ),
                1,
                0,
                None,
            )),
            link_component,
            Arc::new(Spacer::new(1)),
            Arc::new(Text::new(
                theme.fg(
                    &ThemeColor::Muted,
                    "If the browser runs on another machine, paste the URL it was redirected to:",
                ),
                1,
                0,
                None,
            )),
            input.clone(),
        ];
        let handler: InputHandler = {
            let input = input.clone();
            let finish = finish.clone();
            Arc::new(move |data: &str| {
                if matches_key(data, "tui.select.confirm") {
                    let value = input.get_value().trim().to_string();
                    if !value.is_empty() {
                        finish(Some(value));
                    }
                    return;
                }
                if matches_key(data, "tui.select.cancel") {
                    finish(None);
                    return;
                }
                if matches_key(data, "app.message.copy") {
                    if let Some(link) = link.clone() {
                        crate::utils::uncaught::spawn(async move {
                            let _ = link.copy().await;
                        });
                    }
                    return;
                }
                input.handle_input(data);
            })
        };
        self.set_content(
            frame(
                theme,
                title,
                body,
                Some(&format!(
                    "{} • {}",
                    key_hint("tui.select.confirm".to_string(), "submit"),
                    key_hint("tui.select.cancel".to_string(), "cancel")
                )),
            ),
            Some(handler),
            Some(input.clone()),
        );
        tokio::select! {
            value = receiver => Ok(value.unwrap_or(None)),
            _ = signal.cancelled() => {
                sender.lock().take();
                Ok(None)
            }
        }
    }
}

impl Component for McpManagerView {
    fn render(&self, width: usize) -> Vec<String> {
        let content = self.state.lock().content.clone();
        content
            .render(width)
            .into_iter()
            .map(|line| {
                if visible_width(&line) > width {
                    truncate_to_width(&line, width, "", false)
                } else {
                    line
                }
            })
            .collect()
    }

    fn handle_input(&self, data: &str) {
        let handler = self.state.lock().input_handler.clone();
        if let Some(handler) = handler {
            handler(data);
        }
        self.request_render();
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        let content = self.state.lock().content.clone();
        content.invalidate();
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        let target = {
            let mut state = self.state.lock();
            state.focused = focused;
            state.input_target.clone()
        };
        if let Some(target) = target {
            target.set_focused(focused);
        }
    }

    fn focused(&self) -> bool {
        self.state.lock().focused
    }
}

/// Run `manage` in the manager view until it returns.
pub async fn show_mcp_manager(
    ctx: &ExtensionCommandContext,
    manage: Arc<dyn Fn(Arc<dyn McpUi>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
) -> anyhow::Result<()> {
    let ui = ctx.ui()?;
    let notify_ui = ui.clone();
    let factory: CustomComponentFactory = Arc::new(move |tui, theme, keybindings, done| {
        let view = McpManagerView::new(tui, theme, keybindings);
        let manage = manage.clone();
        let notify_ui = notify_ui.clone();
        let managed: Arc<dyn McpUi> = view.clone();
        crate::utils::uncaught::spawn(async move {
            if let Err(error) = manage(managed).await {
                notify_ui.notify(&error.to_string(), NotifyType::Error);
            }
            done(Box::new(()));
        });
        Box::pin(async move { Ok(view as ComponentRef) })
    });
    ui.custom(factory, CustomUiOptions::default()).await?;
    Ok(())
}
