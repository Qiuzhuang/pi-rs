//! Port of `packages/coding-agent/src/extensions/mcp/oauth.ts`.
//!
//! OAuth sign-in for remote MCP servers.
//!
//! Connections never start a browser flow on their own. They send the stored access token and, after
//! a 401, try the stored refresh token. When that is not possible they fail with
//! `McpOAuthAuthorizationRequiredError`, and the user signs in through `/mcp`, which runs
//! the authorization code flow (PKCE, a Client ID Metadata Document or dynamic client registration)
//! against a loopback callback.
//!
//! Credentials live in `<agent-dir>/mcp-auth.json`, keyed by server name and URL.

use crate::config::{APP_NAME, get_agent_dir};
use crate::core::auth_storage::{
    AuthStorageBackend, AuthStorageBackendExt, FileAuthStorageBackend, LockResult,
};
use crate::core::mcp_servers::mcp_namespace;
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_mcp::auth_provider::{AuthProvider, McpFetch, UnauthorizedContext, default_fetch};
use pi_mcp::oauth::callback::{OAuthCallbackPage, OAuthCallbackServer, OAuthCallbackServerOptions};
use pi_mcp::oauth::discovery::parse_www_authenticate;
use pi_mcp::oauth::flow::{
    OAuthClientMetadataDocument, OAuthFlowOptions, OAuthFlowResult, authorize_mcp,
    get_search_param, step_up_scope,
};
use pi_mcp::oauth::provider::McpOAuthProvider;
use pi_mcp::oauth::provider::McpOAuthProviderOptions;
use pi_mcp::oauth::provider::McpOAuthState;
use pi_mcp::oauth::provider::McpOAuthStateStore;
use pi_mcp::oauth::types::OAuthChallenge;
use pi_mcp::oauth::types::OAuthTokens;
use pi_mcp::oauth::types::{AuthorizationServerMetadata, OAuthClientInformation, OAuthClientMetadata};
use pi_mcp::{AbortSignal, Error as McpError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;
use url::Url;

const CALLBACK_HOST: &str = "127.0.0.1";
const CALLBACK_PATH: &str = "/callback";
/// Where pi.dev serves pi's Client ID Metadata Documents: `client.json` and `<callback ID>/client.json`.
const CLIENT_METADATA_BASE_URL: &str = "https://pi.dev/oauth";
/// Access tokens this close to expiry are refreshed before they are sent.
const REFRESH_SKEW_MS: f64 = 30_000.0;
/// Bounds each request to the authorization server, so an unresponsive one cannot hold the refresh lock,
/// delay shutdown, or keep a sign-in waiting for long.
const OAUTH_REQUEST_TIMEOUT_MS: u64 = 15_000;
/// A refresh lock that its holder stopped renewing (the process was killed) is taken over after this.
const REFRESH_LOCK_STALE_MS: u64 = 20_000;
/// How long to wait for another process's refresh: longer than a stale lock lives.
const REFRESH_LOCK_WAIT_MS: u64 = 25_000;
const REFRESH_LOCK_RETRY_MS: u64 = 100;

/// Redirect URI for refreshes when none is stored. Refreshing never redirects the user.
fn fallback_redirect_url() -> String {
    format!("http://{CALLBACK_HOST}{CALLBACK_PATH}")
}

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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

impl McpOAuthSettings {
    fn auth_server_metadata_url(&self) -> anyhow::Result<Option<Url>> {
        self.auth_server_metadata_url
            .as_deref()
            .map(|url| Url::parse(url).map_err(|_| anyhow::anyhow!("Invalid URL: {url}")))
            .transpose()
    }
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

fn callback_settings(settings: &McpOAuthSettings) -> anyhow::Result<CallbackSettings> {
    let configured = settings
        .callback_url
        .clone()
        .unwrap_or_else(fallback_redirect_url);
    let mut url =
        Url::parse(&configured).map_err(|_| anyhow::anyhow!("Invalid URL: {configured}"))?;
    let hostname = url.host_str().unwrap_or("").to_string();
    let address = hostname
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = url
        .port()
        .map(i64::from)
        .or(settings.callback_port.map(|port| port as i64));
    // A configured URI with a port is sent exactly as written, since servers compare it as a string.
    let fixed_redirect_url = if url.port().is_some() {
        settings.callback_url.clone()
    } else if let Some(port) = port {
        let _ = url.set_port(u16::try_from(port).ok());
        Some(url.to_string())
    } else {
        None
    };
    Ok(CallbackSettings {
        // `localhost` is served on 127.0.0.1; browsers fall back to it when ::1 refuses.
        host: if address == "localhost" {
            CALLBACK_HOST.to_string()
        } else {
            address.clone()
        },
        redirect_host: address,
        port,
        path: url.path().to_string(),
        fixed_redirect_url,
    })
}

/// Scopes of both lists, each once.
fn merge_scopes(scopes: &[Option<&str>]) -> Option<String> {
    let mut merged: Vec<&str> = Vec::new();
    for scope in scopes.iter().flatten() {
        for entry in scope.split_whitespace() {
            if !merged.contains(&entry) {
                merged.push(entry);
            }
        }
    }
    (!merged.is_empty()).then(|| merged.join(" "))
}

pub type StoredStates = IndexMap<String, McpOAuthState>;

/// States as stored: entries stay JSON so content this version does not understand survives rewrites.
type RawStates = IndexMap<String, Value>;

fn parse_states(content: Option<&str>) -> anyhow::Result<RawStates> {
    let Some(content) = content.filter(|content| !content.trim().is_empty()) else {
        return Ok(RawStates::new());
    };
    let parsed: Value = serde_json::from_str(content)?;
    Ok(match parsed {
        Value::Object(object) => object.into_iter().collect(),
        _ => RawStates::new(),
    })
}

fn serialize_states(states: &RawStates) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(states).unwrap_or_else(|_| "{}".to_string())
    )
}

