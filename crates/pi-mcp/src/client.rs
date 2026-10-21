use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Weak};
use std::time::Duration;

use futures::future::BoxFuture;
use parking_lot::Mutex;
use serde_json::{Map, Value, json};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::abort::{AbortController, AbortSignal};
use crate::error::{Error, McpError, Result};
use crate::protocol::content::CallToolResult;
use crate::protocol::jsonrpc::{
    JSON_RPC_ERROR_CODES, JsonRpcId, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
};
use crate::protocol::types::{
    ClientCapabilities, GetPromptResult, Implementation, InitializeResult, LATEST_PROTOCOL_VERSION,
    ListChangedCapability, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, ProgressNotification,
    Prompt, ReadResourceResult, Resource, ResourceTemplate, Root, SUPPORTED_PROTOCOL_VERSIONS, ServerCapabilities, Tool,
};
use crate::transports::transport::{Disposer, Listeners, McpTransport, McpTransportSendOptions, next_listener_id};

const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 30_000;
const MAX_LIST_PAGES: usize = 1_000;

/// Connection state of an [`McpClient`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Idle,
    Connecting,
    Connected,
    Closed,
}

impl ConnectionState {
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectionState::Idle => "idle",
            ConnectionState::Connecting => "connecting",
            ConnectionState::Connected => "connected",
            ConnectionState::Closed => "closed",
        }
    }
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Context of a server-to-client request handler.
#[derive(Clone, Debug)]
pub struct RequestHandlerContext {
    /// Aborted when the server cancels the request or the connection closes.
    pub signal: AbortSignal,
}

/// Handles a server-to-client request. `Value::Null` results are sent as `{}`. Errors that are
/// [`Error::Rpc`] keep their code and data; others become internal errors.
pub type RequestHandler =
    Arc<dyn Fn(Option<Value>, RequestHandlerContext) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

type NotificationListener = dyn Fn(Option<Value>) + Send + Sync;
type RootsFn = dyn Fn() -> BoxFuture<'static, Result<Vec<Root>>> + Send + Sync;

/// Roots the client offers through `roots/list`.
#[derive(Clone)]
pub enum Roots {
    Static(Vec<Root>),
    Dynamic(Arc<RootsFn>),
}

impl std::fmt::Debug for Roots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Roots::Static(roots) => f.debug_tuple("Static").field(roots).finish(),
            Roots::Dynamic(_) => f.write_str("Dynamic"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct McpClientOptions {
    pub name: String,
    pub version: String,
    pub title: Option<String>,
    pub capabilities: Option<ClientCapabilities>,
    /// One of [`SUPPORTED_PROTOCOL_VERSIONS`]. Default: [`LATEST_PROTOCOL_VERSION`].
    pub protocol_version: Option<String>,
    /// Default request timeout. `0` disables it. Default: 30000.
    pub request_timeout_ms: Option<u64>,
    pub roots: Option<Roots>,
}

impl McpClientOptions {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            title: None,
            capabilities: None,
            protocol_version: None,
            request_timeout_ms: None,
            roots: None,
        }
    }

    pub fn request_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.request_timeout_ms = Some(timeout_ms);
        self
    }

    pub fn roots(mut self, roots: Vec<Root>) -> Self {
        self.roots = Some(Roots::Static(roots));
        self
    }
}

pub type ProgressListener = Arc<dyn Fn(ProgressNotification) + Send + Sync>;

#[derive(Clone, Default)]
pub struct McpRequestOptions {
    pub signal: Option<AbortSignal>,
    /// Overrides the client's timeout; `0` disables it. Progress notifications renew it.
    pub timeout_ms: Option<u64>,
    pub on_progress: Option<ProgressListener>,
}

impl std::fmt::Debug for McpRequestOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpRequestOptions")
            .field("signal", &self.signal)
            .field("timeout_ms", &self.timeout_ms)
            .field("on_progress", &self.on_progress.is_some())
            .finish()
    }
}

impl McpRequestOptions {
    pub fn signal(mut self, signal: impl Into<AbortSignal>) -> Self {
        self.signal = Some(signal.into());
        self
    }

    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    pub fn on_progress(mut self, listener: impl Fn(ProgressNotification) + Send + Sync + 'static) -> Self {
        self.on_progress = Some(Arc::new(listener));
        self
    }
}

struct PendingRequest {
    sender: oneshot::Sender<Result<Value>>,
    timeout_ms: u64,
    timer: Option<JoinHandle<()>>,
    abort_watch: Option<JoinHandle<()>>,
    /// Aborted when the request settles, so the transport does not dispatch (or retry) it afterwards.
    send_controller: AbortController,
    cancellable: bool,
    on_progress: Option<ProgressListener>,
    progress_token: Option<JsonRpcId>,
}

