//! Port of `packages/coding-agent/src/extensions/mcp/runtime.ts`.
//!
//! The part of the MCP integration that talks to servers: connections, transports, and OAuth
//! sign-in.

use crate::config::VERSION;
use crate::core::mcp_servers::McpServerConfig;
use crate::core::resolve_config_value::resolve_config_value_or_throw;
use crate::extensions::mcp::config::McpServerEntry;
use crate::extensions::mcp::oauth::{
    CreateMcpAuthProviderOptions, McpAuthProvider, McpOAuthSettings,
    McpOAuthSettingsClientRegistration, create_mcp_auth_provider,
};
use crate::extensions::mcp::resources::McpResourceServer;
use crate::extensions::mcp::tools::McpToolCaller;
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_mcp::auth_provider::AuthProvider;
use pi_mcp::client::{ConnectionState, McpClient, McpClientOptions, McpRequestOptions};
use pi_mcp::oauth::types::OAuthChallenge;
use pi_mcp::protocol::content::CallToolResult;
use pi_mcp::protocol::types::ListResourceTemplatesResult;
use pi_mcp::protocol::types::ListResourcesResult;
use pi_mcp::protocol::types::ReadResourceResult;
use pi_mcp::protocol::types::Resource;
use pi_mcp::protocol::types::ResourceTemplate;
use pi_mcp::protocol::types::{Root, Tool};
use pi_mcp::transports::transport::McpTransport;
use pi_mcp::{
    Error as McpError, JSON_RPC_ERROR_CODES, StdioTransport, StdioTransportOptions,
    StreamableHttpTransport, StreamableHttpTransportOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub use crate::extensions::mcp::log::McpServerLog;
pub use crate::extensions::mcp::oauth::McpOAuthCredentialStore;
pub use crate::extensions::mcp::oauth::McpSignInCancelledError;
pub use crate::extensions::mcp::oauth::sign_in_mcp_server;

const DEFAULT_TIMEOUT_SECONDS: i64 = 60;
const STDERR_TAIL_CHARS: usize = 2_000;
/// Delays between attempts to connect to an HTTP server that failed with a transient error.
const CONNECT_RETRY_DELAYS_MS: [u64; 2] = [250, 1_000];

/// `disconnected`: the connection dropped (for example the stdio server exited); the next call
/// reconnects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServerState {
    #[serde(rename = "connecting")]
    Connecting,
    #[serde(rename = "connected")]
    Connected,
    #[serde(rename = "disconnected")]
    Disconnected,
    #[serde(rename = "needs-auth")]
    NeedsAuth,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "closed")]
    Closed,
}

impl ServerState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::NeedsAuth => "needs-auth",
            Self::Failed => "failed",
            Self::Closed => "closed",
        }
    }
}

/// A transport made by a [`McpTransportFactory`]. `stdio` is set for stdio transports, whose
/// stderr explains connection failures (TS: `transport instanceof StdioTransport`).
#[derive(Clone)]
pub struct McpTransportHandle {
    pub transport: Arc<dyn McpTransport>,
    pub stdio: Option<StdioTransport>,
}

impl McpTransportHandle {
    pub fn new(transport: impl McpTransport) -> Self {
        Self {
            transport: Arc::new(transport),
            stdio: None,
        }
    }

    pub fn from_arc(transport: Arc<dyn McpTransport>) -> Self {
        Self {
            transport,
            stdio: None,
        }
    }

    pub fn stdio(transport: StdioTransport) -> Self {
        Self {
            transport: Arc::new(transport.clone()),
            stdio: Some(transport),
        }
    }
}

/// Creates the transport of a server. Fails when the config cannot be resolved (for example a
/// header referencing a missing environment variable).
pub type McpTransportFactory = Arc<
    dyn Fn(McpServerEntry, String, Option<Arc<dyn AuthProvider>>) -> anyhow::Result<McpTransportHandle>
        + Send
        + Sync,
>;

fn error_message(error: &anyhow::Error) -> String {
    error.to_string()
}

fn mcp_error(error: &anyhow::Error) -> Option<&McpError> {
    error.downcast_ref::<McpError>()
}