fn state_of(value: Option<&Value>) -> Option<McpOAuthState> {
    value.and_then(|value| serde_json::from_value(value.clone()).ok())
}

/// Keys of a server's state: by name and URL, so servers sharing a URL keep separate accounts, and the
/// legacy key by URL alone, written by older versions.
fn store_keys(name: &str, server_url: &str) -> (String, String) {
    let legacy_key = Url::parse(server_url)
        .map(|url| url.to_string())
        .unwrap_or_else(|_| server_url.to_string());
    (format!("{}|{legacy_key}", mcp_namespace(name)), legacy_key)
}

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
    backend: Arc<dyn AuthStorageBackend>,
    /// Directory for the refresh lock files. Without one, refreshes are only serialized in this process.
    lock_dir: Option<String>,
}

fn to_mcp_error(error: anyhow::Error) -> McpError {
    match error.downcast::<McpError>() {
        Ok(error) => error,
        Err(error) => McpError::Other(error.to_string()),
    }
}

struct ServerStore {
    backend: Arc<dyn AuthStorageBackend>,
    lock_dir: Option<String>,
    key: String,
    legacy_key: String,
}

#[async_trait::async_trait]
impl McpOAuthStateStore for ServerStore {
    async fn load(&self) -> pi_mcp::Result<Option<McpOAuthState>> {
        let (key, legacy_key) = (self.key.clone(), self.legacy_key.clone());
        // The first server to load legacy state takes it over; others with the same URL sign in again.
        self.backend
            .with_lock(move |current| {
                let mut states = parse_states(current.as_deref())?;
                if states.contains_key(&key) || !states.contains_key(&legacy_key) {
                    return Ok(LockResult::read(state_of(states.get(&key))));
                }
                let legacy = states.shift_remove(&legacy_key).unwrap_or(Value::Null);
                states.insert(key.clone(), legacy);
                Ok(LockResult::write(
                    state_of(states.get(&key)),
                    serialize_states(&states),
                ))
            })
            .map_err(to_mcp_error)
    }

    async fn save(&self, state: McpOAuthState) -> pi_mcp::Result<()> {
        let key = self.key.clone();
        let value = serde_json::to_value(&state).map_err(|error| McpError::Other(error.to_string()))?;
        write_states(&*self.backend, move |states| {
            states.insert(key, value);
        })
        .map_err(to_mcp_error)
    }
}

#[async_trait::async_trait]
impl McpOAuthServerStore for ServerStore {
    async fn with_refresh_lock(
        &self,
        r#fn: Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<Value>> + Send + Sync>,
    ) -> anyhow::Result<Value> {
        let Some(lock_dir) = &self.lock_dir else {
            return r#fn().await;
        };
        let lock = RefreshLock::acquire(lock_dir, &self.key).await?;
        let result = r#fn().await;
        lock.release();
        result
    }
}

fn write_states(
    backend: &dyn AuthStorageBackend,
    update: impl FnOnce(&mut RawStates) + Send,
) -> anyhow::Result<()> {
    backend.with_lock(move |current| {
        let mut states = parse_states(current.as_deref())?;
        update(&mut states);
        Ok(LockResult::write((), serialize_states(&states)))
    })
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as f64)
        .unwrap_or(0.0)
}

/// A lock directory per server, like proper-lockfile: `mkdir` takes it, its mtime is renewed while
/// held, and a lock whose holder stopped renewing it (the process was killed) goes stale and is
/// taken over. Released locks are removed.
struct RefreshLock {
    path: std::path::PathBuf,
    renew: tokio::task::JoinHandle<()>,
}

