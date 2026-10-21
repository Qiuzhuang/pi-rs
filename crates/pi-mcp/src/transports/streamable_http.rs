use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures::future::BoxFuture;
use futures::stream::{Stream, StreamExt};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use reqwest::header::HeaderMap;
use serde_json::Value;
use url::Url;

use super::transport::{DEFAULT_MAX_MESSAGE_BYTES, McpTransport, McpTransportSendOptions, TransportEvents};
use crate::abort::{AbortController, AbortSignal};
use crate::auth_provider::{
    AuthProvider, BodyStream, HttpRequest, HttpResponse, HttpResponseHead, McpFetch, UnauthorizedContext,
    default_fetch, set_header,
};
use crate::error::{Error, Result};
use crate::protocol::jsonrpc::{JSON_RPC_ERROR_CODES, JsonRpcId, JsonRpcMessage, JsonRpcResponse, parse_json_rpc_message};

const MAX_ERROR_BODY_BYTES: usize = 8 * 1024;
const ERROR_MESSAGE_BODY_CHARS: usize = 500;
const DEFAULT_RECONNECT_INITIAL_DELAY_MS: u64 = 1_000;
const DEFAULT_RECONNECT_MAX_DELAY_MS: u64 = 30_000;
const DEFAULT_RECONNECT_MAX_RETRIES: u32 = 5;

/// A server-sent event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
}

/// Callbacks of [`consume_sse_stream`].
pub struct ConsumeSseOptions<'a> {
    pub max_event_bytes: Option<usize>,
    pub on_event: &'a mut (dyn FnMut(SseEvent) + Send),
    /// Called for every `id` field, including events without data (for example resumption priming events).
    pub on_id: Option<&'a mut (dyn FnMut(String) + Send)>,
    /// Called for every valid `retry` field, in milliseconds.
    pub on_retry: Option<&'a mut (dyn FnMut(u64) + Send)>,
}

struct SseParser<'a, 'b> {
    options: &'b mut ConsumeSseOptions<'a>,
    max_event_bytes: usize,
    event_name: Option<String>,
    event_id: Option<String>,
    data_lines: Vec<String>,
    /// Bytes of the pending event's data, including the "\n" joins, so events streamed as many
    /// short `data:` lines without a terminating blank line cannot grow without bound.
    data_bytes: usize,
}

impl SseParser<'_, '_> {
    fn dispatch(&mut self) {
        if self.data_lines.is_empty() {
            self.event_name = None;
            self.event_id = None;
            return;
        }
        let data = self.data_lines.join("\n");
        let event = SseEvent {
            event: self.event_name.take().filter(|name| !name.is_empty()),
            data,
            id: self.event_id.take().filter(|id| !id.is_empty()),
        };
        (self.options.on_event)(event);
        self.data_lines.clear();
        self.data_bytes = 0;
    }

