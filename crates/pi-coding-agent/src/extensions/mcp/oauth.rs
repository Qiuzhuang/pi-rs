//! Port of `packages/coding-agent/src/extensions/mcp/oauth.ts`.

use crate::core::auth_storage::AuthStorageBackend;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_mcp::auth_provider::AuthProvider;
use pi_mcp::oauth::provider::McpOAuthState;
use pi_mcp::oauth::provider::McpOAuthStateStore;
use pi_mcp::oauth::types::OAuthChallenge;
use pi_mcp::oauth::types::OAuthTokens;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum McpOAuthSettingsClientRegistration {
    #[serde(rename = "dcr")]
    Dcr,
    #[serde(rename = "cimd")]
    Cimd,
}

impl McpOAuthSettingsClientRegistration {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dcr => "dcr",
            Self::Cimd => "cimd",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOAuthSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Already resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub callback_port: Option<f64>,
    /// Loopback redirect URI; see `McpOAuthConfig.callbackUrl`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
    /// Scopes to request, separated by spaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// `client_name` for dynamic client registration. Default: `APP_NAME`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// See `McpOAuthConfig.clientRegistration`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_registration: Option<McpOAuthSettingsClientRegistration>,
    /// See `McpOAuthConfig.authServerMetadataUrl`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_server_metadata_url: Option<String>,
}

/// Where the loopback callback server listens and the redirect URI it serves.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallbackSettings {
    /// Address to listen on.
    pub host: String,
    /// Host name in the redirect URI.
    pub redirect_host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<i64>,
    pub path: String,
    /// The exact redirect URI, when the port is fixed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_redirect_url: Option<String>,
}

pub type StoredStates = IndexMap<String, McpOAuthState>;

#[async_trait::async_trait]
pub trait McpOAuthServerStore: McpOAuthStateStore + Send + Sync {
    /// Run `fn` while no other process refreshes the server's tokens.
    async fn with_refresh_lock(
        &self,
        r#fn: Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<Value>> + Send + Sync>,
    ) -> anyhow::Result<Value>;
}

/// Per-server OAuth state (client registration, tokens, pending PKCE verifier) in `mcp-auth.json`.
pub struct McpOAuthCredentialStore {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl McpOAuthCredentialStore {
    pub fn new(backend: Option<Arc<dyn AuthStorageBackend>>, lock_dir: Option<&str>) -> Self {
        todo!("port extensions/mcp/oauth.ts:McpOAuthCredentialStore.constructor")
    }

    pub fn for_server(&self, name: &str, server_url: &str) -> Arc<dyn McpOAuthServerStore> {
        todo!("port extensions/mcp/oauth.ts:McpOAuthCredentialStore.forServer")
    }

    /// The stored tokens of a server, for noticing sign-ins done by another process. Does not take over legacy state.
    pub fn tokens(&self, name: &str, server_url: &str) -> Option<OAuthTokens> {
        todo!("port extensions/mcp/oauth.ts:McpOAuthCredentialStore.tokens")
    }

    /// Returns whether credentials were stored for the server. Removes legacy state the server would take over.
    pub fn remove(&self, name: &str, server_url: &str) -> bool {
        todo!("port extensions/mcp/oauth.ts:McpOAuthCredentialStore.remove")
    }
}

#[async_trait::async_trait]
pub trait McpAuthProvider: AuthProvider + Send + Sync {
    /// Resolves when no refresh is running, so shutdown does not drop rotated tokens before they are saved.
    async fn settled(&self) -> anyhow::Result<()>;
}

#[derive(Clone)]
pub struct CreateMcpAuthProviderOptions {
    pub server_url: String,
    pub store: Arc<dyn McpOAuthServerStore>,
    pub settings: Arc<dyn Fn() -> McpOAuthSettings + Send + Sync>,
    pub on_challenge: Arc<dyn Fn(OAuthChallenge) + Send + Sync>,
}

/// Auth provider for MCP connections: sends the stored access token and refreshes it when it is about
/// to expire or after a 401. Throws `McpOAuthAuthorizationRequiredError` when the user has to sign in,
/// including when the server asks for more scope (`insufficient_scope`). `onChallenge` receives the
/// server's `WWW-Authenticate` challenge so sign-in can use its resource metadata URL and scope.
/// `settings` is only called when a refresh is needed, so a secret that fails to resolve fails the
/// refresh instead of the whole connection setup.
///
/// Many servers rotate refresh tokens, so two refreshes with the same refresh token lose the grant.
/// Requests in this process share one refresh, and other processes are kept out by the store's
/// refresh lock, held from reading the tokens to saving new ones. Tokens that changed meanwhile
/// (another process refreshed them, or the user signed in) are used without refreshing.
pub fn create_mcp_auth_provider(
    options: &CreateMcpAuthProviderOptions,
) -> anyhow::Result<Arc<dyn McpAuthProvider>> {
    todo!("port extensions/mcp/oauth.ts:createMcpAuthProvider")
}

#[async_trait::async_trait]
pub trait McpSignInPrompt: Send + Sync {
    /// Show the authorization URL to the user and open it in a browser.
    fn show_authorization_url(&self, url: &str);
    /// Ask for the redirect URL from the browser address bar, for when the browser cannot reach the
    /// loopback callback (for example over SSH). Aborted once the callback arrives or the sign-in is
    /// cancelled. Resolves to `undefined` or an empty string when the user cancels.
    async fn prompt_for_redirect_url(
        &self,
        signal: CancellationToken,
    ) -> anyhow::Result<Option<String>>;
}

/// TS: `extends Error`
#[derive(Debug, Clone)]
pub struct McpSignInCancelledError {
    pub message: String,
}

impl std::fmt::Display for McpSignInCancelledError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for McpSignInCancelledError {}

impl McpSignInCancelledError {
    pub fn new() -> Self {
        todo!("port extensions/mcp/oauth.ts:McpSignInCancelledError.constructor")
    }
}

// TS: Pick<OAuthCallback, "code" | "iss">
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationResponse {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
}

#[derive(Clone)]
pub struct SignInMcpServerOptions {
    pub server_url: String,
    pub store: Arc<dyn McpOAuthStateStore>,
    pub settings: McpOAuthSettings,
    pub challenge: Option<OAuthChallenge>,
    pub prompt: Arc<dyn McpSignInPrompt>,
    pub signal: Option<CancellationToken>,
}

/// Sign in to an MCP server. Uses the stored refresh token when possible; otherwise runs the browser
/// authorization code flow. Tokens are saved to `store`. Aborting `signal` stops the sign-in at any
/// step with `McpSignInCancelledError`; requests to the authorization server are also time-limited.
pub async fn sign_in_mcp_server(options: &SignInMcpServerOptions) -> anyhow::Result<()> {
    todo!("port extensions/mcp/oauth.ts:signInMcpServer")
}