impl RefreshLock {
    async fn acquire(lock_dir: &str, key: &str) -> anyhow::Result<Self> {
        create_private_dir(lock_dir)?;
        let digest = Sha256::digest(key.as_bytes());
        let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        let file = Path::new(lock_dir).join(format!("mcp-auth-refresh-{}", &hash[..16]));
        let path = std::path::PathBuf::from(format!("{}.lock", file.display()));
        let retries = REFRESH_LOCK_WAIT_MS / REFRESH_LOCK_RETRY_MS;
        let mut attempt = 0;
        loop {
            match std::fs::create_dir(&path) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&path)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                        .is_some_and(|age| age > Duration::from_millis(REFRESH_LOCK_STALE_MS));
                    if stale {
                        let _ = std::fs::remove_dir_all(&path);
                        continue;
                    }
                    if attempt >= retries {
                        anyhow::bail!("Lock file is already being held");
                    }
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(REFRESH_LOCK_RETRY_MS)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let renewed = path.clone();
        let renew = crate::utils::uncaught::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(REFRESH_LOCK_STALE_MS / 2)).await;
                if let Ok(dir) = std::fs::File::open(&renewed) {
                    let _ = dir.set_modified(SystemTime::now());
                }
            }
        });
        Ok(Self { path, renew })
    }

    fn release(self) {
        drop(self);
    }
}

