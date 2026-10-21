//! Port of `packages/coding-agent/src/modes/interactive/components/oauth-selector.ts`.

use pi_ai::auth::types::ApiKeyAuth;
use pi_ai::auth::types::AuthCheck;
use pi_ai::auth::types::OAuthAuth;
use pi_tui::tui::Component;
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

pub fn format_auth_selector_provider_type(
    auth_type: AuthSelectorProviderAuthType,
    subscription: Option<bool>,
) -> String {
    todo!("port modes/interactive/components/oauth-selector.ts:formatAuthSelectorProviderType")
}

/// Themed suffix describing whether and how a login option is configured, for example " ✓ configured".
pub fn format_auth_selector_provider_status(provider: &AuthSelectorProvider) -> String {
    todo!("port modes/interactive/components/oauth-selector.ts:formatAuthSelectorProviderStatus")
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

/// Component that renders an auth provider selector
/// TS: `extends Container implements Focusable`
pub struct OAuthSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl OAuthSelectorComponent {
    pub fn new(
        mode: OAuthSelectorComponentMode,
        providers: &[AuthSelectorProvider],
        on_select: Arc<dyn Fn(String, AuthSelectorProviderAuthType) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        initial_search_input: Option<&str>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/oauth-selector.ts:OAuthSelectorComponent.constructor"
        )
    }
}

// TODO(port): impl pi-tui::Container for OAuthSelectorComponent

impl Component for OAuthSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/oauth-selector.ts:OAuthSelectorComponent.render")
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/oauth-selector.ts:OAuthSelectorComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}
