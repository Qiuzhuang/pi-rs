//! Port of `packages/coding-agent/src/modes/interactive/components/oauth-selector.ts`.

use pi_ai::auth::types::ApiKeyAuth;
use pi_ai::auth::types::AuthCheck;
use pi_ai::auth::types::OAuthAuth;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use parking_lot::Mutex;
use pi_ai::auth::types::AuthType;
use pi_tui::components::input::Input;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::truncated_text::TruncatedText;
use pi_tui::fuzzy::fuzzy_filter;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use regex::Regex;
use std::sync::{LazyLock, Weak};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthSelectorProviderAuthType {
    #[serde(rename = "oauth")]
    Oauth,
    #[serde(rename = "api_key")]
    ApiKey,
}

impl AuthSelectorProviderAuthType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Oauth => "oauth",
            Self::ApiKey => "api_key",
        }
    }
}

#[derive(Clone)]
pub enum AuthSelectorProviderMethod {
    Arc(Arc<dyn ApiKeyAuth>),
    Arc2(Arc<dyn OAuthAuth>),
}

#[derive(Clone)]
pub struct AuthSelectorProvider {
    pub id: String,
    pub name: String,
    pub auth_type: AuthSelectorProviderAuthType,
    pub method: Option<AuthSelectorProviderMethod>,
    pub status: Option<AuthCheck>,
    /// Whether the provider's OAuth sign-in is backed by a subscription. `false` labels it as an account;
    /// unset keeps the "subscription" label.
    pub subscription: Option<bool>,
}


impl From<AuthType> for AuthSelectorProviderAuthType {
    fn from(value: AuthType) -> Self {
        match value {
            AuthType::ApiKey => Self::ApiKey,
            AuthType::OAuth => Self::Oauth,
        }
    }
}

impl AuthSelectorProviderMethod {
    pub fn name(&self) -> String {
        match self {
            Self::Arc(m) => m.name().to_string(),
            Self::Arc2(m) => m.name().to_string(),
        }
    }
}

pub fn format_auth_selector_provider_type(
    auth_type: AuthSelectorProviderAuthType,
    subscription: Option<bool>,
) -> String {
    if auth_type == AuthSelectorProviderAuthType::ApiKey {
        return "API key".to_string();
    }
    if subscription == Some(false) { "account".to_string() } else { "subscription".to_string() }
}

static ENV_SOURCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][A-Z0-9_]*(?:, [A-Z][A-Z0-9_]*)*$").expect("valid regex"));

/// Themed suffix describing whether and how a login option is configured, for example " ✓ configured".
pub fn format_auth_selector_provider_status(provider: &AuthSelectorProvider) -> String {
    let t = theme();
    let Some(status) = &provider.status else {
        return t.fg(&ThemeColor::Muted, " • not configured");
    };
    let status_type = AuthSelectorProviderAuthType::from(status.auth_type);
    if status_type != provider.auth_type {
        let label = format!(
            "{} configured",
            format_auth_selector_provider_type(status_type, provider.subscription)
        );
        return t.fg(&ThemeColor::Muted, " • ") + &t.fg(&ThemeColor::Warning, &label);
    }
    let source = match status.source.as_deref() {
        None | Some("") | Some("OAuth") | Some("stored credential") => {
            return t.fg(&ThemeColor::Success, " ✓ configured");
        }
        Some(source) => source,
    };
    let source = if ENV_SOURCE_RE.is_match(source) { format!("env: {source}") } else { source.to_string() };
    t.fg(&ThemeColor::Success, &format!(" ✓ {source}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OAuthSelectorComponentMode {
    #[serde(rename = "login")]
    Login,
    #[serde(rename = "logout")]
    Logout,
}

impl OAuthSelectorComponentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Logout => "logout",
        }
    }
}

struct State {
    filtered_providers: Vec<AuthSelectorProvider>,
    selected_index: usize,
    focused: bool,
}

struct Inner {
    container: Container,
    search_input: Arc<Input>,
    list_container: Arc<Container>,
    all_providers: Vec<AuthSelectorProvider>,
    state: Mutex<State>,
    mode: OAuthSelectorComponentMode,
    on_select_callback: Arc<dyn Fn(String, AuthSelectorProviderAuthType) + Send + Sync>,
    on_cancel_callback: Arc<dyn Fn() + Send + Sync>,
    show_auth_type_labels: bool,
}

impl Inner {
    fn select_current(&self) {
        let selected = {
            let s = self.state.lock();
            s.filtered_providers.get(s.selected_index).map(|p| (p.id.clone(), p.auth_type))
        };
        if let Some((id, auth_type)) = selected {
            (self.on_select_callback)(id, auth_type);
        }
    }

    fn filter_providers(&self, query: &str) {
        {
            let mut s = self.state.lock();
            s.filtered_providers = if query.is_empty() {
                self.all_providers.clone()
            } else {
                fuzzy_filter(&self.all_providers, query, |provider| {
                    format!(
                        "{} {} {} {}",
                        provider.name,
                        provider.id,
                        provider.auth_type.as_str(),
                        provider.method.as_ref().map(|m| m.name()).unwrap_or_default()
                    )
                })
            };
            let max = s.filtered_providers.len().saturating_sub(1);
            s.selected_index = s.selected_index.min(max);
        }
        self.update_list();
    }