impl Drop for RefreshLock {
    fn drop(&mut self) {
        self.renew.abort();
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn create_private_dir(path: &str) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

impl McpOAuthCredentialStore {
    pub fn new(backend: Option<Arc<dyn AuthStorageBackend>>, lock_dir: Option<&str>) -> Self {
        match backend {
            Some(backend) => Self {
                backend,
                lock_dir: lock_dir.map(str::to_string),
            },
            None => {
                let path = Path::new(&get_agent_dir()).join("mcp-auth.json");
                Self {
                    backend: Arc::new(FileAuthStorageBackend::new(Some(&path.to_string_lossy()))),
                    lock_dir: Some(get_agent_dir()),
                }
            }
        }
    }

    pub fn for_server(&self, name: &str, server_url: &str) -> Arc<dyn McpOAuthServerStore> {
        let (key, legacy_key) = store_keys(name, server_url);
        Arc::new(ServerStore {
            backend: self.backend.clone(),
            lock_dir: self.lock_dir.clone(),
            key,
            legacy_key,
        })
    }

    /// The stored tokens of a server, for noticing sign-ins done by another process. Does not take over legacy state.
    pub fn tokens(&self, name: &str, server_url: &str) -> Option<OAuthTokens> {
        let (key, legacy_key) = store_keys(name, server_url);
        let states = self
            .backend
            .with_lock(|current| Ok(LockResult::read(parse_states(current.as_deref())?)))
            .ok()?;
        state_of(states.get(&key).or_else(|| states.get(&legacy_key)))?.tokens
    }

    /// Returns whether credentials were stored for the server. Removes legacy state the server would take over.
    pub fn remove(&self, name: &str, server_url: &str) -> bool {
        let (key, legacy_key) = store_keys(name, server_url);
        self.backend
            .with_lock(move |current| {
                let mut states = parse_states(current.as_deref())?;
                let stored = if states.contains_key(&key) {
                    key
                } else if states.contains_key(&legacy_key) {
                    legacy_key
                } else {
                    return Ok(LockResult::read(false));
                };
                states.shift_remove(&stored);
                Ok(LockResult::write(true, serialize_states(&states)))
            })
            .unwrap_or(false)
    }
}

/// `fetch` with `OAUTH_REQUEST_TIMEOUT_MS` per request, on top of the request's own signal.
fn timed_fetch(fetch: Option<McpFetch>) -> McpFetch {
    let fetch = fetch.unwrap_or_else(default_fetch);
    Arc::new(move |mut request| {
        let timeout = AbortSignal::timeout(Duration::from_millis(OAUTH_REQUEST_TIMEOUT_MS));
        request.signal = Some(match request.signal.take() {
            Some(signal) => AbortSignal::any(&[signal, timeout]),
            None => timeout,
        });
        fetch(request)
    })
}

fn registered_redirect_urls(client: Option<&OAuthClientInformation>) -> Vec<String> {
    client.map(OAuthClientInformation::redirect_uris).unwrap_or_default()
}

/// 12 characters identifying an MCP server URL in callback paths, computed like Codex does.
fn callback_id(server_url: &str) -> String {
    use base64::Engine;
    let href = match Url::parse(server_url) {
        Ok(mut url) => {
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => server_url.to_string(),
    };
    let digest = Sha256::digest(href.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..9])
}

/// pi's Client ID Metadata Document, for `clientRegistration: "cimd"`, chosen like Codex chooses its own.
/// The configuration ensures the default callback path. Without the `iss` parameter in authorization
/// responses (RFC 9207), the redirect URI and the document are specific to the MCP server, so a response
/// cannot be mixed up with one from another authorization server (RFC 9700 section 4.4.2.2).
fn client_metadata_document(
    server_url: &str,
    redirect_url: &str,
    metadata: Option<&AuthorizationServerMetadata>,
) -> pi_mcp::Result<OAuthClientMetadataDocument> {
    let supported = metadata.is_some_and(|metadata| {
        metadata.client_id_metadata_document_supported == Some(true)
            && metadata
                .token_endpoint_auth_methods_supported
                .as_ref()
                .is_some_and(|methods| methods.iter().any(|method| method == "none"))
    });
    if !supported {
        return Err(McpError::Other(
            "The authorization server does not support Client ID Metadata Documents for public clients; remove oauth.clientRegistration \"cimd\"".to_string(),
        ));
    }
    if metadata.and_then(|metadata| metadata.authorization_response_iss_parameter_supported)
        == Some(true)
    {
        return Ok(OAuthClientMetadataDocument {
            url: format!("{CLIENT_METADATA_BASE_URL}/client.json"),
            redirect_url: redirect_url.to_string(),
        });
    }
    let id = callback_id(server_url);
    let mut redirect = Url::parse(redirect_url)
        .map_err(|_| McpError::Type(format!("Invalid URL: {redirect_url}")))?;
    redirect.set_path(&format!("{CALLBACK_PATH}/{id}"));
    Ok(OAuthClientMetadataDocument {
        url: format!("{CLIENT_METADATA_BASE_URL}/{id}/client.json"),
        redirect_url: redirect.to_string(),
    })
}

fn create_provider(
    server_url: &Url,
    store: Arc<dyn McpOAuthStateStore>,
    settings: &McpOAuthSettings,
    redirect_url: &str,
    on_redirect: impl Fn(&Url) + Send + Sync + 'static,
) -> McpOAuthProvider {
    let mut options = McpOAuthProviderOptions::new(server_url.clone(), redirect_url, move |url| {
        on_redirect(url);
        Ok(())
    });
    options.client_metadata = OAuthClientMetadata {
        client_name: Some(
            settings
                .client_name
                .clone()
                .unwrap_or_else(|| APP_NAME.to_string()),
        ),
        ..Default::default()
    };
    if settings.client_registration == Some(McpOAuthSettingsClientRegistration::Cimd) {
        let (server, redirect) = (server_url.to_string(), redirect_url.to_string());
        options.client_metadata_document = Some(Arc::new(move |metadata| {
            client_metadata_document(&server, &redirect, metadata).map(Some)
        }));
    }
    options.client_id = settings.client_id.clone();
    options.client_secret = settings.client_secret.clone();
    options.store = Some(store);
    McpOAuthProvider::new(options)
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
    /// Fails when a setting (for example the client secret) cannot be resolved.
    pub settings: Arc<dyn Fn() -> anyhow::Result<McpOAuthSettings> + Send + Sync>,
    pub on_challenge: Arc<dyn Fn(OAuthChallenge) + Send + Sync>,
}

type SharedRefresh = Shared<BoxFuture<'static, Result<(), McpError>>>;

struct OAuthAuthProvider {
    server_url: String,
    store: Arc<dyn McpOAuthServerStore>,
    settings: Arc<dyn Fn() -> anyhow::Result<McpOAuthSettings> + Send + Sync>,
    on_challenge: Arc<dyn Fn(OAuthChallenge) + Send + Sync>,
    refreshing: Arc<Mutex<Option<SharedRefresh>>>,
}

impl OAuthAuthProvider {
    /// Replace `stale_token`, the access token that expired or was rejected.
    fn refresh(
        &self,
        stale_token: Option<String>,
        fetch: Option<McpFetch>,
        challenge: Option<OAuthChallenge>,
    ) -> SharedRefresh {
        let mut refreshing = self.refreshing.lock();
        if let Some(running) = refreshing.as_ref() {
            return running.clone();
        }
        let store = self.store.clone();
        let settings = self.settings.clone();
        let server_url = self.server_url.clone();
        let slot = self.refreshing.clone();
        let locked_store = store.clone();
        let work: Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<Value>> + Send + Sync> =
            Arc::new(move || {
                let store = locked_store.clone();
                let settings = settings.clone();
                let server_url = server_url.clone();
                let stale_token = stale_token.clone();
                let fetch = fetch.clone();
                let challenge = challenge.clone();
                Box::pin(async move {
                    let state = store.load().await?;
                    let tokens = state.as_ref().and_then(|state| state.tokens.as_ref());
                    if tokens.map(|tokens| tokens.access_token.clone()) != stale_token {
                        return Ok(Value::Null);
                    }
                    if tokens.and_then(|tokens| tokens.refresh_token.as_ref()).is_none() {
                        return Err(McpError::OAuthAuthorizationRequired.into());
                    }
                    let settings = settings()?;
                    let redirect_url = callback_settings(&settings)?
                        .fixed_redirect_url
                        .or_else(|| {
                            registered_redirect_urls(
                                state.as_ref().and_then(|state| state.client_information.as_ref()),
                            )
                            .into_iter()
                            .next()
                        })
                        .unwrap_or_else(fallback_redirect_url);
                    let url = Url::parse(&server_url)?;
                    let provider =
                        create_provider(&url, store.clone(), &settings, &redirect_url, |_| {});
                    // Refreshes the tokens, or reports that a new sign-in is needed. Not cancellable: a refresh the
                    // server answered may have rotated the refresh token, so its answer must be saved.
                    let mut flow = OAuthFlowOptions::new(url);
                    flow.resource_metadata_url = challenge
                        .as_ref()
                        .and_then(|challenge| challenge.resource_metadata_url.clone());
                    flow.authorization_server_metadata_url = settings.auth_server_metadata_url()?;
                    flow.scope = challenge.as_ref().and_then(|challenge| challenge.scope.clone());
                    flow.fetch = Some(timed_fetch(fetch));
                    if authorize_mcp(&provider, &flow).await? == OAuthFlowResult::Redirect {
                        return Err(McpError::OAuthAuthorizationRequired.into());
                    }
                    Ok(Value::Null)
                })
            });
        // A task, so the refresh finishes and saves its tokens even when every caller stops waiting.
        let task = tokio::spawn(async move {
            let result = store.with_refresh_lock(work).await;
            slot.lock().take();
            result.map(|_| ()).map_err(to_mcp_error)
        });
        let future: BoxFuture<'static, Result<(), McpError>> = Box::pin(async move {
            task.await
                .unwrap_or_else(|error| Err(McpError::Other(error.to_string())))
        });
        let shared = future.shared();
        *refreshing = Some(shared.clone());
        shared
    }