struct State {
    state: ConnectionState,
    transport: Option<Arc<dyn McpTransport>>,
    next_request_id: i64,
    server_info: Option<Implementation>,
    server_capabilities: Option<ServerCapabilities>,
    instructions: Option<String>,
    protocol_version: Option<String>,
    pending: HashMap<JsonRpcId, PendingRequest>,
    progress_requests: HashMap<JsonRpcId, JsonRpcId>,
    incoming: HashMap<JsonRpcId, AbortController>,
    disposers: Vec<Disposer>,
}

struct Inner {
    this: Weak<Inner>,
    options: McpClientOptions,
    state: Mutex<State>,
    request_handlers: Mutex<HashMap<String, (u64, RequestHandler)>>,
    notification_listeners: Mutex<HashMap<String, Vec<(u64, Arc<NotificationListener>)>>>,
    error_listeners: Listeners<dyn Fn(Error) + Send + Sync>,
    close_listeners: Listeners<dyn Fn() + Send + Sync>,
}

/// Spawn on the current Tokio runtime, if there is one.
fn spawn_detached(future: impl Future<Output = ()> + Send + 'static) -> Option<JoinHandle<()>> {
    tokio::runtime::Handle::try_current().ok().map(|handle| handle.spawn(future))
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Rpc(McpError::new(JSON_RPC_ERROR_CODES.invalid_request, message))
}

fn validate_initialize_result(value: Value) -> Result<InitializeResult> {
    let invalid_result = || invalid("Invalid MCP initialize result");
    let object = value.as_object().ok_or_else(invalid_result)?;
    let server_info = object.get("serverInfo").and_then(Value::as_object);
    let valid = object.get("protocolVersion").is_some_and(Value::is_string)
        && object.get("capabilities").is_some_and(Value::is_object)
        && server_info.is_some_and(|info| {
            info.get("name").is_some_and(Value::is_string) && info.get("version").is_some_and(Value::is_string)
        })
        && object.get("instructions").is_none_or(Value::is_string);
    if !valid {
        return Err(invalid_result());
    }
    serde_json::from_value(value).map_err(|_| invalid_result())
}

type ListPage = (Vec<Map<String, Value>>, Option<String>);

/// One page of a paginated list: the items under `key`, each checked by `is_item`.
fn validate_list_page(method: &str, key: &str, value: Value, is_item: fn(&Map<String, Value>) -> bool) -> Result<ListPage> {
    let Value::Object(mut object) = value else {
        return Err(invalid(format!("Invalid MCP {method} result")));
    };
    let Some(Value::Array(items)) = object.remove(key) else {
        return Err(invalid(format!("Invalid MCP {method} result")));
    };
    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::Object(item) if is_item(&item) => entries.push(item),
            _ => return Err(invalid(format!("Invalid entry in MCP {method} result"))),
        }
    }
    // Some servers end pagination with `null` or `""` instead of omitting the cursor.
    let next_cursor = match object.remove("nextCursor") {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor)) if cursor.is_empty() => None,
        Some(Value::String(cursor)) => Some(cursor),
        Some(_) => return Err(invalid(format!("Invalid MCP {method} cursor"))),
    };
    Ok((entries, next_cursor))
}

fn optional_string(item: &Map<String, Value>, key: &str) -> bool {
    item.get(key).is_none_or(Value::is_string)
}

fn is_tool(tool: &Map<String, Value>) -> bool {
    tool.get("name").is_some_and(Value::is_string) && tool.get("inputSchema").is_some_and(Value::is_object)
}

// `name` is required by the spec, but some servers omit it; the URI stands in.
fn is_resource(resource: &Map<String, Value>) -> bool {
    resource.get("uri").is_some_and(Value::is_string) && optional_string(resource, "name")
}

fn is_resource_template(template: &Map<String, Value>) -> bool {
    template.get("uriTemplate").is_some_and(Value::is_string) && optional_string(template, "name")
}

fn is_prompt(prompt: &Map<String, Value>) -> bool {
    prompt.get("name").is_some_and(Value::is_string)
}

fn convert<T: serde::de::DeserializeOwned>(method: &str, item: Map<String, Value>) -> Result<T> {
    serde_json::from_value(Value::Object(item)).map_err(|_| invalid(format!("Invalid entry in MCP {method} result")))
}

fn to_tool(item: Map<String, Value>) -> Result<Tool> {
    convert("tools/list", item)
}

fn to_resource(mut item: Map<String, Value>) -> Result<Resource> {
    if !item.contains_key("name")
        && let Some(uri) = item.get("uri").cloned()
    {
        item.insert("name".into(), uri);
    }
    convert("resources/list", item)
}

fn to_resource_template(mut item: Map<String, Value>) -> Result<ResourceTemplate> {
    if !item.contains_key("name")
        && let Some(uri) = item.get("uriTemplate").cloned()
    {
        item.insert("name".into(), uri);
    }
    convert("resources/templates/list", item)
}