    fn process_line(&mut self, raw_line: &str) -> Result<()> {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() {
            self.dispatch();
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = match line.find(':') {
            Some(colon) => (&line[..colon], &line[colon + 1..]),
            None => (line, ""),
        };
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "data" => {
                self.data_bytes += value.len() + usize::from(!self.data_lines.is_empty());
                if self.data_bytes > self.max_event_bytes {
                    return Err(Error::Other(format!("MCP SSE event exceeds {} bytes", self.max_event_bytes)));
                }
                self.data_lines.push(value.to_string());
            }
            "event" => self.event_name = Some(value.to_string()),
            "id" if !value.contains('\0') => {
                self.event_id = Some(value.to_string());
                if let Some(on_id) = self.options.on_id.as_mut() {
                    on_id(value.to_string());
                }
            }
            "retry" if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
                if let (Some(on_retry), Ok(delay)) = (self.options.on_retry.as_mut(), value.parse::<u64>()) {
                    on_retry(delay);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Parse a server-sent event stream, calling the callbacks as events arrive.
pub async fn consume_sse_stream<S>(stream: S, mut options: ConsumeSseOptions<'_>) -> Result<()>
where
    S: Stream<Item = Result<Bytes>> + Unpin,
{
    let max_event_bytes = options.max_event_bytes.unwrap_or(DEFAULT_MAX_MESSAGE_BYTES);
    let mut parser = SseParser {
        options: &mut options,
        max_event_bytes,
        event_name: None,
        event_id: None,
        data_lines: Vec::new(),
        data_bytes: 0,
    };
    let mut stream = stream;
    let mut buffered: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        buffered.extend_from_slice(&chunk?);
        while let Some(newline) = buffered.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffered.drain(..=newline).take(newline).collect();
            parser.process_line(&String::from_utf8_lossy(&line))?;
        }
        if buffered.len() > max_event_bytes {
            return Err(Error::Other(format!("MCP SSE event exceeds {max_event_bytes} bytes")));
        }
    }
    if !buffered.is_empty() {
        parser.process_line(&String::from_utf8_lossy(&buffered))?;
    }
    parser.dispatch();
    Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StreamableHttpReconnectOptions {
    /// Delay before the first reconnection attempt, unless the server sent a `retry` field. Default: 1000.
    pub initial_delay_ms: Option<u64>,
    /// Upper bound for the exponential backoff. Default: 30000.
    pub max_delay_ms: Option<u64>,
    /// Consecutive failed attempts before giving up on a stream. Default: 5.
    pub max_retries: Option<u32>,
}

#[derive(Clone)]
pub struct StreamableHttpTransportOptions {
    pub url: Url,
    /// Extra request headers, in order; later entries replace earlier ones with the same name.
    pub headers: Vec<(String, String)>,
    pub fetch: Option<McpFetch>,
    /// Open the server-to-client GET stream after initialization. Default: true.
    pub open_get_stream: bool,
    pub max_message_bytes: Option<usize>,
    pub auth_provider: Option<Arc<dyn AuthProvider>>,
    /// Reconnection of dropped SSE streams (the GET stream, and response streams that carry event IDs).
    pub reconnect: StreamableHttpReconnectOptions,
}

impl StreamableHttpTransportOptions {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            headers: Vec::new(),
            fetch: None,
            open_get_stream: true,
            max_message_bytes: None,
            auth_provider: None,
            reconnect: StreamableHttpReconnectOptions::default(),
        }
    }

    /// Parse `url` (like `new URL(url)`).
    pub fn parse(url: &str) -> Result<Self> {
        Ok(Self::new(crate::error::parse_url(url)?))
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn fetch(mut self, fetch: McpFetch) -> Self {
        self.fetch = Some(fetch);
        self
    }

    pub fn open_get_stream(mut self, open: bool) -> Self {
        self.open_get_stream = open;
        self
    }

    pub fn auth_provider(mut self, provider: Arc<dyn AuthProvider>) -> Self {
        self.auth_provider = Some(provider);
        self
    }

    pub fn reconnect(mut self, reconnect: StreamableHttpReconnectOptions) -> Self {
        self.reconnect = reconnect;
        self
    }
}

impl std::fmt::Debug for StreamableHttpTransportOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamableHttpTransportOptions")
            .field("url", &self.url.as_str())
            .field("open_get_stream", &self.open_get_stream)
            .field("auth_provider", &self.auth_provider.is_some())
            .field("reconnect", &self.reconnect)
            .finish_non_exhaustive()
    }
}

fn content_type(head: &HttpResponseHead) -> Option<String> {
    head.header("content-type")
        .and_then(|value| value.split(';').next().map(|value| value.trim().to_ascii_lowercase()))
}

static INSUFFICIENT_SCOPE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)(?:^|[\s,])error="?insufficient_scope"?"#).expect("valid regex"));

/// 401, or 403 with an `insufficient_scope` bearer challenge (step-up authorization).
fn needs_authorization(head: &HttpResponseHead) -> bool {
    match head.status {
        401 => true,
        403 => INSUFFICIENT_SCOPE.is_match(&head.header("www-authenticate").unwrap_or_default()),
        _ => false,
    }
}

/// Statuses worth retrying when a stream fails to (re)open.
fn is_transient_status(status: u16) -> bool {
    status == 408 || status == 429 || status >= 500
}

