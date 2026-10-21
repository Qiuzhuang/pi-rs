//! HTTP abstraction (`McpFetch`) and the auth provider contract of the HTTP transport.

use std::sync::Arc;

use bytes::Bytes;
use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};
use once_cell::sync::Lazy;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use url::Url;

use crate::abort::AbortSignal;
use crate::error::{Error, Result};

/// An HTTP request for [`McpFetch`].
#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: String,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Option<Bytes>,
    /// Aborts the request, including reading its response body.
    pub signal: Option<AbortSignal>,
}

impl HttpRequest {
    pub fn new(method: impl Into<String>, url: Url) -> Self {
        Self {
            method: method.into(),
            url,
            headers: HeaderMap::new(),
            body: None,
            signal: None,
        }
    }

    pub fn get(url: Url) -> Self {
        Self::new("GET", url)
    }

    /// Set a header; invalid names or values are ignored.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        set_header(&mut self.headers, name, value);
        self
    }

    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn signal(mut self, signal: Option<AbortSignal>) -> Self {
        self.signal = signal;
        self
    }
}

/// Set a header like `Headers.set`; invalid names or values are ignored.
pub fn set_header(headers: &mut HeaderMap, name: &str, value: &str) -> bool {
    match (
        HeaderName::from_bytes(name.as_bytes()),
        HeaderValue::from_str(value),
    ) {
        (Ok(name), Ok(value)) => {
            headers.insert(name, value);
            true
        }
        _ => false,
    }
}

/// A streamed response body.
pub type BodyStream = BoxStream<'static, Result<Bytes>>;

/// Status and headers of a response.
#[derive(Clone, Debug)]
pub struct HttpResponseHead {
    pub status: u16,
    pub headers: HeaderMap,
}

impl HttpResponseHead {
    /// `response.headers.get(name)`.
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .get(name)
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
    }

    /// `response.ok`.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// An HTTP response from [`McpFetch`]. Dropping it cancels the body.
pub struct HttpResponse {
    pub head: HttpResponseHead,
    body: Option<BodyStream>,
}

impl std::fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpResponse")
            .field("head", &self.head)
            .finish_non_exhaustive()
    }
}

impl HttpResponse {
    pub fn new(status: u16, headers: HeaderMap, body: Option<BodyStream>) -> Self {
        Self {
            head: HttpResponseHead { status, headers },
            body,
        }
    }

    /// A response with a complete body.
    pub fn from_bytes(status: u16, headers: HeaderMap, body: impl Into<Bytes>) -> Self {
        let body: Bytes = body.into();
        Self::new(
            status,
            headers,
            Some(futures::stream::once(async move { Ok(body) }).boxed()),
        )
    }

    pub fn status(&self) -> u16 {
        self.head.status
    }

    pub fn ok(&self) -> bool {
        self.head.ok()
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.head.headers
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.head.header(name)
    }

    /// Take the body stream (`response.body`).
    pub fn take_body(&mut self) -> Option<BodyStream> {
        self.body.take()
    }

    pub async fn bytes(mut self) -> Result<Bytes> {
        let Some(mut body) = self.body.take() else {
            return Ok(Bytes::new());
        };
        let mut out = Vec::new();
        while let Some(chunk) = body.next().await {
            out.extend_from_slice(&chunk?);
        }
        Ok(Bytes::from(out))
    }

    /// `response.text()`.
    pub async fn text(self) -> Result<String> {
        let bytes = self.bytes().await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// `response.json()`.
    pub async fn json(self) -> Result<Value> {
        let text = self.text().await?;
        serde_json::from_str(&text)
            .map_err(|error| Error::Other(format!("Invalid JSON response: {error}")))
    }
}

/// `fetch`: send a request and resolve with the response head once it arrives. Network failures
/// are [`Error::Network`]; aborted requests are [`Error::Aborted`].
pub type McpFetch =
    Arc<dyn Fn(HttpRequest) -> BoxFuture<'static, Result<HttpResponse>> + Send + Sync>;

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);

/// `fetch` rejects with the signal's reason; DOMException reasons carry their name as a prefix.
fn aborted_error(signal: &AbortSignal) -> Error {
    let reason = signal.reason().unwrap_or_default();
    let message = ["AbortError: ", "TimeoutError: "]
        .iter()
        .find_map(|prefix| reason.strip_prefix(prefix))
        .unwrap_or(&reason);
    Error::Aborted(if message.is_empty() {
        "This operation was aborted".to_string()
    } else {
        message.to_string()
    })
}