    fn update_list(&self) {
        self.list_container.clear();
        let t = theme();
        let s = self.state.lock();
        let len = s.filtered_providers.len() as i64;
        let max_visible: i64 = 8;
        let selected = s.selected_index as i64;
        let start_index = (selected - max_visible / 2).min(len - max_visible).max(0);
        let end_index = (start_index + max_visible).min(len);

        for i in start_index..end_index {
            let provider = &s.filtered_providers[i as usize];
            let is_selected = i == selected;
            let status_indicator = format_auth_selector_provider_status(provider);
            let auth_type_label = if self.show_auth_type_labels {
                t.fg(
                    &ThemeColor::Muted,
                    &format!(" [{}]", format_auth_selector_provider_type(provider.auth_type, provider.subscription)),
                )
            } else {
                String::new()
            };
            let line = if is_selected {
                let prefix = t.fg(&ThemeColor::Accent, "→ ");
                let text = t.fg(&ThemeColor::Accent, &provider.name);
                format!("{prefix}{text}{auth_type_label}{status_indicator}")
            } else {
                let text = format!("  {}", t.fg(&ThemeColor::Text, &provider.name));
                format!("{text}{auth_type_label}{status_indicator}")
            };
            self.list_container.add_child(Arc::new(TruncatedText::new(line, 1, 0)));
        }

        if start_index > 0 || end_index < len {
            let scroll_info = t.fg(&ThemeColor::Muted, &format!("  ({}/{})", selected + 1, len));
            self.list_container.add_child(Arc::new(TruncatedText::new(scroll_info, 1, 0)));
        }

        // Show "no providers" if empty
        if len == 0 {
            let message = if self.all_providers.is_empty() {
                if self.mode == OAuthSelectorComponentMode::Login {
                    "No providers available"
                } else {
                    "No providers logged in. Use /login first."
                }
            } else {
                "No matching providers"
            };
            self.list_container
                .add_child(Arc::new(TruncatedText::new(t.fg(&ThemeColor::Muted, &format!("  {message}")), 1, 0)));
        }
    }
}

/// Component that renders an auth provider selector
/// TS: `extends Container implements Focusable`
pub struct OAuthSelectorComponent {
    inner: Arc<Inner>,
}

impl OAuthSelectorComponent {
    pub fn new(
        mode: OAuthSelectorComponentMode,
        providers: &[AuthSelectorProvider],
        on_select: Arc<dyn Fn(String, AuthSelectorProviderAuthType) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        initial_search_input: Option<&str>,
    ) -> Self {
        let mut auth_types: Vec<AuthSelectorProviderAuthType> = Vec::new();
        for p in providers {
            if !auth_types.contains(&p.auth_type) {
                auth_types.push(p.auth_type);
            }
        }
        let show_auth_type_labels = auth_types.len() > 1;

        let inner = Arc::new_cyclic(|weak: &Weak<Inner>| {
            let t = theme();
            let container = Container::new();
            // Add top border
            container.add_child(Arc::new(DynamicBorder::new(None)));
            container.add_child(Arc::new(Spacer::new(1)));

            // Add title
            let title = if mode == OAuthSelectorComponentMode::Login {
                "Select provider to configure:"
            } else {
                "Select provider to logout:"
            };
            container.add_child(Arc::new(TruncatedText::new(t.fg(&ThemeColor::Accent, &t.bold(title)), 1, 0)));
            container.add_child(Arc::new(Spacer::new(1)));

            let search_input = Arc::new(Input::default());
            if let Some(initial) = initial_search_input.filter(|s| !s.is_empty()) {
                search_input.set_value(initial);
            }
            let w = weak.clone();
            search_input.set_on_submit(Some(Arc::new(move |_v: String| {
                if let Some(inner) = w.upgrade() {
                    inner.select_current();
                }
            })));
            container.add_child(search_input.clone());
            container.add_child(Arc::new(Spacer::new(1)));

            // Create list container
            let list_container = Arc::new(Container::new());
            container.add_child(list_container.clone());
            container.add_child(Arc::new(Spacer::new(1)));

            // Add bottom border
            container.add_child(Arc::new(DynamicBorder::new(None)));

            Inner {
                container,
                search_input,
                list_container,
                all_providers: providers.to_vec(),
                state: Mutex::new(State {
                    filtered_providers: providers.to_vec(),
                    selected_index: 0,
                    focused: false,
                }),
                mode,
                on_select_callback: on_select,
                on_cancel_callback: on_cancel,
                show_auth_type_labels,
            }
        });

        // Initial render
        inner.filter_providers(initial_search_input.unwrap_or(""));
        Self { inner }
    }
}

impl Component for OAuthSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.inner.container.render(width)
    }

    fn handle_input(&self, data: &str) {
        let inner = &self.inner;
        let kb = get_keybindings();
        if kb.matches(data, "tui.select.up") {
            {
                let mut s = inner.state.lock();
                if s.filtered_providers.is_empty() {
                    return;
                }
                s.selected_index = s.selected_index.saturating_sub(1);
            }
            inner.update_list();
        } else if kb.matches(data, "tui.select.down") {
            {
                let mut s = inner.state.lock();
                if s.filtered_providers.is_empty() {
                    return;
                }
                s.selected_index = (s.selected_index + 1).min(s.filtered_providers.len() - 1);
            }
            inner.update_list();
        } else if kb.matches(data, "tui.select.confirm") {
            inner.select_current();
        } else if kb.matches(data, "tui.select.cancel") {
            (inner.on_cancel_callback)();
        } else {
            // Pass everything else to search input
            inner.search_input.handle_input(data);
            inner.filter_providers(&inner.search_input.get_value());
        }
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
        self.inner.state.lock().focused = focused;
        self.inner.search_input.set_focused(focused);
    }

    fn focused(&self) -> bool {
        self.inner.state.lock().focused
    }
}