/// Network failures and overloaded or restarting servers, which are worth another attempt.
fn is_transient_error(error: &McpError) -> bool {
    if let Some(status) = error.http_status() {
        return status == 408 || status == 429 || (status >= 500 && status != 501);
    }
    error.is_type_error()
}

fn is_transient(error: &anyhow::Error) -> bool {
    mcp_error(error).is_some_and(is_transient_error)
}

fn auth_provider_name(entry: &McpServerEntry) -> Option<String> {
    match &entry.config {
        McpServerConfig::McpHttpServerConfig(http) => {
            http.auth.as_ref().map(|auth| auth.provider.clone())
        }
        McpServerConfig::McpStdioServerConfig(_) => None,
    }
}

fn sign_in_required_message(entry: &McpServerEntry) -> String {
    let command = match auth_provider_name(entry) {
        Some(provider) => format!("/login {provider}"),
        None => "/mcp".to_string(),
    };
    format!(
        "MCP server \"{}\" requires sign-in. Run {command} to sign in.",
        entry.name
    )
}

/// HTTP servers authenticate with OAuth unless the config supplies an `Authorization` header or `auth`.
fn uses_oauth(entry: &McpServerEntry) -> bool {
    let McpServerConfig::McpHttpServerConfig(config) = &entry.config else {
        return false;
    };
    if config.auth.is_some() {
        return false;
    }
    !config.headers.as_ref().is_some_and(|headers| {
        headers
            .keys()
            .any(|header| header.eq_ignore_ascii_case("authorization"))
    })
}