    fn running(&self) -> Option<SharedRefresh> {
        self.refreshing.lock().clone()
    }
}

impl AuthProvider for OAuthAuthProvider {
    fn token(&self) -> BoxFuture<'_, pi_mcp::Result<Option<String>>> {
        Box::pin(async move {
            if let Some(running) = self.running() {
                let _ = running.await;
            }
            let state = self.store.load().await?;
            let token = state
                .as_ref()
                .and_then(|state| state.tokens.as_ref())
                .map(|tokens| tokens.access_token.clone());
            let expired = state
                .as_ref()
                .and_then(|state| state.tokens_expire_at)
                .is_some_and(|expires_at| expires_at - REFRESH_SKEW_MS <= now_ms());
            let has_refresh_token = state
                .as_ref()
                .and_then(|state| state.tokens.as_ref())
                .is_some_and(|tokens| tokens.refresh_token.is_some());
            if !expired || !has_refresh_token {
                return Ok(token);
            }
            // Failures fall through: the request goes out with the old token and a 401 decides what happens.
            let _ = self.refresh(token, None, None).await;
            Ok(self
                .store
                .load()
                .await?
                .and_then(|state| state.tokens)
                .map(|tokens| tokens.access_token))
        })
    }

    fn on_unauthorized(&self, context: UnauthorizedContext) -> Option<BoxFuture<'_, pi_mcp::Result<()>>> {
        Some(Box::pin(async move {
            let challenge =
                parse_www_authenticate(context.response.header("www-authenticate").as_deref());
            (self.on_challenge)(challenge.clone());
            // A refresh keeps the granted scope, so more scope needs a new sign-in.
            if challenge.error.as_deref() == Some("insufficient_scope") {
                return Err(McpError::OAuthAuthorizationRequired);
            }
            self.refresh(context.token, Some(context.fetch), Some(challenge))
                .await
        }))
    }
}

