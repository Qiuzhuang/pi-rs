use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use parking_lot::Mutex;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use url::Url;

use super::flow::{InvalidateCredentials, OAuthClientMetadataDocument, OAuthClientProvider};
use super::types::{
    AuthorizationServerMetadata, OAuthClientInformation, OAuthClientMetadata, OAuthDiscoveryState, OAuthTokens,
};
use crate::error::{Error, Result};

/// Persisted OAuth state of one MCP server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOAuthState {
    pub server_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_information: Option<OAuthClientInformation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<OAuthTokens>,
    /// When the access token expires, in milliseconds since the epoch, from `expires_in` at the
    /// time it was saved.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::serde_util::opt_number")]
    pub tokens_expire_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_verifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery: Option<OAuthDiscoveryState>,
}

impl McpOAuthState {
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            client_information: None,
            tokens: None,
            tokens_expire_at: None,
            code_verifier: None,
            oauth_state: None,
            discovery: None,
        }
    }
}

/// Durable storage for [`McpOAuthState`].
#[async_trait]
pub trait McpOAuthStateStore: Send + Sync {
    async fn load(&self) -> Result<Option<McpOAuthState>>;
    async fn save(&self, state: McpOAuthState) -> Result<()>;
}

/// An in-memory [`McpOAuthStateStore`].
#[derive(Default)]
pub struct MemoryOAuthStateStore {
    value: Mutex<Option<McpOAuthState>>,
}

impl MemoryOAuthStateStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl McpOAuthStateStore for MemoryOAuthStateStore {
    async fn load(&self) -> Result<Option<McpOAuthState>> {
        Ok(self.value.lock().clone())
    }

    async fn save(&self, state: McpOAuthState) -> Result<()> {
        *self.value.lock() = Some(state);
        Ok(())
    }
}

pub type ClientMetadataDocumentFn =
    Arc<dyn Fn(Option<&AuthorizationServerMetadata>) -> Result<Option<OAuthClientMetadataDocument>> + Send + Sync>;
pub type RedirectFn = Arc<dyn Fn(&Url) -> Result<()> + Send + Sync>;

#[derive(Clone)]
pub struct McpOAuthProviderOptions {
    pub server_url: Url,
    pub redirect_url: String,
    /// `redirect_uris` defaults to `[redirect_url]` when empty.
    pub client_metadata: OAuthClientMetadata,
    /// See [`OAuthClientProvider::client_metadata_document`].
    pub client_metadata_document: Option<ClientMetadataDocumentFn>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub store: Option<Arc<dyn McpOAuthStateStore>>,
    pub on_redirect: RedirectFn,
}

impl McpOAuthProviderOptions {
    pub fn new(server_url: Url, redirect_url: impl Into<String>, on_redirect: impl Fn(&Url) -> Result<()> + Send + Sync + 'static) -> Self {
        Self {
            server_url,
            redirect_url: redirect_url.into(),
            client_metadata: OAuthClientMetadata::default(),
            client_metadata_document: None,
            client_id: None,
            client_secret: None,
            store: None,
            on_redirect: Arc::new(on_redirect),
        }
    }
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as f64).unwrap_or(0.0)
}

/// Default stateful provider for one exact MCP server URL. Applications inject durable storage if needed.
pub struct McpOAuthProvider {
    redirect_url: String,
    client_metadata: OAuthClientMetadata,
    client_metadata_document: Option<ClientMetadataDocumentFn>,
    server_url: String,
    configured_client: Option<OAuthClientInformation>,
    store: Arc<dyn McpOAuthStateStore>,
    on_redirect: RedirectFn,
    /// Serializes read-modify-write updates of the store.
    writes: tokio::sync::Mutex<()>,
}

impl McpOAuthProvider {
    pub fn new(options: McpOAuthProviderOptions) -> Self {
        let redirect_url = options.redirect_url;
        let mut client_metadata = options.client_metadata;
        if client_metadata.redirect_uris.is_empty() {
            client_metadata.redirect_uris = vec![redirect_url.clone()];
        }
        if client_metadata.grant_types.is_none() {
            client_metadata.grant_types = Some(vec!["authorization_code".into(), "refresh_token".into()]);
        }
        if client_metadata.response_types.is_none() {
            client_metadata.response_types = Some(vec!["code".into()]);
        }
        let has_secret = options.client_secret.as_deref().is_some_and(|secret| !secret.is_empty());
        if client_metadata.token_endpoint_auth_method.is_none() {
            client_metadata.token_endpoint_auth_method =
                Some(if has_secret { "client_secret_post" } else { "none" }.to_string());
        }
        let configured_client = options.client_id.filter(|id| !id.is_empty()).map(|client_id| {
            let client = OAuthClientInformation::new(client_id);
            match options.client_secret.filter(|secret| !secret.is_empty()) {
                Some(secret) => client.with_secret(secret),
                None => client,
            }
        });
        Self {
            redirect_url,
            client_metadata,
            client_metadata_document: options.client_metadata_document,
            server_url: options.server_url.to_string(),
            configured_client,
            store: options.store.unwrap_or_else(|| Arc::new(MemoryOAuthStateStore::new())),
            on_redirect: options.on_redirect,
            writes: tokio::sync::Mutex::new(()),
        }
    }