fn to_prompt(item: Map<String, Value>) -> Result<Prompt> {
    convert("prompts/list", item)
}

fn validate_read_resource_result(value: Value) -> Result<ReadResourceResult> {
    let contents = value.get("contents").and_then(Value::as_array);
    let Some(contents) = contents.filter(|_| value.is_object()) else {
        return Err(invalid("Invalid MCP resources/read result"));
    };
    for entry in contents {
        let valid = entry.is_object()
            && entry.get("uri").is_some_and(Value::is_string)
            && (entry.get("text").is_some_and(Value::is_string) || entry.get("blob").is_some_and(Value::is_string));
        if !valid {
            return Err(invalid("Invalid contents in MCP resources/read result"));
        }
    }
    serde_json::from_value(value).map_err(|_| invalid("Invalid contents in MCP resources/read result"))
}

/// `content` is required by the spec, but servers that only return `structuredContent` omit it
/// (the SDK defaults it too). `structuredContent` may be any JSON value, as MCP 2026-07-28 allows.
fn validate_call_tool_result(value: Value) -> Result<CallToolResult> {
    let valid = value.as_object().is_some_and(|object| object.get("content").is_none_or(Value::is_array));
    if !valid {
        return Err(invalid("Invalid MCP tools/call result"));
    }
    serde_json::from_value(value).map_err(|_| invalid("Invalid MCP tools/call result"))
}

fn validate_get_prompt_result(value: Value) -> Result<GetPromptResult> {
    if !value.get("messages").is_some_and(Value::is_array) {
        return Err(invalid("Invalid MCP prompts/get result"));
    }
    serde_json::from_value(value).map_err(|_| invalid("Invalid MCP prompts/get result"))
}

/// Removes the pending request when its future is dropped before settling, cancelling it on the server.
struct PendingGuard {
    client: Weak<Inner>,
    id: JsonRpcId,
    cancellable: bool,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if let Some(client) = self.client.upgrade() {
            client.cancel_pending(&self.id, Error::aborted(), self.cancellable, Some("Request cancelled".to_string()));
        }
    }
}

/// MCP client: one connection to one server over an [`McpTransport`]. Cheap to clone; clones
/// share the connection.
#[derive(Clone)]
pub struct McpClient {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient").field("state", &self.connection_state()).finish_non_exhaustive()
    }
}

impl McpClient {
    pub fn new(options: McpClientOptions) -> Self {
        let inner = Arc::new_cyclic(|this| Inner {
            this: this.clone(),
            options,
            state: Mutex::new(State {
                state: ConnectionState::Idle,
                transport: None,
                next_request_id: 1,
                server_info: None,
                server_capabilities: None,
                instructions: None,
                protocol_version: None,
                pending: HashMap::new(),
                progress_requests: HashMap::new(),
                incoming: HashMap::new(),
                disposers: Vec::new(),
            }),
            request_handlers: Mutex::new(HashMap::new()),
            notification_listeners: Mutex::new(HashMap::new()),
            error_listeners: Listeners::default(),
            close_listeners: Listeners::default(),
        });
        let client = Self { inner };
        let _ = client.set_request_handler("ping", |_, _| Box::pin(async { Ok(json!({})) }));
        if let Some(roots) = client.inner.options.roots.clone() {
            let _ = client.set_request_handler("roots/list", move |_, _| {
                let roots = roots.clone();
                Box::pin(async move {
                    let roots = match roots {
                        Roots::Static(roots) => roots,
                        Roots::Dynamic(roots) => roots().await?,
                    };
                    Ok(json!({ "roots": roots }))
                })
            });
        }
        client
    }

    pub fn options(&self) -> &McpClientOptions {
        &self.inner.options
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.inner.state.lock().state
    }

    pub fn server_info(&self) -> Option<Implementation> {
        self.inner.state.lock().server_info.clone()
    }

    pub fn server_capabilities(&self) -> Option<ServerCapabilities> {
        self.inner.state.lock().server_capabilities.clone()
    }

    pub fn instructions(&self) -> Option<String> {
        self.inner.state.lock().instructions.clone()
    }

    pub fn protocol_version(&self) -> Option<String> {
        self.inner.state.lock().protocol_version.clone()
    }