fn home_dir() -> String {
    dirs::home_dir()
        .map(|home| home.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `~` and `~/…` (also `~\…` on Windows) name the home directory, like in a shell.
fn expand_home(value: &str) -> String {
    if value == "~" {
        return home_dir();
    }
    if value.starts_with("~/") || (cfg!(windows) && value.starts_with("~\\")) {
        return Path::new(&home_dir())
            .join(&value[2..])
            .to_string_lossy()
            .into_owned();
    }
    value.to_string()
}

/// `path.resolve(base, path)`.
fn resolve_path(base: &str, path: &str) -> String {
    let joined = Path::new(base).join(path);
    let mut normalized = std::path::PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized.to_string_lossy().into_owned()
}

pub fn create_default_transport(
    entry: &McpServerEntry,
    cwd: &str,
    auth_provider: Option<Arc<dyn AuthProvider>>,
) -> anyhow::Result<McpTransportHandle> {
    let name = &entry.name;
    match &entry.config {
        McpServerConfig::McpHttpServerConfig(config) => {
            let mut options = StreamableHttpTransportOptions::parse(&config.url)?;
            for (key, value) in config.headers.iter().flatten() {
                let resolved = resolve_config_value_or_throw(
                    value,
                    &format!("MCP server \"{name}\" header \"{key}\""),
                    None,
                )?;
                options = options.header(key.clone(), resolved);
            }
            if let Some(provider) = auth_provider {
                options = options.auth_provider(provider);
            }
            Ok(McpTransportHandle::new(StreamableHttpTransport::new(
                options,
            )))
        }
        McpServerConfig::McpStdioServerConfig(config) => {
            let mut options = StdioTransportOptions::new(expand_home(&config.command)).args(
                config
                    .args
                    .iter()
                    .flatten()
                    .map(|arg| expand_home(arg))
                    .collect::<Vec<_>>(),
            );
            for (key, value) in config.env.iter().flatten() {
                let resolved = resolve_config_value_or_throw(
                    value,
                    &format!("MCP server \"{name}\" env \"{key}\""),
                    None,
                )?;
                options = options.env(key.clone(), resolved);
            }
            options = options.cwd(resolve_path(
                cwd,
                &expand_home(config.cwd.as_deref().unwrap_or(".")),
            ));
            Ok(McpTransportHandle::stdio(StdioTransport::new(options)))
        }
    }
}

/// The default [`McpTransportFactory`].
pub fn default_transport_factory() -> McpTransportFactory {
    Arc::new(|entry, cwd, auth_provider| create_default_transport(&entry, &cwd, auth_provider))
}

fn is_method_not_found(error: &McpError) -> bool {
    error.rpc_code() == Some(JSON_RPC_ERROR_CODES.method_not_found)
}

#[derive(Clone)]
pub struct McpServerConnectionOptions {
    pub entry: McpServerEntry,
    pub cwd: String,
    pub create_transport: McpTransportFactory,
    pub credentials: Arc<McpOAuthCredentialStore>,
    /// The current token of a pi provider, for servers with `auth.provider`.
    pub provider_token: Option<
        Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<Option<String>>> + Send + Sync>,
    >,
    pub on_tools: Arc<dyn Fn(Arc<McpServerConnection>) + Send + Sync>,
    /// Called when `state`, `error`, or `tools` change.
    pub on_change: Option<Arc<dyn Fn(Arc<McpServerConnection>) + Send + Sync>>,
    /// Receives the server's log messages (`notifications/message`).
    pub log: Option<Arc<McpServerLog>>,
}

/// Sends the token of a pi provider. Read on every request, so the provider's refreshes apply; MCP
/// stores no copy.
struct ProviderTokenAuth {
    provider: String,
    token: Option<
        Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<Option<String>>> + Send + Sync>,
    >,
}

impl AuthProvider for ProviderTokenAuth {
    fn token(&self) -> BoxFuture<'_, pi_mcp::Result<Option<String>>> {
        Box::pin(async move {
            match &self.token {
                Some(token) => token(self.provider.clone())
                    .await
                    .map_err(|error| McpError::Other(error.to_string())),
                None => Ok(None),
            }
        })
    }
}

#[async_trait::async_trait]
impl McpAuthProvider for ProviderTokenAuth {
    async fn settled(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

type SharedOpen = Shared<BoxFuture<'static, Result<McpClient, Arc<anyhow::Error>>>>;

struct ConnectionInner {
    state: ServerState,
    error: Option<String>,
    tools: Vec<Tool>,
    has_resources: bool,
    instructions: Option<String>,
    /// The connected client and its id, which identifies it in callbacks.
    client: Option<(u64, McpClient)>,
    opening: Option<SharedOpen>,
    /// Stderr of the last stdio server that failed to connect.
    stderr_tail: Option<String>,
}

static NEXT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

/// One configured server. Reconnects lazily when a call finds the connection gone.
/// TS: `implements McpToolCaller, McpResourceServer`
pub struct McpServerConnection {
    this: Weak<McpServerConnection>,
    entry: McpServerEntry,
    cwd: String,
    create_transport: McpTransportFactory,
    auth_provider: Option<Arc<dyn McpAuthProvider>>,
    on_tools: Arc<dyn Fn(Arc<McpServerConnection>) + Send + Sync>,
    on_change: Option<Arc<dyn Fn(Arc<McpServerConnection>) + Send + Sync>>,
    log: Option<Arc<McpServerLog>>,
    /// Aborted by `close()`; cancels a connect in progress, including the wait between retries.
    shutdown: CancellationToken,
    /// Last OAuth challenge from the server; shared with the auth provider that records it.
    challenge: Arc<Mutex<Option<OAuthChallenge>>>,
    inner: Mutex<ConnectionInner>,
}

/// A memoized close of a client: a second `client.close()` returns before the first finished
/// closing the transport.
fn memoized_close(client: &McpClient) -> Shared<BoxFuture<'static, ()>> {
    // `close()` starts closing when called, so it is only called once the future runs.
    let client = client.clone();
    async move {
        let _ = client.close().await;
    }
    .boxed()
    .shared()
}

impl McpServerConnection {
    pub fn new(options: &McpServerConnectionOptions) -> Arc<Self> {
        Arc::new_cyclic(|this: &Weak<Self>| {
            let challenge: Arc<Mutex<Option<OAuthChallenge>>> = Arc::default();
            let entry = options.entry.clone();
            let oauth_url = oauth_url_of(&entry);
            let provider = auth_provider_name(&entry);
            let auth_provider: Option<Arc<dyn McpAuthProvider>> = if let Some(url) = oauth_url {
                let settings_this = this.clone();
                let recorded = challenge.clone();
                create_mcp_auth_provider(&CreateMcpAuthProviderOptions {
                    server_url: url.clone(),
                    store: options.credentials.for_server(&entry.name, &url),
                    settings: Arc::new(move || match settings_this.upgrade() {
                        Some(connection) => connection.oauth_settings(),
                        None => Ok(McpOAuthSettings::default()),
                    }),
                    on_challenge: Arc::new(move |value| {
                        *recorded.lock() = Some(value);
                    }),
                })
                .ok()
            } else {
                provider.map(|provider| {
                    Arc::new(ProviderTokenAuth {
                        provider,
                        token: options.provider_token.clone(),
                    }) as Arc<dyn McpAuthProvider>
                })
            };
            Self {
                this: this.clone(),
                entry,
                cwd: options.cwd.clone(),
                create_transport: options.create_transport.clone(),
                auth_provider,
                on_tools: options.on_tools.clone(),
                on_change: options.on_change.clone(),
                log: options.log.clone(),
                shutdown: CancellationToken::new(),
                challenge,
                inner: Mutex::new(ConnectionInner {
                    state: ServerState::Connecting,
                    error: None,
                    tools: Vec::new(),
                    has_resources: false,
                    instructions: None,
                    client: None,
                    opening: None,
                    stderr_tail: None,
                }),
            }
        })
    }

    fn arc(&self) -> Option<Arc<Self>> {
        self.this.upgrade()
    }

    pub fn entry(&self) -> McpServerEntry {
        self.entry.clone()
    }

    pub fn state(&self) -> ServerState {
        self.inner.lock().state
    }

    pub fn set_state(&self, value: ServerState) {
        self.inner.lock().state = value;
    }

    pub fn error(&self) -> Option<String> {
        self.inner.lock().error.clone()
    }

    pub fn set_error(&self, value: Option<String>) {
        self.inner.lock().error = value;
    }

    pub fn tools(&self) -> Vec<Tool> {
        self.inner.lock().tools.clone()
    }

    pub fn set_tools(&self, value: Vec<Tool>) {
        self.inner.lock().tools = value;
    }

    /// Whether the server offers resources, which the resource tools list and read on demand. They are
    /// not listed at connect: listing every resource can take long (a large drive, say).
    pub fn has_resources(&self) -> bool {
        self.inner.lock().has_resources
    }

    pub fn set_has_resources(&self, value: bool) {
        self.inner.lock().has_resources = value;
    }

    /// Server instructions from `initialize`, describing its tools as a group.
    pub fn instructions(&self) -> Option<String> {
        self.inner.lock().instructions.clone()
    }

    pub fn set_instructions(&self, value: Option<String>) {
        self.inner.lock().instructions = value;
    }

    /// Last OAuth challenge from the server; sign-in uses its resource metadata URL and scope.
    pub fn challenge(&self) -> Option<OAuthChallenge> {
        self.challenge.lock().clone()
    }

    pub fn set_challenge(&self, value: Option<OAuthChallenge>) {
        *self.challenge.lock() = value;
    }

    /// The server name.
    pub fn name(&self) -> String {
        self.entry.name.clone()
    }

    /// Per-request timeout in milliseconds.
    pub fn timeout_ms(&self) -> i64 {
        let seconds = match &self.entry.config {
            McpServerConfig::McpStdioServerConfig(config) => config.timeout,
            McpServerConfig::McpHttpServerConfig(config) => config.timeout,
        };
        seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS) * 1000
    }

    fn closed(&self) -> bool {
        self.shutdown.is_cancelled()
    }

    fn is_http(&self) -> bool {
        matches!(self.entry.config, McpServerConfig::McpHttpServerConfig(_))
    }

    /// Server URL when the server authenticates with OAuth.
    pub fn oauth_url(&self) -> Option<String> {
        oauth_url_of(&self.entry)
    }

    /// OAuth client settings from the config. Fails when the client secret cannot be resolved.
    pub fn oauth_settings(&self) -> anyhow::Result<McpOAuthSettings> {
        let McpServerConfig::McpHttpServerConfig(config) = &self.entry.config else {
            return Ok(McpOAuthSettings::default());
        };
        let Some(oauth) = &config.oauth else {
            return Ok(McpOAuthSettings::default());
        };
        let client_secret = match &oauth.client_secret {
            Some(secret) => Some(resolve_config_value_or_throw(
                secret,
                &format!("MCP server \"{}\" oauth.clientSecret", self.entry.name),
                None,
            )?),
            None => None,
        };
        Ok(McpOAuthSettings {
            client_id: oauth.client_id.clone(),
            client_secret,
            callback_port: oauth.callback_port,
            callback_url: oauth.callback_url.clone(),
            scope: oauth.scope.clone(),
            client_name: oauth.client_name.clone(),
            client_registration: oauth.client_registration.map(|registration| {
                match registration {
                    crate::core::mcp_servers::McpOAuthConfigClientRegistration::Dcr => {
                        McpOAuthSettingsClientRegistration::Dcr
                    }
                    crate::core::mcp_servers::McpOAuthConfigClientRegistration::Cimd => {
                        McpOAuthSettingsClientRegistration::Cimd
                    }
                }
            }),
            auth_server_metadata_url: oauth
                .auth_server_metadata_url
                .clone()
                .filter(|url| !url.is_empty()),
        })
    }

    pub async fn get_client(&self) -> anyhow::Result<McpClient> {
        if self.closed() {
            anyhow::bail!("MCP server \"{}\" is shut down", self.entry.name);
        }
        let opening = {
            let mut inner = self.inner.lock();
            if let Some((_, client)) = &inner.client
                && client.connection_state() == ConnectionState::Connected
            {
                return Ok(client.clone());
            }
            match &inner.opening {
                Some(opening) => opening.clone(),
                None => {
                    let Some(this) = self.arc() else {
                        anyhow::bail!("MCP server \"{}\" is shut down", self.entry.name);
                    };
                    // Runs as a task, so the connection opens even when every caller stops waiting.
                    let task = tokio::spawn(async move {
                        let result = this.open().await;
                        this.inner.lock().opening = None;
                        result
                    });
                    let opening: SharedOpen = async move {
                        match task.await {
                            Ok(result) => result.map_err(Arc::new),
                            Err(error) => Err(Arc::new(anyhow::anyhow!("{error}"))),
                        }
                    }
                    .boxed()
                    .shared();
                    inner.opening = Some(opening.clone());
                    opening
                }
            }
        };
        opening
            .await
            .map_err(|error| anyhow::anyhow!("{}", error_message(&error)))
    }

    /// Run a request, reconnecting when needed. `read_only` requests are retried once after a transient
    /// HTTP error; tool calls are not, since they may have run.
    async fn with_client<T, F, Fut>(&self, run: F, read_only: bool) -> anyhow::Result<T>
    where
        F: Fn(McpClient) -> Fut,
        Fut: std::future::Future<Output = pi_mcp::Result<T>>,
    {
        let mut attempt = 1;
        loop {
            let client = self.get_client().await?;
            let error = match run(client.clone()).await {
                Ok(value) => return Ok(value),
                Err(error) => error,
            };
            if read_only && attempt == 1 && error.is_http() && is_transient_error(&error) {
                tokio::time::sleep(Duration::from_millis(CONNECT_RETRY_DELAYS_MS[0])).await;
                attempt += 1;
                continue;
            }
            if matches!(error, McpError::SessionExpired { .. }) && attempt == 1 {
                // The server no longer knows the session (restart, deploy), so it did not run the request.
                // Retry once on a new session. The old client is detached but not closed: closing would
                // fail its other in-flight calls, which instead get the same 404 and retry the same way.
                let mut inner = self.inner.lock();
                if inner
                    .client
                    .as_ref()
                    .is_some_and(|(_, current)| same_client(current, &client))
                {
                    inner.client = None;
                }
                drop(inner);
                attempt += 1;
                continue;
            }
            if !self.needs_sign_in(&error) {
                return Err(error.into());
            }
            self.drop_client(&client).await;
            self.mark_needs_auth();
            anyhow::bail!("{}", sign_in_required_message(&self.entry));
        }
    }

    async fn wait_for_opening(&self) {
        let opening = self.inner.lock().opening.clone();
        if let Some(opening) = opening {
            let _ = opening.await;
        }
    }

    /// Connect again with fresh credentials, for example after signing in.
    pub async fn reconnect(&self) -> anyhow::Result<()> {
        self.wait_for_opening().await;
        let client = self.inner.lock().client.as_ref().map(|(_, client)| client.clone());
        if let Some(client) = client {
            self.drop_client(&client).await;
        }
        self.get_client().await?;
        Ok(())
    }

    /// Disconnect after the stored credentials were removed.
    pub async fn sign_out(&self) -> anyhow::Result<()> {
        self.wait_for_opening().await;
        let client = self.inner.lock().client.as_ref().map(|(_, client)| client.clone());
        if let Some(client) = client {
            self.drop_client(&client).await;
        }
        if !self.closed() {
            self.mark_needs_auth();
        }
        Ok(())
    }

    /// OAuth servers that still reject the request after a refresh need the user to sign in again.
    fn needs_sign_in(&self, error: &McpError) -> bool {
        matches!(error, McpError::OAuthAuthorizationRequired)
            || (self.auth_provider.is_some() && matches!(error, McpError::AuthRequired { .. }))
    }

    fn needs_sign_in_any(&self, error: &anyhow::Error) -> bool {
        mcp_error(error).is_some_and(|error| self.needs_sign_in(error))
    }

    fn mark_needs_auth(&self) {
        {
            let mut inner = self.inner.lock();
            inner.state = ServerState::NeedsAuth;
            inner.error = None;
        }
        self.changed();
    }

    fn changed(&self) {
        if let (Some(on_change), Some(this)) = (&self.on_change, self.arc()) {
            on_change(this);
        }
    }

    async fn drop_client(&self, client: &McpClient) {
        {
            let mut inner = self.inner.lock();
            if inner
                .client
                .as_ref()
                .is_some_and(|(_, current)| same_client(current, client))
            {
                inner.client = None;
            }
        }
        let _ = client.close().await;
    }

    async fn open(self: &Arc<Self>) -> anyhow::Result<McpClient> {
        self.inner.lock().state = ServerState::Connecting;
        self.changed();
        let retries: &[u64] = if self.is_http() {
            &CONNECT_RETRY_DELAYS_MS
        } else {
            &[]
        };
        let mut attempt = 0;
        loop {
            self.inner.lock().stderr_tail = None;
            let error = match self.connect_once().await {
                Ok(client) => return Ok(client),
                Err(error) => error,
            };
            let delay = retries.get(attempt).copied();
            attempt += 1;
            let Some(delay) = delay.filter(|_| !self.closed() && is_transient(&error)) else {
                return Err(self.connect_failed(error));
            };
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                _ = self.shutdown.cancelled() => {}
            }
            if self.closed() {
                return Err(self.connect_failed(error));
            }
        }
    }

    async fn connect_once(self: &Arc<Self>) -> anyhow::Result<McpClient> {
        let root_uri = url::Url::from_file_path(&self.cwd)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| format!("file://{}", self.cwd));
        let root_name = Path::new(&self.cwd)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let client = McpClient::new(
            McpClientOptions::new("pi", VERSION)
                .request_timeout_ms(self.timeout_ms().max(0) as u64)
                .roots(vec![Root::new(root_uri, Some(root_name))]),
        );
        let id = NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed);
        if let Some(log) = self.log.clone() {
            let name = self.entry.name.clone();
            let _ = client.on_notification("notifications/message", move |params| {
                log.write(&name, params.unwrap_or(Value::Null));
            });
        }
        // Closing the client fails its pending initialize or discovery requests and closes the transport.
        let close_client = memoized_close(&client);
        let on_shutdown = {
            let shutdown = self.shutdown.clone();
            let close_client = close_client.clone();
            crate::utils::uncaught::spawn(async move {
                shutdown.cancelled().await;
                close_client.await;
            })
        };
        let mut stdio: Option<StdioTransport> = None;
        let result = async {
            let handle = (self.create_transport)(
                self.entry.clone(),
                self.cwd.clone(),
                self.auth_provider
                    .clone()
                    .map(|provider| provider as Arc<dyn AuthProvider>),
            )?;
            stdio = handle.stdio.clone();
            client.connect(handle.transport.clone()).await?;
            {
                let this = self.this.clone();
                let notified = client.clone();
                let _ = client.on_notification("notifications/tools/list_changed", move |_| {
                    if let Some(this) = this.upgrade() {
                        let client = notified.clone();
                        crate::utils::uncaught::spawn(async move { this.refresh_tools(id, client).await });
                    }
                });
            }
            {
                let this = self.this.clone();
                let stdio = handle.stdio.clone();
                let _ = client.on_close(move || {
                    if let Some(this) = this.upgrade() {
                        this.handle_client_close(id, stdio.as_ref());
                    }
                });
            }
            // Servers without the tools capability (prompts or resources only) do not answer tools/list.
            let capabilities = client.server_capabilities();
            let tools = if capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.tools.is_some())
            {
                client.list_tools(McpRequestOptions::default()).await?
            } else {
                Vec::new()
            };
            if self.closed() {
                anyhow::bail!("shut down while connecting");
            }
            if client.connection_state() != ConnectionState::Connected {
                anyhow::bail!("connection closed during setup");
            }
            {
                let mut inner = self.inner.lock();
                inner.client = Some((id, client.clone()));
                inner.tools = tools;
                inner.has_resources = capabilities
                    .as_ref()
                    .is_some_and(|capabilities| capabilities.resources.is_some());
                inner.instructions = client
                    .instructions()
                    .map(|instructions| instructions.trim().to_string())
                    .filter(|instructions| !instructions.is_empty());
                inner.state = ServerState::Connected;
                inner.error = None;
            }
            (self.on_tools)(self.clone());
            self.changed();
            Ok::<_, anyhow::Error>(client.clone())
        }
        .await;
        on_shutdown.abort();
        match result {
            Ok(client) => Ok(client),
            Err(error) => {
                close_client.await;
                if let Some(stdio) = &stdio {
                    let tail = stderr_tail(&stdio.stderr());
                    self.inner.lock().stderr_tail = tail;
                }
                Err(error)
            }
        }
    }

    fn connect_failed(&self, error: anyhow::Error) -> anyhow::Error {
        if self.needs_sign_in_any(&error) && !self.closed() {
            self.mark_needs_auth();
            return anyhow::anyhow!("{}", sign_in_required_message(&self.entry));
        }
        let message = {
            let mut inner = self.inner.lock();
            inner.state = if self.closed() {
                ServerState::Closed
            } else {
                ServerState::Failed
            };
            let message = match &inner.stderr_tail {
                Some(tail) => format!("{}\n{tail}", error_message(&error)),
                None => error_message(&error),
            };
            inner.error = Some(message.clone());
            message
        };
        self.changed();
        anyhow::anyhow!(
            "MCP server \"{}\" failed to connect: {message}",
            self.entry.name
        )
    }

    /// The transport dropped. The next call reconnects; until then the status shows why.
    fn handle_client_close(&self, id: u64, stdio: Option<&StdioTransport>) {
        {
            let mut inner = self.inner.lock();
            if inner.client.as_ref().map(|(current, _)| *current) != Some(id) || self.closed() {
                return;
            }
            inner.client = None;
            inner.state = ServerState::Disconnected;
            let stderr = stdio.and_then(|stdio| stderr_tail(&stdio.stderr()));
            inner.error = Some(match stderr {
                Some(stderr) => format!("Connection closed\n{stderr}"),
                None => "Connection closed".to_string(),
            });
        }
        self.changed();
    }

    async fn refresh_tools(self: Arc<Self>, id: u64, client: McpClient) {
        match client.list_tools(McpRequestOptions::default()).await {
            Ok(tools) => {
                {
                    let mut inner = self.inner.lock();
                    if inner.client.as_ref().map(|(current, _)| *current) != Some(id)
                        || self.closed()
                    {
                        return;
                    }
                    inner.tools = tools;
                }
                (self.on_tools)(self.clone());
            }
            Err(error) => {
                self.inner.lock().error = Some(format!("Failed to refresh tools: {error}"));
            }
        }
        self.changed();
    }

    /// Resolves once no transport of this server is open, including one that was still connecting.
    pub async fn close(&self) -> anyhow::Result<()> {
        self.shutdown.cancel();
        let (client, opening) = {
            let mut inner = self.inner.lock();
            inner.state = ServerState::Closed;
            (inner.client.take(), inner.opening.clone())
        };
        self.changed();
        let close = async {
            if let Some((_, client)) = client {
                let _ = client.close().await;
            }
        };
        let open = async {
            if let Some(opening) = opening {
                let _ = opening.await;
            }
        };
        futures::join!(close, open);
        // A refresh the server already answered may have rotated the refresh token; exiting before the
        // new tokens are saved would lose the grant.
        if let Some(provider) = &self.auth_provider {
            provider.settled().await?;
        }
        Ok(())
    }
}