#[async_trait::async_trait]
impl McpAuthProvider for OAuthAuthProvider {
    async fn settled(&self) -> anyhow::Result<()> {
        if let Some(running) = self.running() {
            let _ = running.await;
        }
        Ok(())
    }
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
    Ok(Arc::new(OAuthAuthProvider {
        server_url: options.server_url.clone(),
        store: options.store.clone(),
        settings: options.settings.clone(),
        on_challenge: options.on_challenge.clone(),
        refreshing: Arc::default(),
    }))
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

impl Default for McpSignInCancelledError {
    fn default() -> Self {
        Self::new()
    }
}

impl McpSignInCancelledError {
    pub fn new() -> Self {
        Self {
            message: "Sign-in cancelled".to_string(),
        }
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

fn response_from_redirect_url(
    input: &str,
    state: &str,
    redirect_url: &Url,
) -> anyhow::Result<AuthorizationResponse> {
    let url = Url::parse(input.trim())
        .map_err(|_| anyhow::anyhow!("Expected the full redirect URL from the browser address bar"))?;
    // A server-specific redirect URI tells authorization servers apart, so it must match exactly.
    if url.origin() != redirect_url.origin() || url.path() != redirect_url.path() {
        anyhow::bail!("The redirect URL does not match this sign-in's redirect URI");
    }
    if let Some(error) = get_search_param(&url, "error") {
        anyhow::bail!(
            "{}",
            get_search_param(&url, "error_description").unwrap_or(error)
        );
    }
    if get_search_param(&url, "state").as_deref() != Some(state) {
        anyhow::bail!("The redirect URL belongs to a different sign-in");
    }
    let Some(code) = get_search_param(&url, "code").filter(|code| !code.is_empty()) else {
        anyhow::bail!("The redirect URL does not contain an authorization code");
    };
    Ok(AuthorizationResponse {
        code,
        iss: get_search_param(&url, "iss"),
    })
}

/// Wait for the browser callback or a pasted redirect URL, whichever comes first. Aborting `signal`
/// aborts the prompt, which then cancels the sign-in.
async fn wait_for_authorization_response(
    from_browser: impl std::future::Future<Output = pi_mcp::Result<pi_mcp::oauth::callback::OAuthCallback>>,
    state: &str,
    redirect_url: &Url,
    prompt: &Arc<dyn McpSignInPrompt>,
    signal: Option<&CancellationToken>,
) -> anyhow::Result<AuthorizationResponse> {
    let prompt_signal = match signal {
        Some(signal) => signal.child_token(),
        None => CancellationToken::new(),
    };
    let from_user = {
        let prompt = prompt.clone();
        let prompt_signal = prompt_signal.clone();
        async move {
            let input = prompt.prompt_for_redirect_url(prompt_signal).await?;
            match input.filter(|input| !input.trim().is_empty()) {
                None => Err(anyhow::Error::new(McpSignInCancelledError::new())),
                Some(input) => response_from_redirect_url(&input, state, redirect_url),
            }
        }
    };
    let result = tokio::select! {
        response = from_browser => response
            .map(|callback| AuthorizationResponse { code: callback.code, iss: callback.iss })
            .map_err(anyhow::Error::new),
        response = from_user => response,
    };
    // The losing side ends once the prompt is aborted or the callback server closes.
    prompt_signal.cancel();
    result
}

/// Listen on `port`, or on a free port when it is taken and not `required`.
async fn listen_for_callback(
    settings: &CallbackSettings,
    extra_paths: Vec<String>,
    port: Option<i64>,
    required: bool,
) -> anyhow::Result<OAuthCallbackServer> {
    let options = OAuthCallbackServerOptions {
        host: Some(settings.host.clone()),
        redirect_host: Some(settings.redirect_host.clone()),
        port: None,
        path: Some(settings.path.clone()),
        extra_paths,
        timeout_ms: None,
        render_page: Some(Arc::new(|page: &OAuthCallbackPage| match page {
            OAuthCallbackPage::Ok => pi_ai::utils::oauth_page::oauth_success_html(
                "Signed in to the MCP server. You may now close this page.",
            ),
            OAuthCallbackPage::Error { message, details } => {
                pi_ai::utils::oauth_page::oauth_error_html(message, details.as_deref())
            }
        })),
    };
    let first = OAuthCallbackServer::listen(OAuthCallbackServerOptions {
        port: Some(port.and_then(|port| u16::try_from(port).ok()).unwrap_or(0)),
        ..options.clone()
    })
    .await;
    match first {
        Ok(server) => Ok(server),
        Err(error) if required || port.is_none() => Err(error.into()),
        Err(_) => Ok(OAuthCallbackServer::listen(options).await?),
    }
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
    let signal = options.signal.as_ref();
    if signal.is_some_and(CancellationToken::is_cancelled) {
        return Err(McpSignInCancelledError::new().into());
    }
    let settings = &options.settings;
    let stored = options.store.load().await?;
    let step_up = options
        .challenge
        .as_ref()
        .and_then(|challenge| challenge.error.as_deref())
        == Some("insufficient_scope");
    let callback_options = callback_settings(settings)?;
    // Reuse the port of the registered redirect URI so the registered client stays valid.
    let registered = registered_redirect_urls(
        stored
            .as_ref()
            .and_then(|stored| stored.client_information.as_ref()),
    )
    .into_iter()
    .next();
    let preferred_port = callback_options.port.or_else(|| {
        registered
            .as_deref()
            .and_then(|registered| Url::parse(registered).ok())
            .and_then(|url| url.port())
            .filter(|port| *port != 0)
            .map(i64::from)
    });
    let cimd = settings.client_registration == Some(McpOAuthSettingsClientRegistration::Cimd);
    let callback = listen_for_callback(
        &callback_options,
        // The redirect URI of a server-specific Client ID Metadata Document.
        if cimd {
            vec![format!("{CALLBACK_PATH}/{}", callback_id(&options.server_url))]
        } else {
            Vec::new()
        },
        preferred_port,
        callback_options.port.is_some(),
    )
    .await?;
    let redirect_url = callback_options
        .fixed_redirect_url
        .clone()
        .unwrap_or_else(|| callback.redirect_url().to_string());
    let result = sign_in_with_callback(options, stored, step_up, cimd, &callback, &redirect_url).await;
    let _ = callback.close().await;
    match result {
        // Aborted requests fail with the signal's reason; report them as the cancellation they are.
        Err(_) if signal.is_some_and(CancellationToken::is_cancelled) => {
            Err(McpSignInCancelledError::new().into())
        }
        result => result,
    }
}

async fn sign_in_with_callback(
    options: &SignInMcpServerOptions,
    stored: Option<McpOAuthState>,
    step_up: bool,
    cimd: bool,
    callback: &OAuthCallbackServer,
    redirect_url: &str,
) -> anyhow::Result<()> {
    let settings = &options.settings;
    if let Some(stored) = &stored {
        let mut next = stored.clone();
        // Every sign-in gets a fresh `state` parameter.
        next.oauth_state = None;
        // A registered client cannot use another redirect URI, and its tokens belong to it. A Client ID
        // Metadata Document is not stored, so with one, a stored client was registered before and is replaced.
        let keep_client = settings.client_id.as_deref().is_some_and(|id| !id.is_empty())
            || if cimd {
                stored.client_information.is_none()
            } else {
                registered_redirect_urls(stored.client_information.as_ref())
                    .iter()
                    .any(|url| url == redirect_url)
            };
        if !keep_client {
            next.client_information = None;
            next.tokens = None;
            next.tokens_expire_at = None;
        }
        options.store.save(next).await?;
    }

    let authorization_url: Arc<Mutex<Option<Url>>> = Arc::default();
    let captured = authorization_url.clone();
    let server_url = Url::parse(&options.server_url)
        .map_err(|_| anyhow::anyhow!("Invalid URL: {}", options.server_url))?;
    let provider = create_provider(
        &server_url,
        options.store.clone(),
        settings,
        redirect_url,
        move |url| {
            *captured.lock() = Some(url.clone());
        },
    );
    let mut flow = OAuthFlowOptions::new(server_url);
    flow.fetch = Some(timed_fetch(None));
    flow.signal = options.signal.clone().map(AbortSignal::from);
    flow.resource_metadata_url = options
        .challenge
        .as_ref()
        .and_then(|challenge| challenge.resource_metadata_url.clone());
    flow.authorization_server_metadata_url = settings.auth_server_metadata_url()?;
    // A server asking for more scope gets it on top of the configured scope and, since the challenge
    // may list only the missing scopes, on top of the scope granted so far.
    let challenged = options
        .challenge
        .as_ref()
        .and_then(|challenge| challenge.scope.clone());
    let extra = if step_up {
        step_up_scope(
            stored
                .as_ref()
                .and_then(|stored| stored.tokens.as_ref())
                .and_then(|tokens| tokens.scope.as_deref()),
            challenged.as_deref(),
        )
    } else {
        challenged
    };
    flow.scope = merge_scopes(&[settings.scope.as_deref(), extra.as_deref()]);
    // A refresh keeps the granted scope; a server asking for more needs the browser flow.
    let mut first = flow.clone();
    first.skip_refresh = step_up;
    if authorize_mcp(&provider, &first).await? == OAuthFlowResult::Authorized {
        return Ok(());
    }
    let Some(authorization_url) = authorization_url.lock().clone() else {
        anyhow::bail!("OAuth flow did not produce an authorization URL");
    };

    let state = provider.oauth_state().await?;
    // The flow picks the redirect URI, which may be specific to the MCP server.
    let redirect = get_search_param(&authorization_url, "redirect_uri")
        .unwrap_or_else(|| redirect_url.to_string());
    let authorization_redirect_url =
        Url::parse(&redirect).map_err(|_| anyhow::anyhow!("Invalid URL: {redirect}"))?;
    // Wait for the callback before the browser can reach it.
    let from_browser =
        callback.wait_for_callback(&state, Some(authorization_redirect_url.path()))?;
    options
        .prompt
        .show_authorization_url(authorization_url.as_str());
    let response = wait_for_authorization_response(
        from_browser,
        &state,
        &authorization_redirect_url,
        &options.prompt,
        options.signal.as_ref(),
    )
    .await?;
    let mut exchange = flow;
    exchange.authorization_code = Some(response.code);
    exchange.iss = response.iss;
    authorize_mcp(&provider, &exchange).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::auth_storage::InMemoryAuthStorageBackend;

    const SERVER_URL: &str = "https://mcp.example.com/mcp";

    fn state(access_token: &str) -> McpOAuthState {
        McpOAuthState {
            tokens: Some(OAuthTokens::bearer(access_token)),
            ..McpOAuthState::new(SERVER_URL)
        }
    }

    fn stored_keys(backend: &InMemoryAuthStorageBackend) -> Vec<String> {
        let content = backend
            .with_lock(|current| Ok(LockResult::read(current.unwrap_or_else(|| "{}".into()))))
            .unwrap();
        let parsed: IndexMap<String, Value> = serde_json::from_str(&content).unwrap();
        parsed.keys().cloned().collect()
    }

    fn seed_legacy(backend: &InMemoryAuthStorageBackend) {
        let mut states = RawStates::new();
        states.insert(
            SERVER_URL.to_string(),
            serde_json::to_value(state("legacy-token")).unwrap(),
        );
        let next = serde_json::to_string(&states).unwrap();
        backend
            .with_lock(move |_| Ok(LockResult::write((), next)))
            .unwrap();
    }

    // https://github.com/earendil-works/pi/issues/10252
    #[tokio::test]
    async fn keeps_separate_credentials_for_servers_sharing_a_url() {
        let store = McpOAuthCredentialStore::new(Some(Arc::new(InMemoryAuthStorageBackend::new())), None);
        store.for_server("work", SERVER_URL).save(state("work-token")).await.unwrap();
        store
            .for_server("personal", SERVER_URL)
            .save(state("personal-token"))
            .await
            .unwrap();
        let token = |name: &'static str| {
            let store = store.for_server(name, SERVER_URL);
            async move { store.load().await.unwrap().and_then(|s| s.tokens).map(|t| t.access_token) }
        };
        assert_eq!(token("work").await.as_deref(), Some("work-token"));
        assert_eq!(token("personal").await.as_deref(), Some("personal-token"));
        assert!(store.remove("work", SERVER_URL));
        assert!(store.for_server("work", SERVER_URL).load().await.unwrap().is_none());
        assert_eq!(
            store.tokens("personal", SERVER_URL).map(|t| t.access_token).as_deref(),
            Some("personal-token")
        );
    }

    #[tokio::test]
    async fn moves_legacy_credentials_to_the_first_server_that_loads_them() {
        let backend = Arc::new(InMemoryAuthStorageBackend::new());
        seed_legacy(&backend);
        let store = McpOAuthCredentialStore::new(Some(backend.clone()), None);
        // Reading tokens does not take the legacy state over.
        assert_eq!(
            store.tokens("work", SERVER_URL).map(|t| t.access_token).as_deref(),
            Some("legacy-token")
        );
        assert_eq!(stored_keys(&backend), vec![SERVER_URL.to_string()]);
        let loaded = store.for_server("my_work", SERVER_URL).load().await.unwrap();
        assert_eq!(
            loaded.and_then(|s| s.tokens).map(|t| t.access_token).as_deref(),
            Some("legacy-token")
        );
        // Names differing only in `-` and `_` are the same server.
        assert_eq!(
            store.tokens("my-work", SERVER_URL).map(|t| t.access_token).as_deref(),
            Some("legacy-token")
        );
        assert!(store.for_server("personal", SERVER_URL).load().await.unwrap().is_none());
        assert_eq!(stored_keys(&backend), vec![format!("mcp__my_work|{SERVER_URL}")]);
    }

    #[test]
    fn signs_out_of_legacy_credentials() {
        let backend = Arc::new(InMemoryAuthStorageBackend::new());
        seed_legacy(&backend);
        let store = McpOAuthCredentialStore::new(Some(backend.clone()), None);
        assert!(store.remove("work", SERVER_URL));
        assert!(stored_keys(&backend).is_empty());
        assert!(!store.remove("work", SERVER_URL));
    }

    #[test]
    fn computes_callback_settings() {
        let settings = callback_settings(&McpOAuthSettings::default()).unwrap();
        assert_eq!(settings.host, "127.0.0.1");
        assert_eq!(settings.path, "/callback");
        assert_eq!(settings.port, None);
        assert_eq!(settings.fixed_redirect_url, None);

        let settings = callback_settings(&McpOAuthSettings {
            callback_url: Some("http://localhost:8080/oauth/callback".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(settings.host, "127.0.0.1");
        assert_eq!(settings.redirect_host, "localhost");
        assert_eq!(settings.port, Some(8080));
        assert_eq!(
            settings.fixed_redirect_url.as_deref(),
            Some("http://localhost:8080/oauth/callback")
        );

        let settings = callback_settings(&McpOAuthSettings {
            callback_url: Some("http://[::1]/cb".into()),
            callback_port: Some(9000.0),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(settings.host, "::1");
        assert_eq!(settings.fixed_redirect_url.as_deref(), Some("http://[::1]:9000/cb"));
    }

    #[test]
    fn merges_scopes_once() {
        assert_eq!(merge_scopes(&[Some("a b"), Some("b c")]).as_deref(), Some("a b c"));
        assert_eq!(merge_scopes(&[None, Some(" ")]), None);
    }

    #[test]
    fn parses_pasted_redirect_urls() {
        let redirect = Url::parse("http://127.0.0.1:5000/callback").unwrap();
        let response =
            response_from_redirect_url("http://127.0.0.1:5000/callback?code=c&state=s&iss=i", "s", &redirect)
                .unwrap();
        assert_eq!(response.code, "c");
        assert_eq!(response.iss.as_deref(), Some("i"));
        let error = |input: &str| {
            response_from_redirect_url(input, "s", &redirect)
                .unwrap_err()
                .to_string()
        };
        assert_eq!(error("nope"), "Expected the full redirect URL from the browser address bar");
        assert_eq!(
            error("http://127.0.0.1:5000/other?code=c&state=s"),
            "The redirect URL does not match this sign-in's redirect URI"
        );
        assert_eq!(error("http://127.0.0.1:5000/callback?error=denied"), "denied");
        assert_eq!(
            error("http://127.0.0.1:5000/callback?code=c&state=x"),
            "The redirect URL belongs to a different sign-in"
        );
        assert_eq!(
            error("http://127.0.0.1:5000/callback?state=s"),
            "The redirect URL does not contain an authorization code"
        );
    }

    #[test]
    fn computes_callback_ids_like_codex() {
        use base64::Engine;
        let digest = Sha256::digest(SERVER_URL.as_bytes());
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..9]);
        assert_eq!(callback_id(&format!("{SERVER_URL}#frag")), expected);
        assert_eq!(expected.len(), 12);
    }
}