    /// Start `transport` and run the initialize handshake. On failure the client closes.
    pub async fn connect(&self, transport: Arc<dyn McpTransport>) -> Result<InitializeResult> {
        {
            let mut state = self.inner.state.lock();
            if state.state != ConnectionState::Idle {
                return Err(Error::Other(format!("Cannot connect MCP client in {} state", state.state)));
            }
            state.state = ConnectionState::Connecting;
            state.transport = Some(transport.clone());
        }
        let events = transport.events();
        let (on_message, on_error, on_close) =
            (Arc::downgrade(&self.inner), Arc::downgrade(&self.inner), Arc::downgrade(&self.inner));
        let disposers = vec![
            events.on_message(move |message| {
                if let Some(inner) = on_message.upgrade() {
                    inner.handle_message(message);
                }
            }),
            // Transport errors are reported only. Pending requests fail when the transport closes.
            events.on_error(move |error| {
                if let Some(inner) = on_error.upgrade() {
                    inner.emit_error(error);
                }
            }),
            events.on_close(move || {
                if let Some(inner) = on_close.upgrade() {
                    inner.mark_closed(Error::connection_closed());
                }
            }),
        ];
        self.inner.state.lock().disposers = disposers;

        match self.handshake(transport).await {
            Ok(result) => Ok(result),
            Err(error) => {
                let _ = self.close().await;
                Err(error)
            }
        }
    }