fn oauth_url_of(entry: &McpServerEntry) -> Option<String> {
    match &entry.config {
        McpServerConfig::McpHttpServerConfig(config) if uses_oauth(entry) => {
            Some(config.url.clone())
        }
        _ => None,
    }
}

fn stderr_tail(stderr: &str) -> Option<String> {
    let trimmed = stderr.trim();
    let count = trimmed.chars().count();
    let tail: String = trimmed
        .chars()
        .skip(count.saturating_sub(STDERR_TAIL_CHARS))
        .collect();
    (!tail.is_empty()).then_some(tail)
}

/// Whether two handles are the same client.
fn same_client(a: &McpClient, b: &McpClient) -> bool {
    client_key(a) == client_key(b)
}

/// Identity of a client handle: clones share it.
fn client_key(client: &McpClient) -> usize {
    // `McpClient` is a handle around an `Arc`; its address identifies the client.
    let options: *const McpClientOptions = client.options();
    options as usize
}

#[async_trait::async_trait]
impl McpToolCaller for McpServerConnection {
    async fn call_tool(
        &self,
        name: &str,
        args: IndexMap<String, Value>,
        options: McpRequestOptions,
    ) -> anyhow::Result<CallToolResult> {
        let arguments: Map<String, Value> = args.into_iter().collect();
        self.with_client(
            |client| client.call_tool(name, Some(arguments.clone()), options.clone()),
            false,
        )
        .await
    }
}