fn truncate_chars(text: &str, max: usize) -> &str {
    match text.char_indices().nth(max) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

fn describe_http_failure(status: u16, body: &str) -> String {
    let text = body.trim();
    let snippet = if text.chars().count() > ERROR_MESSAGE_BODY_CHARS {
        format!("{}...", truncate_chars(text, ERROR_MESSAGE_BODY_CHARS - 3))
    } else {
        text.to_string()
    };
    if snippet.is_empty() {
        format!("MCP HTTP request failed with status {status}")
    } else {
        format!("MCP HTTP request failed with status {status}: {snippet}")
    }
}

#[derive(Default)]
struct StreamCursor {
    last_event_id: Option<String>,
    retry_ms: Option<u64>,
    /// Whether the stream delivered any event since it was (re)opened.
    received: bool,
}

struct Inner {
    url: Url,
    options: StreamableHttpTransportOptions,
    fetch: McpFetch,
    events: TransportEvents,
    controller: AbortController,
    started: AtomicBool,
    closed: AtomicBool,
    session_id: Mutex<Option<String>>,
    protocol_version: Mutex<Option<String>>,
    get_stream_started: AtomicBool,
    /// Access token of the latest request, which `close()` reuses instead of asking the auth provider.
    last_token: Mutex<Option<String>>,
}

/// MCP Streamable HTTP transport: JSON-RPC over POST, with JSON or SSE responses, session IDs,
/// a server-to-client GET stream, stream resumption, and bearer authorization.
#[derive(Clone)]
pub struct StreamableHttpTransport {
    inner: Arc<Inner>,
}

impl StreamableHttpTransport {
    pub fn new(options: StreamableHttpTransportOptions) -> Self {
        let fetch = options.fetch.clone().unwrap_or_else(default_fetch);
        Self {
            inner: Arc::new(Inner {
                url: options.url.clone(),
                options,
                fetch,
                events: TransportEvents::default(),
                controller: AbortController::new(),
                started: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                session_id: Mutex::new(None),
                protocol_version: Mutex::new(None),
                get_stream_started: AtomicBool::new(false),
                last_token: Mutex::new(None),
            }),
        }
    }

    pub fn url(&self) -> &Url {
        &self.inner.url
    }

    pub fn options(&self) -> &StreamableHttpTransportOptions {
        &self.inner.options
    }

    pub fn session_id(&self) -> Option<String> {
        self.inner.session_id.lock().clone()
    }
}

impl Inner {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn build_headers(&self, extra: &[(&str, String)], token: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in &self.options.headers {
            set_header(&mut headers, name, value);
        }
        for (name, value) in extra {
            set_header(&mut headers, name, value);
        }
        if let Some(session_id) = self.session_id.lock().as_deref() {
            set_header(&mut headers, "Mcp-Session-Id", session_id);
        }
        if let Some(version) = self.protocol_version.lock().as_deref() {
            set_header(&mut headers, "MCP-Protocol-Version", version);
        }
        if let Some(token) = token.filter(|token| !token.is_empty()) {
            set_header(&mut headers, "Authorization", &format!("Bearer {token}"));
        }
        headers
    }

    async fn headers(&self, extra: &[(&str, String)]) -> Result<(HeaderMap, Option<String>)> {
        let token = match &self.options.auth_provider {
            Some(provider) => provider.token().await?,
            None => None,
        };
        *self.last_token.lock() = token.clone();
        let headers = self.build_headers(extra, token.as_deref());
        Ok((headers, token.filter(|token| !token.is_empty())))
    }

    /// Fetch with auth headers. A 401 (or a 403 asking for more scope) is handed to the auth
    /// provider once, and the request is retried with whatever credentials it left behind.
    ///
    /// `signal` only stops dispatch: getting a token or the authorization recovery can take a
    /// while, and a request its sender gave up on in the meantime must not reach the server. It
    /// does not abort the shared recovery, which other requests may wait for.
    async fn authorized_fetch(
        &self,
        method: &str,
        extra: &[(&str, String)],
        body: Option<Bytes>,
        signal: Option<&AbortSignal>,
    ) -> Result<HttpResponse> {
        let mut attempt = 0;
        loop {
            let (headers, token) = self.headers(extra).await?;
            if signal.is_some_and(AbortSignal::is_aborted) {
                return Err(Error::aborted());
            }
            let request = HttpRequest {
                method: method.to_string(),
                url: self.url.clone(),
                headers,
                body: body.clone(),
                signal: Some(self.controller.signal()),
            };
            let response = (self.fetch)(request).await?;
            let Some(provider) = &self.options.auth_provider else {
                return Ok(response);
            };
            if attempt > 0 || !needs_authorization(&response.head) {
                return Ok(response);
            }
            let context = UnauthorizedContext {
                response: response.head.clone(),
                server_url: self.url.clone(),
                fetch: self.fetch.clone(),
                token,
            };
            let Some(recovery) = provider.on_unauthorized(context) else {
                return Ok(response);
            };
            let result = recovery.await;
            drop(response);
            result?;
            attempt += 1;
        }
    }

    fn capture_session(&self, head: &HttpResponseHead) {
        if let Some(session_id) = head.header("mcp-session-id").filter(|id| !id.is_empty()) {
            *self.session_id.lock() = Some(session_id);
        }
    }

    async fn check_response(&self, response: HttpResponse) -> Result<HttpResponse> {
        if response.ok() {
            return Ok(response);
        }
        let head = response.head.clone();
        let text = response.text().await.unwrap_or_default();
        let body = truncate_chars(&text, MAX_ERROR_BODY_BYTES).to_string();
        if head.status == 401 {
            return Err(Error::AuthRequired { body, www_authenticate: head.header("www-authenticate") });
        }
        if head.status == 404 && self.session_id.lock().is_some() {
            return Err(Error::SessionExpired { body });
        }
        Err(Error::Http { status: head.status, message: describe_http_failure(head.status, &body), body })
    }

    async fn consume_sse(
        &self,
        stream: BodyStream,
        cursor: &mut StreamCursor,
        mut on_message: impl FnMut(&JsonRpcMessage) + Send,
    ) -> Result<()> {
        let mut last_event_id = None;
        let mut retry_ms = None;
        let mut received = false;
        let mut on_event = |event: SseEvent| {
            received = true;
            // Events without data prime resumption; other event types are not JSON-RPC.
            if event.data.trim().is_empty() || event.event.as_deref().is_some_and(|name| name != "message") {
                return;
            }
            let parsed = serde_json::from_str::<Value>(&event.data)
                .map_err(|error| Error::Other(error.to_string()))
                .and_then(parse_json_rpc_message);
            match parsed {
                Ok(message) => {
                    on_message(&message);
                    self.events.emit_message(message);
                }
                Err(error) => self.events.emit_error(error),
            }
        };
        let mut on_id = |id: String| last_event_id = Some(id);
        let mut on_retry = |delay: u64| retry_ms = Some(delay);
        let result = consume_sse_stream(
            stream,
            ConsumeSseOptions {
                max_event_bytes: Some(self.options.max_message_bytes.unwrap_or(DEFAULT_MAX_MESSAGE_BYTES)),
                on_event: &mut on_event,
                on_id: Some(&mut on_id),
                on_retry: Some(&mut on_retry),
            },
        )
        .await;
        if last_event_id.is_some() {
            cursor.last_event_id = last_event_id;
        }
        if retry_ms.is_some() {
            cursor.retry_ms = retry_ms;
        }
        if received {
            cursor.received = true;
        }
        result
    }

    /// Read the SSE stream answering one request. When the stream ends or breaks before the
    /// response arrives and the server assigned event IDs, resume it with GET and `Last-Event-ID`,
    /// as the server may close response streams at will. Otherwise only this request fails.
    async fn consume_response_stream(self: Arc<Self>, body: BodyStream, request_id: JsonRpcId) {
        let mut cursor = StreamCursor::default();
        let mut answered = false;
        let mut stream = Some(body);
        let mut failure: Option<Error> = None;
        let mut attempt: u32 = 0;
        loop {
            if let Some(body) = stream.take() {
                let result = self
                    .consume_sse(body, &mut cursor, |message| {
                        if let JsonRpcMessage::Response(response) = message
                            && response.id() == &request_id
                        {
                            answered = true;
                        }
                    })
                    .await;
                failure = result.err();
            }
            if answered || self.is_closed() {
                return;
            }
            if failure.as_ref().is_some_and(|error| !is_retryable(error)) {
                break;
            }
            if cursor.last_event_id.is_none() || attempt >= self.max_retries() {
                break;
            }
            if cursor.received {
                attempt = 0;
            }
            cursor.received = false;
            let delay = self.reconnect_delay(attempt, cursor.retry_ms);
            attempt += 1;
            if !self.sleep(delay).await {
                return;
            }
            match self.open_sse_stream(cursor.last_event_id.clone()).await {
                Ok(body) => stream = body,
                Err(error) => {
                    let retryable = is_retryable(&error);
                    failure = Some(error);
                    if !retryable {
                        break;
                    }
                }
            }
        }
        if self.is_closed() {
            return;
        }
        let reason = match &failure {
            None => "stream ended without a response".to_string(),
            Some(error) => error.to_string(),
        };
        self.events.emit_message(JsonRpcMessage::Response(JsonRpcResponse::Error {
            id: request_id,
            error: crate::protocol::jsonrpc::JsonRpcErrorObject {
                code: JSON_RPC_ERROR_CODES.internal_error,
                message: format!("MCP response stream failed: {reason}"),
                data: None,
            },
        }));
    }

    fn start_get_stream(self: &Arc<Self>) {
        if !self.options.open_get_stream || self.is_closed() || self.get_stream_started.swap(true, Ordering::SeqCst) {
            return;
        }
        tokio::spawn(self.clone().run_get_stream());
    }

    /// Keep the server-to-client stream open, reconnecting with backoff when it drops.
    async fn run_get_stream(self: Arc<Self>) {
        let mut cursor = StreamCursor::default();
        let mut attempt: u32 = 0;
        while !self.is_closed() {
            match self.open_sse_stream(cursor.last_event_id.clone()).await {
                // The server does not offer a GET stream.
                Ok(None) => return,
                Ok(Some(stream)) => {
                    let opened_at = Instant::now();
                    let result = self.consume_sse(stream, &mut cursor, |_| {}).await;
                    match result {
                        Ok(()) => {
                            // A stream that stayed up for a while counts as healthy, even if it was idle.
                            if cursor.received || opened_at.elapsed() > Duration::from_millis(self.max_delay()) {
                                attempt = 0;
                            }
                        }
                        Err(error) => {
                            if self.is_closed() {
                                return;
                            }
                            if !is_retryable(&error) {
                                self.events.emit_error(error);
                                return;
                            }
                        }
                    }
                }
                Err(error) => {
                    if self.is_closed() {
                        return;
                    }
                    if !is_retryable(&error) {
                        self.events.emit_error(error);
                        return;
                    }
                }
            }
            cursor.received = false;
            if attempt >= self.max_retries() {
                self.events.emit_error(Error::other("MCP server-to-client stream dropped and could not be reopened"));
                return;
            }
            let delay = self.reconnect_delay(attempt, cursor.retry_ms);
            attempt += 1;
            if !self.sleep(delay).await {
                return;
            }
        }
    }

    /// Open a GET SSE stream. Resolves to `None` when the server answers 405 (no GET stream).
    async fn open_sse_stream(&self, last_event_id: Option<String>) -> Result<Option<BodyStream>> {
        let mut extra = vec![("accept", "text/event-stream".to_string())];
        if let Some(id) = last_event_id {
            extra.push(("last-event-id", id));
        }
        let response = self.authorized_fetch("GET", &extra, None, None).await?;
        if response.status() == 405 {
            return Ok(None);
        }
        let mut response = self.check_response(response).await?;
        self.capture_session(&response.head);
        let kind = content_type(&response.head);
        let body = response.take_body();
        match (kind.as_deref(), body) {
            (Some("text/event-stream"), Some(body)) => Ok(Some(body)),
            _ => Err(Error::Http {
                status: response.status(),
                message: format!(
                    "Unsupported MCP GET response content type: {}",
                    kind.as_deref().unwrap_or("missing")
                ),
                body: String::new(),
            }),
        }
    }

    fn reconnect_delay(&self, attempt: u32, server_delay_ms: Option<u64>) -> u64 {
        if let Some(delay) = server_delay_ms {
            return delay;
        }
        let initial = self.options.reconnect.initial_delay_ms.unwrap_or(DEFAULT_RECONNECT_INITIAL_DELAY_MS);
        let factor = 2u64.checked_pow(attempt).unwrap_or(u64::MAX);
        initial.saturating_mul(factor).min(self.max_delay())
    }

    fn max_delay(&self) -> u64 {
        self.options.reconnect.max_delay_ms.unwrap_or(DEFAULT_RECONNECT_MAX_DELAY_MS)
    }

    fn max_retries(&self) -> u32 {
        self.options.reconnect.max_retries.unwrap_or(DEFAULT_RECONNECT_MAX_RETRIES)
    }

    /// Resolves `false` when the transport closed while waiting.
    async fn sleep(&self, ms: u64) -> bool {
        let signal = self.controller.signal();
        if signal.is_aborted() {
            return false;
        }
        tokio::select! {
            _ = signal.aborted() => false,
            _ = tokio::time::sleep(Duration::from_millis(ms)) => true,
        }
    }

    async fn send(self: Arc<Self>, message: JsonRpcMessage, signal: Option<AbortSignal>) -> Result<()> {
        if !self.started.load(Ordering::SeqCst) || self.is_closed() {
            return Err(Error::connection_closed());
        }
        let body = serde_json::to_vec(&message).map_err(|error| Error::Other(error.to_string()))?;
        let extra = [
            ("accept", "application/json, text/event-stream".to_string()),
            ("content-type", "application/json".to_string()),
        ];
        let response = self.authorized_fetch("POST", &extra, Some(Bytes::from(body)), signal.as_ref()).await?;
        let mut response = self.check_response(response).await?;
        self.capture_session(&response.head);

        let JsonRpcMessage::Request(request) = &message else {
            // Notifications and responses are acknowledged with 202 and carry no reply; ignore any body.
            drop(response);
            // The server-to-client stream may only open once the session is initialized.
            if message.method() == Some("notifications/initialized") {
                self.start_get_stream();
            }
            return Ok(());
        };
        let status = response.status();
        if status == 202 || status == 204 {
            return Err(Error::Http {
                status,
                message: format!("MCP server accepted request {} without a response", request.method),
                body: String::new(),
            });
        }
        let kind = content_type(&response.head);
        if kind.as_deref() == Some("application/json") {
            let body = response.json().await?;
            let items = match body {
                Value::Array(items) => items,
                other => vec![other],
            };
            for item in items {
                self.events.emit_message(parse_json_rpc_message(item)?);
            }
            return Ok(());
        }
        if kind.as_deref() == Some("text/event-stream")
            && let Some(body) = response.take_body()
        {
            tokio::spawn(self.clone().consume_response_stream(body, request.id.clone()));
            return Ok(());
        }
        drop(response);
        Err(Error::Http {
            status,
            message: format!("Unsupported MCP response content type: {}", kind.as_deref().unwrap_or("missing")),
            body: String::new(),
        })
    }

    async fn close(self: Arc<Self>) -> Result<()> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.controller.abort();
        let session_id = self.session_id.lock().clone();
        if self.started.load(Ordering::SeqCst) && session_id.is_some() {
            // Best effort: the session expires on the server anyway. The auth provider may refresh
            // tokens over the network, so it is not asked here and closing never waits for a refresh.
            let token = self.last_token.lock().clone();
            let request = HttpRequest {
                method: "DELETE".to_string(),
                url: self.url.clone(),
                headers: self.build_headers(&[], token.as_deref()),
                body: None,
                signal: Some(AbortSignal::timeout(Duration::from_millis(1_000))),
            };
            let delete = (self.fetch)(request);
            if let Ok(Ok(response)) = tokio::time::timeout(Duration::from_millis(1_000), delete).await {
                drop(response);
            }
        }
        self.events.emit_close();
        Ok(())
    }
}

/// Network failures and transient statuses are retried; auth, session, and protocol errors are not.
fn is_retryable(error: &Error) -> bool {
    if let Some(status) = error.http_status() {
        return is_transient_status(status);
    }
    error.is_type_error()
}

impl McpTransport for StreamableHttpTransport {
    fn start(&self) -> BoxFuture<'static, Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            if inner.started.swap(true, Ordering::SeqCst) {
                return Err(Error::other("MCP Streamable HTTP transport already started"));
            }
            if inner.is_closed() {
                return Err(Error::connection_closed());
            }
            Ok(())
        })
    }

    fn send(&self, message: JsonRpcMessage, options: McpTransportSendOptions) -> BoxFuture<'static, Result<()>> {
        Box::pin(self.inner.clone().send(message, options.signal))
    }

    fn close(&self) -> BoxFuture<'static, Result<()>> {
        Box::pin(self.inner.clone().close())
    }

    fn events(&self) -> &TransportEvents {
        &self.inner.events
    }

    fn set_protocol_version(&self, version: &str) {
        *self.inner.protocol_version.lock() = Some(version.to_string());
    }
}