    /// The OAuth `state` parameter, created and stored on first use.
    pub async fn oauth_state(&self) -> Result<String> {
        if let Some(existing) = self.load().await?.oauth_state.filter(|state| !state.is_empty()) {
            return Ok(existing);
        }
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let state: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let value = state.clone();
        self.update(move |current| McpOAuthState { oauth_state: Some(value), ..current }).await?;
        Ok(state)
    }

    async fn load(&self) -> Result<McpOAuthState> {
        let _writes = self.writes.lock().await;
        Ok(self.own(self.store.load().await?))
    }

    async fn update(&self, update: impl FnOnce(McpOAuthState) -> McpOAuthState) -> Result<()> {
        let _writes = self.writes.lock().await;
        let current = self.own(self.store.load().await?);
        self.store.save(update(current)).await
    }

    /// Stored state for another server URL is ignored so credentials never leak across servers.
    fn own(&self, state: Option<McpOAuthState>) -> McpOAuthState {
        match state {
            Some(state) if state.server_url == self.server_url => state,
            _ => McpOAuthState::new(self.server_url.clone()),
        }
    }
}

#[async_trait]
impl OAuthClientProvider for McpOAuthProvider {
    fn redirect_url(&self) -> String {
        self.redirect_url.clone()
    }

    fn client_metadata(&self) -> OAuthClientMetadata {
        self.client_metadata.clone()
    }

    fn client_metadata_document(
        &self,
        metadata: Option<&AuthorizationServerMetadata>,
    ) -> Result<Option<OAuthClientMetadataDocument>> {
        match &self.client_metadata_document {
            Some(document) => document(metadata),
            None => Ok(None),
        }
    }

    async fn state(&self) -> Result<Option<String>> {
        self.oauth_state().await.map(Some)
    }

    async fn client_information(&self) -> Result<Option<OAuthClientInformation>> {
        if let Some(client) = &self.configured_client {
            return Ok(Some(client.clone()));
        }
        Ok(self.load().await?.client_information)
    }

    async fn save_client_information(&self, information: OAuthClientInformation) -> Result<()> {
        if self.configured_client.is_some() {
            return Ok(());
        }
        self.update(move |current| McpOAuthState { client_information: Some(information), ..current }).await
    }

    async fn tokens(&self) -> Result<Option<OAuthTokens>> {
        Ok(self.load().await?.tokens)
    }

    async fn save_tokens(&self, tokens: OAuthTokens) -> Result<()> {
        let expires_at = tokens.expires_in.map(|expires_in| now_ms() + expires_in * 1000.0);
        self.update(move |current| McpOAuthState { tokens: Some(tokens), tokens_expire_at: expires_at, ..current }).await
    }

    async fn redirect_to_authorization(&self, url: Url) -> Result<()> {
        (self.on_redirect)(&url)
    }

    async fn save_code_verifier(&self, verifier: String) -> Result<()> {
        self.update(move |current| McpOAuthState { code_verifier: Some(verifier), ..current }).await
    }

    async fn code_verifier(&self) -> Result<String> {
        self.load()
            .await?
            .code_verifier
            .filter(|verifier| !verifier.is_empty())
            .ok_or_else(|| Error::other("No OAuth PKCE code verifier is stored"))
    }

    async fn invalidate_credentials(&self, kind: InvalidateCredentials) -> Result<()> {
        self.update(move |mut next| {
            let all = kind == InvalidateCredentials::All;
            if all || kind == InvalidateCredentials::Client {
                next.client_information = None;
            }
            if all || kind == InvalidateCredentials::Tokens {
                next.tokens = None;
                next.tokens_expire_at = None;
            }
            if all || kind == InvalidateCredentials::Verifier {
                next.code_verifier = None;
            }
            if all || kind == InvalidateCredentials::Discovery {
                next.discovery = None;
            }
            if all {
                next.oauth_state = None;
            }
            next
        })
        .await
    }

    async fn save_discovery_state(&self, state: OAuthDiscoveryState) -> Result<()> {
        self.update(move |current| McpOAuthState { discovery: Some(state), ..current }).await
    }

    async fn discovery_state(&self) -> Result<Option<OAuthDiscoveryState>> {
        Ok(self.load().await?.discovery)
    }
}