/// Wrap a body stream so it fails with [`Error::Aborted`] once `signal` aborts.
pub fn abortable_body(body: BodyStream, signal: Option<AbortSignal>) -> BodyStream {
    let Some(signal) = signal else {
        return body;
    };
    async_stream::stream! {
        let mut body = body;
        loop {
            tokio::select! {
                biased;
                _ = signal.aborted() => {
                    yield Err(aborted_error(&signal));
                    break;
                }
                chunk = body.next() => match chunk {
                    Some(chunk) => yield chunk,
                    None => break,
                },
            }
        }
    }
    .boxed()
}

async fn reqwest_fetch(client: reqwest::Client, request: HttpRequest) -> Result<HttpResponse> {
    let signal = request.signal.clone();
    if let Some(signal) = &signal
        && signal.is_aborted()
    {
        return Err(aborted_error(signal));
    }
    let method = reqwest::Method::from_bytes(request.method.as_bytes())
        .map_err(|_| Error::Type(format!("'{}' is not a valid HTTP method.", request.method)))?;
    let mut builder = client
        .request(method, request.url.clone())
        .headers(request.headers);
    if let Some(body) = request.body {
        builder = builder.body(body);
    }
    let send = builder.send();
    let response = match &signal {
        Some(signal) => tokio::select! {
            biased;
            _ = signal.aborted() => return Err(aborted_error(signal)),
            response = send => response,
        },
        None => send.await,
    }
    .map_err(|error| Error::Network(format!("fetch failed: {error}")))?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(|error| Error::Network(format!("terminated: {error}"))))
        .boxed();
    Ok(HttpResponse::new(
        status,
        headers,
        Some(abortable_body(body, signal)),
    ))
}

/// The default [`McpFetch`], backed by a shared `reqwest` client (rustls, system proxy settings,
/// redirects followed).
pub fn default_fetch() -> McpFetch {
    Arc::new(|request| Box::pin(reqwest_fetch(HTTP_CLIENT.clone(), request)))
}

/// An [`McpFetch`] backed by the given `reqwest` client.
pub fn reqwest_fetch_with(client: reqwest::Client) -> McpFetch {
    Arc::new(move |request| Box::pin(reqwest_fetch(client.clone(), request)))
}

/// What the HTTP transport hands to [`AuthProvider::on_unauthorized`].
#[derive(Clone)]
pub struct UnauthorizedContext {
    /// The 401 response, or a 403 response whose challenge reports `insufficient_scope`.
    pub response: HttpResponseHead,
    pub server_url: Url,
    pub fetch: McpFetch,
    /// Access token the rejected request carried, if any. A different current token means another
    /// request already refreshed it.
    pub token: Option<String>,
}

/// Supplies bearer tokens to an MCP HTTP transport and may refresh them after a 401 response.
pub trait AuthProvider: Send + Sync {
    /// Token for the next request. May refresh it over the network first, so the transport calls
    /// it only before requests whose answer it waits for. Closing a session reuses the token of
    /// the last request.
    fn token(&self) -> BoxFuture<'_, Result<Option<String>>>;

    /// Recover from a 401 (or an `insufficient_scope` 403); the request is retried once afterwards.
    /// `None` (the default) means the provider does not handle such responses, which the transport
    /// then reports as errors.
    fn on_unauthorized(&self, _context: UnauthorizedContext) -> Option<BoxFuture<'_, Result<()>>> {
        None
    }
}

type TokenFn = dyn Fn() -> BoxFuture<'static, Result<Option<String>>> + Send + Sync;
type UnauthorizedFn = dyn Fn(UnauthorizedContext) -> BoxFuture<'static, Result<()>> + Send + Sync;

/// An [`AuthProvider`] made of closures.
#[derive(Clone)]
pub struct FnAuthProvider {
    token: Arc<TokenFn>,
    on_unauthorized: Option<Arc<UnauthorizedFn>>,
}

impl FnAuthProvider {
    pub fn new(
        token: impl Fn() -> BoxFuture<'static, Result<Option<String>>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            token: Arc::new(token),
            on_unauthorized: None,
        }
    }

    pub fn on_unauthorized(
        mut self,
        handler: impl Fn(UnauthorizedContext) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    ) -> Self {
        self.on_unauthorized = Some(Arc::new(handler));
        self
    }
}

impl AuthProvider for FnAuthProvider {
    fn token(&self) -> BoxFuture<'_, Result<Option<String>>> {
        (self.token)()
    }

    fn on_unauthorized(&self, context: UnauthorizedContext) -> Option<BoxFuture<'_, Result<()>>> {
        self.on_unauthorized
            .as_ref()
            .map(|handler| handler(context))
    }
}