#[async_trait::async_trait]
impl McpResourceServer for McpServerConnection {
    fn name(&self) -> String {
        self.entry.name.clone()
    }
    fn timeout_ms(&self) -> i64 {
        McpServerConnection::timeout_ms(self)
    }
    async fn resources_page(
        &self,
        cursor: Option<&str>,
        options: McpRequestOptions,
    ) -> anyhow::Result<ListResourcesResult> {
        let cursor = cursor.map(str::to_string);
        self.with_client(
            |client| client.list_resources_page(cursor.clone(), options.clone()),
            true,
        )
        .await
    }
    async fn resource_templates_page(
        &self,
        cursor: Option<&str>,
        options: McpRequestOptions,
    ) -> anyhow::Result<ListResourceTemplatesResult> {
        let cursor = cursor.map(str::to_string);
        self.with_client(
            |client| {
                let page = client.list_resource_templates_page(cursor.clone(), options.clone());
                async move {
                    match page.await {
                        // Servers that do not implement `resources/templates/list` have no templates.
                        Err(error) if is_method_not_found(&error) => {
                            Ok(ListResourceTemplatesResult {
                                resource_templates: Vec::new(),
                                next_cursor: None,
                                meta: None,
                            })
                        }
                        result => result,
                    }
                }
            },
            true,
        )
        .await
    }
    async fn all_resources(&self, options: McpRequestOptions) -> anyhow::Result<Vec<Resource>> {
        self.with_client(|client| client.list_resources(options.clone()), true)
            .await
    }
    async fn all_resource_templates(
        &self,
        options: McpRequestOptions,
    ) -> anyhow::Result<Vec<ResourceTemplate>> {
        self.with_client(
            |client| {
                let list = client.list_resource_templates(options.clone());
                async move {
                    match list.await {
                        Err(error) if is_method_not_found(&error) => Ok(Vec::new()),
                        result => result,
                    }
                }
            },
            true,
        )
        .await
    }
    async fn read_resource(
        &self,
        uri: &str,
        options: McpRequestOptions,
    ) -> anyhow::Result<ReadResourceResult> {
        self.with_client(|client| client.read_resource(uri, options.clone()), true)
            .await
    }
}