    async fn handshake(&self, transport: Arc<dyn McpTransport>) -> Result<InitializeResult> {
        transport.start().await?;
        let options = &self.inner.options;
        let mut capabilities = options.capabilities.clone().unwrap_or_default();
        if options.roots.is_some() && capabilities.roots.is_none() {
            capabilities.roots = Some(ListChangedCapability::default());
        }
        let mut client_info = Map::new();
        client_info.insert("name".into(), Value::String(options.name.clone()));
        client_info.insert("version".into(), Value::String(options.version.clone()));
        if let Some(title) = &options.title {
            client_info.insert("title".into(), Value::String(title.clone()));
        }
        let mut params = Map::new();
        params.insert(
            "protocolVersion".into(),
            Value::String(options.protocol_version.clone().unwrap_or_else(|| LATEST_PROTOCOL_VERSION.to_string())),
        );
        params.insert("capabilities".into(), serde_json::to_value(&capabilities).unwrap_or_else(|_| json!({})));
        params.insert("clientInfo".into(), Value::Object(client_info));
        let response = self.inner.request_internal("initialize", Some(params), McpRequestOptions::default(), true)?.await?;
        let result = validate_initialize_result(response)?;
        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&result.protocol_version.as_str()) {
            return Err(Error::Other(format!(
                "MCP server selected unsupported protocol version {}",
                result.protocol_version
            )));
        }
        {
            let mut state = self.inner.state.lock();
            state.protocol_version = Some(result.protocol_version.clone());
            state.server_info = Some(result.server_info.clone());
            state.server_capabilities = Some(result.capabilities.clone());
            state.instructions = result.instructions.clone();
        }
        transport.set_protocol_version(&result.protocol_version);
        self.inner.notify_internal("notifications/initialized", None, true)?.await?;
        self.inner.state.lock().state = ConnectionState::Connected;
        Ok(result)
    }

    /// Send a request. The request is dispatched when this is called, not when the future is
    /// first polled. Dropping the future before it settles cancels the request.
    pub fn request(
        &self,
        method: &str,
        params: Option<Map<String, Value>>,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<Value>> + Send + 'static + use<> {
        let started = self.inner.request_internal(method, params, options, false);
        async move { started?.await }
    }

    /// Send a notification.
    pub fn notify(
        &self,
        method: &str,
        params: Option<Map<String, Value>>,
    ) -> impl Future<Output = Result<()>> + Send + 'static + use<> {
        let sent = self.inner.notify_internal(method, params, false);
        async move { sent?.await }
    }

    /// Handle server-to-client requests for `method`. The disposer removes this handler only.
    pub fn set_request_handler(
        &self,
        method: &str,
        handler: impl Fn(Option<Value>, RequestHandlerContext) -> BoxFuture<'static, Result<Value>> + Send + Sync + 'static,
    ) -> Disposer {
        let id = next_listener_id();
        self.inner.request_handlers.lock().insert(method.to_string(), (id, Arc::new(handler)));
        let (inner, method) = (Arc::downgrade(&self.inner), method.to_string());
        Disposer::new(move || {
            if let Some(inner) = inner.upgrade() {
                let mut handlers = inner.request_handlers.lock();
                if handlers.get(&method).is_some_and(|(current, _)| *current == id) {
                    handlers.remove(&method);
                }
            }
        })
    }

    /// Listen for notifications of `method`; the listener receives the params.
    pub fn on_notification(&self, method: &str, listener: impl Fn(Option<Value>) + Send + Sync + 'static) -> Disposer {
        let id = next_listener_id();
        self.inner
            .notification_listeners
            .lock()
            .entry(method.to_string())
            .or_default()
            .push((id, Arc::new(listener)));
        let (inner, method) = (Arc::downgrade(&self.inner), method.to_string());
        Disposer::new(move || {
            if let Some(inner) = inner.upgrade() {
                let mut listeners = inner.notification_listeners.lock();
                if let Some(entries) = listeners.get_mut(&method) {
                    entries.retain(|(entry, _)| *entry != id);
                    if entries.is_empty() {
                        listeners.remove(&method);
                    }
                }
            }
        })
    }

    pub fn on_error(&self, listener: impl Fn(Error) + Send + Sync + 'static) -> Disposer {
        self.inner.error_listeners.add(Arc::new(listener))
    }

    /// Called once when the connection closes, whether the transport dropped or `close()` was called.
    pub fn on_close(&self, listener: impl Fn() + Send + Sync + 'static) -> Disposer {
        self.inner.close_listeners.add(Arc::new(listener))
    }

    pub fn ping(&self, options: McpRequestOptions) -> impl Future<Output = Result<()>> + Send + 'static + use<> {
        let request = self.request("ping", None, options);
        async move { request.await.map(|_| ()) }
    }

    /// Every tool, following `nextCursor` through all pages.
    pub fn list_tools(&self, options: McpRequestOptions) -> impl Future<Output = Result<Vec<Tool>>> + Send + 'static + use<> {
        let client = self.clone();
        async move {
            client.list_all("tools/list", "tools", is_tool, options).await?.into_iter().map(to_tool).collect()
        }
    }

    /// Every resource, following `nextCursor` through all pages.
    pub fn list_resources(
        &self,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<Vec<Resource>>> + Send + 'static + use<> {
        let client = self.clone();
        async move {
            client
                .list_all("resources/list", "resources", is_resource, options)
                .await?
                .into_iter()
                .map(to_resource)
                .collect()
        }
    }

    /// One page of resources, starting at `cursor`.
    pub fn list_resources_page(
        &self,
        cursor: Option<String>,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<ListResourcesResult>> + Send + 'static + use<> {
        let page = self.list_page("resources/list", "resources", is_resource, cursor, options);
        async move {
            let (items, next_cursor) = page.await?;
            Ok(ListResourcesResult {
                resources: items.into_iter().map(to_resource).collect::<Result<_>>()?,
                next_cursor,
                meta: None,
            })
        }
    }

    /// Every resource template, following `nextCursor` through all pages.
    pub fn list_resource_templates(
        &self,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<Vec<ResourceTemplate>>> + Send + 'static + use<> {
        let client = self.clone();
        async move {
            client
                .list_all("resources/templates/list", "resourceTemplates", is_resource_template, options)
                .await?
                .into_iter()
                .map(to_resource_template)
                .collect()
        }
    }

    /// One page of resource templates, starting at `cursor`.
    pub fn list_resource_templates_page(
        &self,
        cursor: Option<String>,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult>> + Send + 'static + use<> {
        let page = self.list_page("resources/templates/list", "resourceTemplates", is_resource_template, cursor, options);
        async move {
            let (items, next_cursor) = page.await?;
            Ok(ListResourceTemplatesResult {
                resource_templates: items.into_iter().map(to_resource_template).collect::<Result<_>>()?,
                next_cursor,
                meta: None,
            })
        }
    }

    pub fn read_resource(
        &self,
        uri: &str,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<ReadResourceResult>> + Send + 'static + use<> {
        let mut params = Map::new();
        params.insert("uri".into(), Value::String(uri.to_string()));
        let request = self.request("resources/read", Some(params), options);
        async move { validate_read_resource_result(request.await?) }
    }

    /// Every prompt, following `nextCursor` through all pages. (Rust addition: the TypeScript
    /// client exposes prompts only through `request`.)
    pub fn list_prompts(
        &self,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<Vec<Prompt>>> + Send + 'static + use<> {
        let client = self.clone();
        async move {
            client.list_all("prompts/list", "prompts", is_prompt, options).await?.into_iter().map(to_prompt).collect()
        }
    }

    /// One page of prompts, starting at `cursor`.
    pub fn list_prompts_page(
        &self,
        cursor: Option<String>,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<ListPromptsResult>> + Send + 'static + use<> {
        let page = self.list_page("prompts/list", "prompts", is_prompt, cursor, options);
        async move {
            let (items, next_cursor) = page.await?;
            Ok(ListPromptsResult { prompts: items.into_iter().map(to_prompt).collect::<Result<_>>()?, next_cursor })
        }
    }

    /// `prompts/get` with string arguments.
    pub fn get_prompt(
        &self,
        name: &str,
        arguments: Option<Map<String, Value>>,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<GetPromptResult>> + Send + 'static + use<> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(name.to_string()));
        if let Some(arguments) = arguments {
            params.insert("arguments".into(), Value::Object(arguments));
        }
        let request = self.request("prompts/get", Some(params), options);
        async move { validate_get_prompt_result(request.await?) }
    }

    pub fn call_tool(
        &self,
        name: &str,
        arguments: Option<Map<String, Value>>,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<CallToolResult>> + Send + 'static + use<> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(name.to_string()));
        if let Some(arguments) = arguments {
            params.insert("arguments".into(), Value::Object(arguments));
        }
        let request = self.request("tools/call", Some(params), options);
        async move { validate_call_tool_result(request.await?) }
    }

    /// Close the connection: pending requests fail with a connection-closed error.
    pub fn close(&self) -> impl Future<Output = Result<()>> + Send + 'static + use<> {
        let transport = {
            let mut state = self.inner.state.lock();
            let disposers = std::mem::take(&mut state.disposers);
            let transport = state.transport.take();
            drop(state);
            for dispose in disposers {
                dispose.dispose();
            }
            transport
        };
        self.inner.mark_closed(Error::connection_closed());
        let close = transport.map(|transport| transport.close());
        async move {
            if let Some(close) = close {
                close.await?;
            }
            Ok(())
        }
    }

    fn list_page(
        &self,
        method: &'static str,
        key: &'static str,
        is_item: fn(&Map<String, Value>) -> bool,
        cursor: Option<String>,
        options: McpRequestOptions,
    ) -> impl Future<Output = Result<ListPage>> + Send + 'static + use<> {
        let params = cursor.map(|cursor| {
            let mut params = Map::new();
            params.insert("cursor".into(), Value::String(cursor));
            params
        });
        let request = self.request(method, params, options);
        async move { validate_list_page(method, key, request.await?, is_item) }
    }

    /// Every item of a paginated list method.
    async fn list_all(
        &self,
        method: &'static str,
        key: &'static str,
        is_item: fn(&Map<String, Value>) -> bool,
        options: McpRequestOptions,
    ) -> Result<Vec<Map<String, Value>>> {
        let mut items = Vec::new();
        let mut cursors = HashSet::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_LIST_PAGES {
            let (page, next_cursor) = self.list_page(method, key, is_item, cursor.clone(), options.clone()).await?;
            items.extend(page);
            let Some(next_cursor) = next_cursor else {
                return Ok(items);
            };
            if !cursors.insert(next_cursor.clone()) {
                return Err(Error::Other(format!("MCP {method} returned duplicate cursor: {next_cursor}")));
            }
            cursor = Some(next_cursor);
        }
        Err(Error::Other(format!("MCP {method} exceeded {MAX_LIST_PAGES} pages")))
    }
}

