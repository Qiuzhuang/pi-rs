//! Standalone Model Context Protocol client for pi and other applications
//! (port of `@earendil-works/pi-mcp`).
//!
//! - [`McpClient`]: initialize handshake, tools (paginated `tools/list`, `tools/call`), resources,
//!   prompts, notifications, server-to-client requests (`ping`, `roots/list`), progress,
//!   timeouts and cancellation.
//! - Transports: [`StdioTransport`] (child process, newline-delimited JSON),
//!   [`StreamableHttpTransport`] (POST with JSON/SSE responses, sessions, GET stream,
//!   resumption, bearer auth through an [`AuthProvider`]), and in-memory pairs in [`testing`].
//! - [`oauth`]: discovery, the authorization code flow with PKCE, dynamic client registration,
//!   refresh, a stateful provider, and a loopback callback server.

pub mod abort;
pub mod auth_provider;
pub mod client;
pub mod error;
pub mod oauth;
pub mod protocol;
mod serde_util;
pub mod testing;
pub mod transports;

pub use abort::{AbortController, AbortSignal};
pub use auth_provider::{
    AuthProvider, BodyStream, FnAuthProvider, HttpRequest, HttpResponse, HttpResponseHead,
    McpFetch, UnauthorizedContext, default_fetch, reqwest_fetch_with,
};
pub use client::{
    ConnectionState, McpClient, McpClientOptions, McpRequestOptions, ProgressListener,
    RequestHandler, RequestHandlerContext, Roots,
};
pub use error::{Error, McpError, Result};
pub use protocol::content::{
    AudioContent, BlobResourceContents, CallToolResult, ContentAnnotations, ContentBlock,
    EmbeddedResourceContent, ImageContent, LlmContent, ResourceContents, ResourceLinkContent,
    TextContent, TextResourceContents, to_llm_content,
};
pub use protocol::jsonrpc::{
    JSON_RPC_ERROR_CODES, JsonRpcErrorCodes, JsonRpcErrorObject, JsonRpcId, JsonRpcMessage,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, is_json_rpc_notification,
    is_json_rpc_request, is_json_rpc_response, parse_json_rpc_message,
};
pub use protocol::types::{
    CancelledNotification, ClientCapabilities, GetPromptResult, Implementation, InitializeParams,
    InitializeResult, LATEST_PROTOCOL_VERSION, ListChangedCapability, ListPromptsResult,
    ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, ProgressNotification,
    Prompt, PromptArgument, PromptMessage, ReadResourceResult, Resource, ResourceTemplate,
    ResourcesCapability, Root, SUPPORTED_PROTOCOL_VERSIONS, ServerCapabilities, Tool,
    ToolAnnotations, ToolExecution,
};
pub use transports::stdio::{StderrMode, StdioTransport, StdioTransportOptions};
pub use transports::streamable_http::{
    ConsumeSseOptions, SseEvent, StreamableHttpReconnectOptions, StreamableHttpTransport,
    StreamableHttpTransportOptions, consume_sse_stream,
};
pub use transports::{
    Disposer, McpTransport, McpTransportCloseListener, McpTransportErrorListener,
    McpTransportMessageListener, McpTransportSendOptions, TransportEvents,
};