type RequestFuture = BoxFuture<'static, Result<Value>>;

impl Inner {
    fn arc(&self) -> Option<Arc<Inner>> {
        self.this.upgrade()
    }

    fn require_transport(&self, state: &State, allow_connecting: bool) -> Result<Arc<dyn McpTransport>> {
        match &state.transport {
            Some(transport)
                if state.state == ConnectionState::Connected
                    || (allow_connecting && state.state == ConnectionState::Connecting) =>
            {
                Ok(transport.clone())
            }
            _ => Err(Error::ConnectionClosed(format!("MCP client is {}", state.state))),
        }
    }

    /// Register and dispatch a request; the returned future resolves with its result.
    fn request_internal(
        &self,
        method: &str,
        params: Option<Map<String, Value>>,
        options: McpRequestOptions,
        allow_connecting: bool,
    ) -> Result<RequestFuture> {
        let (sender, receiver) = oneshot::channel();
        let cancellable = method != "initialize";
        let (transport, id, message, send_signal) = {
            let mut state = self.state.lock();
            let transport = self.require_transport(&state, allow_connecting)?;
            if options.signal.as_ref().is_some_and(AbortSignal::is_aborted) {
                return Err(Error::aborted());
            }
            let id = JsonRpcId::from(state.next_request_id);
            state.next_request_id += 1;
            let progress_token = options.on_progress.as_ref().map(|_| id.clone());
            let request_params = match &progress_token {
                None => params.map(Value::Object),
                Some(token) => {
                    let mut params = params.unwrap_or_default();
                    let mut meta = match params.get("_meta") {
                        Some(Value::Object(meta)) => meta.clone(),
                        _ => Map::new(),
                    };
                    meta.insert("progressToken".into(), token.to_value());
                    params.insert("_meta".into(), Value::Object(meta));
                    Some(Value::Object(params))
                }
            };
            let message = JsonRpcMessage::Request(JsonRpcRequest { id: id.clone(), method: method.to_string(), params: request_params });
            let send_controller = AbortController::new();
            let send_signal = send_controller.signal();
            let timeout_ms =
                options.timeout_ms.or(self.options.request_timeout_ms).unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS);
            // The spec forbids cancelling `initialize`.
            let abort_watch = options.signal.clone().and_then(|signal| {
                let (client, id) = (self.this.clone(), id.clone());
                spawn_detached(async move {
                    signal.aborted().await;
                    if let Some(client) = client.upgrade() {
                        let reason = signal.reason().unwrap_or_else(|| "Aborted".to_string());
                        client.cancel_pending(&id, Error::aborted(), cancellable, Some(reason));
                    }
                })
            });
            state.pending.insert(
                id.clone(),
                PendingRequest {
                    sender,
                    timeout_ms,
                    timer: None,
                    abort_watch,
                    send_controller,
                    cancellable,
                    on_progress: options.on_progress.clone(),
                    progress_token: progress_token.clone(),
                },
            );
            if let Some(token) = progress_token {
                state.progress_requests.insert(token, id.clone());
            }
            self.arm_timeout(&mut state, &id);
            (transport, id, message, send_signal)
        };
        let send = transport.send(message, McpTransportSendOptions { signal: Some(send_signal) });
        let client = self.this.clone();
        let failed_id = id.clone();
        spawn_detached(async move {
            if let Err(error) = send.await
                && let Some(client) = client.upgrade()
            {
                client.cancel_pending(&failed_id, error, false, None);
            }
        });
        let guard = PendingGuard { client: self.this.clone(), id, cancellable };
        Ok(Box::pin(async move {
            let result = receiver.await.unwrap_or_else(|_| Err(Error::connection_closed()));
            drop(guard);
            result
        }))
    }

    fn notify_internal(
        &self,
        method: &str,
        params: Option<Map<String, Value>>,
        allow_connecting: bool,
    ) -> Result<BoxFuture<'static, Result<()>>> {
        let transport = {
            let state = self.state.lock();
            self.require_transport(&state, allow_connecting)?
        };
        let message = JsonRpcMessage::Notification(JsonRpcNotification {
            method: method.to_string(),
            params: params.map(Value::Object),
        });
        Ok(transport.send(message, McpTransportSendOptions::default()))
    }

    fn handle_message(&self, message: JsonRpcMessage) {
        match message {
            JsonRpcMessage::Response(response) => self.handle_response(response),
            JsonRpcMessage::Request(request) => {
                if let Some(client) = self.arc() {
                    spawn_detached(async move { client.handle_request(request).await });
                }
            }
            JsonRpcMessage::Notification(notification) => {
                self.handle_notification(&notification.method, notification.params)
            }
        }
    }

    fn handle_response(&self, response: JsonRpcResponse) {
        let entry = {
            let mut state = self.state.lock();
            let id = response.id().clone();
            let entry = state.pending.remove(&id);
            if let Some(entry) = &entry {
                Self::remove_pending(&mut state, entry);
            }
            entry
        };
        let Some(entry) = entry else {
            self.emit_error(Error::Other(format!("Received response for unknown MCP request {}", response.id())));
            return;
        };
        let result = match response {
            JsonRpcResponse::Success { result, .. } => Ok(result),
            JsonRpcResponse::Error { error, .. } => {
                Err(Error::Rpc(McpError::with_data(error.code, error.message, error.data)))
            }
        };
        let _ = entry.sender.send(result);
    }

    async fn handle_request(self: Arc<Self>, request: JsonRpcRequest) {
        let Some(transport) = self.state.lock().transport.clone() else {
            return;
        };
        let handler = self.request_handlers.lock().get(&request.method).map(|(_, handler)| handler.clone());
        let Some(handler) = handler else {
            let response = JsonRpcMessage::error(
                request.id.clone(),
                JSON_RPC_ERROR_CODES.method_not_found,
                format!("Method not found: {}", request.method),
                None,
            );
            if let Err(error) = transport.send(response, McpTransportSendOptions::default()).await {
                self.emit_error(error);
            }
            return;
        };
        let controller = AbortController::new();
        self.state.lock().incoming.insert(request.id.clone(), controller.clone());
        let outcome = match handler(request.params, RequestHandlerContext { signal: controller.signal() }).await {
            Ok(result) => {
                let result = if result.is_null() { json!({}) } else { result };
                transport.send(JsonRpcMessage::result(request.id.clone(), result), McpTransportSendOptions::default()).await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = outcome {
            let response = match error {
                Error::Rpc(error) => JsonRpcMessage::error(request.id.clone(), error.code, error.message, error.data),
                other => {
                    JsonRpcMessage::error(request.id.clone(), JSON_RPC_ERROR_CODES.internal_error, other.to_string(), None)
                }
            };
            if let Err(send_error) = transport.send(response, McpTransportSendOptions::default()).await {
                self.emit_error(send_error);
            }
        }
        self.state.lock().incoming.remove(&request.id);
    }

    fn handle_notification(&self, method: &str, params: Option<Value>) {
        if method == "notifications/progress" {
            self.handle_progress(params.as_ref());
        } else if method == "notifications/cancelled" {
            self.handle_cancelled(params.as_ref());
        }
        let listeners: Vec<Arc<NotificationListener>> = self
            .notification_listeners
            .lock()
            .get(method)
            .map(|entries| entries.iter().map(|(_, listener)| listener.clone()).collect())
            .unwrap_or_default();
        for listener in listeners {
            listener(params.clone());
        }
    }

    fn handle_progress(&self, params: Option<&Value>) {
        let Some(params) = params.filter(|params| params.is_object()) else { return };
        let Some(token) = params.get("progressToken").and_then(JsonRpcId::from_value) else { return };
        if !params.get("progress").is_some_and(Value::is_number) {
            return;
        }
        let listener = {
            let mut state = self.state.lock();
            let Some(request_id) = state.progress_requests.get(&token).cloned() else { return };
            if !state.pending.contains_key(&request_id) {
                return;
            }
            self.arm_timeout(&mut state, &request_id);
            state.pending.get(&request_id).and_then(|entry| entry.on_progress.clone())
        };
        if let Some(listener) = listener
            && let Ok(progress) = serde_json::from_value::<ProgressNotification>(params.clone())
        {
            listener(progress);
        }
    }

    fn handle_cancelled(&self, params: Option<&Value>) {
        let Some(params) = params.filter(|params| params.is_object()) else { return };
        let Some(request_id) = params.get("requestId").and_then(JsonRpcId::from_value) else { return };
        let controller = self.state.lock().incoming.get(&request_id).cloned();
        if let Some(controller) = controller {
            match params.get("reason").and_then(Value::as_str) {
                Some(reason) => controller.abort_with(reason),
                None => controller.abort(),
            }
        }
    }

    fn arm_timeout(&self, state: &mut State, id: &JsonRpcId) {
        let Some(entry) = state.pending.get_mut(id) else { return };
        if let Some(timer) = entry.timer.take() {
            timer.abort();
        }
        if entry.timeout_ms == 0 {
            return;
        }
        let (client, id, timeout_ms, cancellable) = (self.this.clone(), id.clone(), entry.timeout_ms, entry.cancellable);
        entry.timer = spawn_detached(async move {
            tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
            if let Some(client) = client.upgrade() {
                client.cancel_pending(&id, Error::Timeout { timeout_ms }, cancellable, Some("Request timed out".into()));
            }
        });
    }

    fn cancel_pending(&self, id: &JsonRpcId, error: Error, notify_server: bool, reason: Option<String>) {
        let (entry, transport) = {
            let mut state = self.state.lock();
            let Some(entry) = state.pending.remove(id) else { return };
            Self::remove_pending(&mut state, &entry);
            (entry, state.transport.clone())
        };
        let _ = entry.sender.send(Err(error));
        if notify_server && let Some(transport) = transport {
            let mut params = Map::new();
            params.insert("requestId".into(), id.to_value());
            if let Some(reason) = reason.filter(|reason| !reason.is_empty()) {
                params.insert("reason".into(), Value::String(reason));
            }
            let send = transport.send(
                JsonRpcMessage::notification("notifications/cancelled", Some(Value::Object(params))),
                McpTransportSendOptions::default(),
            );
            let client = self.this.clone();
            spawn_detached(async move {
                if let Err(error) = send.await
                    && let Some(client) = client.upgrade()
                {
                    client.emit_error(error);
                }
            });
        }
    }

    fn remove_pending(state: &mut State, entry: &PendingRequest) {
        if let Some(timer) = &entry.timer {
            timer.abort();
        }
        if let Some(watch) = &entry.abort_watch {
            watch.abort();
        }
        if let Some(token) = &entry.progress_token {
            state.progress_requests.remove(token);
        }
        entry.send_controller.abort();
    }

    /// Idempotent: rejects in-flight requests, aborts server requests we are serving, and flips the state.
    fn mark_closed(&self, error: Error) {
        let (was_closed, pending, incoming) = {
            let mut state = self.state.lock();
            let was_closed = state.state == ConnectionState::Closed;
            state.state = ConnectionState::Closed;
            let pending: Vec<PendingRequest> = state.pending.drain().map(|(_, entry)| entry).collect();
            for entry in &pending {
                Self::remove_pending(&mut state, entry);
            }
            let incoming: Vec<AbortController> = state.incoming.drain().map(|(_, controller)| controller).collect();
            (was_closed, pending, incoming)
        };
        for entry in pending {
            let _ = entry.sender.send(Err(error.clone()));
        }
        for controller in incoming {
            controller.abort_with(error.to_string());
        }
        if was_closed {
            return;
        }
        for listener in self.close_listeners.snapshot() {
            listener();
        }
    }

    fn emit_error(&self, error: Error) {
        for listener in self.error_listeners.snapshot() {
            listener(error.clone());
        }
    }
}
