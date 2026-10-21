use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures::future::BoxFuture;
use parking_lot::Mutex;
use pi_mcp::testing::{InMemoryTransport, create_in_memory_transport_pair};
use pi_mcp::{
    AbortController, ConnectionState, Error, JsonRpcMessage, JsonRpcRequest, LATEST_PROTOCOL_VERSION, McpClient,
    McpClientOptions, McpError, McpRequestOptions, McpTransport, McpTransportSendOptions, ProgressNotification, Root,
};
use serde_json::{Map, Value, json};

type ServerHandler = Arc<dyn Fn(JsonRpcRequest) -> BoxFuture<'static, Result<Value, McpError>> + Send + Sync>;

#[derive(Clone)]
struct TestServer {
    transport: InMemoryTransport,
    messages: Arc<Mutex<Vec<Value>>>,
    handlers: Arc<Mutex<HashMap<String, ServerHandler>>>,
}

impl TestServer {
    fn set_handler(
        &self,
        method: &str,
        handler: impl Fn(JsonRpcRequest) -> BoxFuture<'static, Result<Value, McpError>> + Send + Sync + 'static,
    ) {
        self.handlers.lock().insert(method.to_string(), Arc::new(handler));
    }

    fn set_result(&self, method: &str, result: Value) {
        self.set_handler(method, move |_| {
            let result = result.clone();
            Box::pin(async move { Ok(result) })
        });
    }

    fn messages(&self) -> Vec<Value> {
        self.messages.lock().clone()
    }

    async fn send(&self, message: Value) {
        let message: JsonRpcMessage = serde_json::from_value(message).unwrap();
        self.transport.send(message, McpTransportSendOptions::default()).await.unwrap();
    }
}

fn never<T: Send + 'static>() -> BoxFuture<'static, T> {
    Box::pin(std::future::pending())
}

async fn create_server() -> (InMemoryTransport, TestServer) {
    let (client, server_transport) = create_in_memory_transport_pair();
    let server = TestServer {
        transport: server_transport.clone(),
        messages: Arc::default(),
        handlers: Arc::default(),
    };
    let listener = server.clone();
    let _ = server_transport.on_message(move |message| {
        listener.messages.lock().push(serde_json::to_value(&message).unwrap());
        let JsonRpcMessage::Request(request) = message else { return };
        let handler = listener.handlers.lock().get(&request.method).cloned();
        let transport = listener.transport.clone();
        tokio::spawn(async move {
            let id = request.id.clone();
            let outcome = match handler {
                Some(handler) => handler(request).await,
                None => Err(McpError::new(-32601, format!("Method not found: {}", request.method))),
            };
            let response = match outcome {
                Ok(result) => JsonRpcMessage::result(id, result),
                Err(error) => JsonRpcMessage::error(id, error.code, error.message, error.data),
            };
            let _ = transport.send(response, McpTransportSendOptions::default()).await;
        });
    });
    server_transport.start().await.unwrap();
    server.set_result(
        "initialize",
        json!({
            "protocolVersion": LATEST_PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": true } },
            "serverInfo": { "name": "test-server", "version": "1.0.0" },
            "instructions": "Use test tools.",
        }),
    );
    (client, server)
}

async fn connect() -> (McpClient, TestServer) {
    let (transport, server) = create_server().await;
    let client = McpClient::new(McpClientOptions::new("test-client", "2.0.0"));
    client.connect(Arc::new(transport)).await.unwrap();
    (client, server)
}

fn args(value: Value) -> Option<Map<String, Value>> {
    value.as_object().cloned()
}

async fn tick() {
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_millis(5)).await;
}

#[tokio::test]
async fn initializes_the_connection_before_exposing_server_information() {
    let (client, server) = connect().await;
    assert_eq!(client.connection_state(), ConnectionState::Connected);
    assert_eq!(client.protocol_version().as_deref(), Some(LATEST_PROTOCOL_VERSION));
    assert_eq!(serde_json::to_value(client.server_info()).unwrap(), json!({ "name": "test-server", "version": "1.0.0" }));
    assert_eq!(serde_json::to_value(client.server_capabilities()).unwrap(), json!({ "tools": { "listChanged": true } }));
    assert_eq!(client.instructions().as_deref(), Some("Use test tools."));
    tick().await;
    assert_eq!(
        server.messages(),
        vec![
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": LATEST_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "test-client", "version": "2.0.0" },
                },
            }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        ]
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn paginates_tools_and_preserves_protocol_tool_definitions() {
    let (client, server) = connect().await;
    server.set_handler("tools/list", |request| {
        let cursor = request.params.as_ref().and_then(|params| params.get("cursor")).cloned();
        Box::pin(async move {
            Ok(match cursor {
                None => json!({
                    "tools": [{ "name": "search", "description": "Search", "inputSchema": { "type": "object" } }],
                    "nextCursor": "page-2",
                }),
                Some(_) => json!({
                    "tools": [{
                        "name": "read",
                        "inputSchema": { "type": "object" },
                        "outputSchema": { "type": "object" },
                        "annotations": { "readOnlyHint": true },
                        "icons": [{ "src": "x" }],
                    }],
                    // Some servers end pagination with an empty cursor instead of omitting it.
                    "nextCursor": "",
                }),
            })
        })
    });
    let tools = client.list_tools(McpRequestOptions::default()).await.unwrap();
    assert_eq!(
        serde_json::to_value(&tools).unwrap(),
        json!([
            { "name": "search", "description": "Search", "inputSchema": { "type": "object" } },
            {
                "name": "read",
                "inputSchema": { "type": "object" },
                "outputSchema": { "type": "object" },
                "annotations": { "readOnlyHint": true },
                "icons": [{ "src": "x" }],
            },
        ])
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn rejects_duplicate_cursors() {
    let (client, server) = connect().await;
    server.set_result("tools/list", json!({ "tools": [], "nextCursor": "same" }));
    let error = client.list_tools(McpRequestOptions::default()).await.unwrap_err();
    assert_eq!(error.to_string(), "MCP tools/list returned duplicate cursor: same");
    server.set_result("tools/list", json!({ "tools": [], "nextCursor": 5 }));
    let error = client.list_tools(McpRequestOptions::default()).await.unwrap_err();
    assert_eq!(error.to_string(), "Invalid MCP tools/list cursor");
    client.close().await.unwrap();
}

#[tokio::test]
async fn lists_and_reads_resources() {
    let (client, server) = connect().await;
    server.set_handler("resources/list", |request| {
        let first = request.params.as_ref().and_then(|params| params.get("cursor")).is_none();
        Box::pin(async move {
            Ok(if first {
                json!({ "resources": [{ "uri": "file:///a", "name": "a", "mimeType": "text/plain" }], "nextCursor": "2" })
            } else {
                json!({ "resources": [{ "uri": "file:///b" }] })
            })
        })
    });
    server.set_result(
        "resources/templates/list",
        json!({ "resourceTemplates": [{ "uriTemplate": "repo://{owner}/{repo}", "name": "repo" }] }),
    );
    server.set_handler("resources/read", |request| {
        let uri = request.params.as_ref().and_then(|params| params.get("uri")).cloned().unwrap();
        Box::pin(async move { Ok(json!({ "contents": [{ "uri": uri, "text": "hello" }] })) })
    });
    let options = McpRequestOptions::default;
    // A missing name falls back to the URI.
    assert_eq!(
        serde_json::to_value(client.list_resources(options()).await.unwrap()).unwrap(),
        json!([
            { "uri": "file:///a", "name": "a", "mimeType": "text/plain" },
            { "uri": "file:///b", "name": "file:///b" },
        ])
    );
    assert_eq!(
        serde_json::to_value(client.list_resource_templates(options()).await.unwrap()).unwrap(),
        json!([{ "uriTemplate": "repo://{owner}/{repo}", "name": "repo" }])
    );
    // Single pages pass the cursor through.
    assert_eq!(
        serde_json::to_value(client.list_resources_page(None, options()).await.unwrap()).unwrap(),
        json!({ "resources": [{ "uri": "file:///a", "name": "a", "mimeType": "text/plain" }], "nextCursor": "2" })
    );
    assert_eq!(
        serde_json::to_value(client.list_resources_page(Some("2".into()), options()).await.unwrap()).unwrap(),
        json!({ "resources": [{ "uri": "file:///b", "name": "file:///b" }] })
    );
    assert_eq!(
        serde_json::to_value(client.read_resource("file:///a", options()).await.unwrap()).unwrap(),
        json!({ "contents": [{ "uri": "file:///a", "text": "hello" }] })
    );

    server.set_result("resources/read", json!({ "contents": [{ "uri": "file:///a" }] }));
    let error = client.read_resource("file:///a", options()).await.unwrap_err();
    assert_eq!(error.to_string(), "Invalid contents in MCP resources/read result");
    server.set_result("resources/list", json!({ "resources": [{ "name": "no uri" }] }));
    let error = client.list_resources(options()).await.unwrap_err();
    assert_eq!(error.to_string(), "Invalid entry in MCP resources/list result");
    client.close().await.unwrap();
}

#[tokio::test]
async fn lists_and_gets_prompts() {
    let (client, server) = connect().await;
    server.set_result(
        "prompts/list",
        json!({ "prompts": [{ "name": "review", "arguments": [{ "name": "diff", "required": true }] }] }),
    );
    server.set_handler("prompts/get", |request| {
        let params = request.params.clone().unwrap();
        Box::pin(async move {
            Ok(json!({
                "messages": [{ "role": "user", "content": { "type": "text", "text": params["arguments"]["diff"] } }]
            }))
        })
    });
    let prompts = client.list_prompts(McpRequestOptions::default()).await.unwrap();
    assert_eq!(prompts[0].name, "review");
    assert_eq!(prompts[0].arguments.as_ref().unwrap()[0].required, Some(true));
    let prompt =
        client.get_prompt("review", args(json!({ "diff": "+x" })), McpRequestOptions::default()).await.unwrap();
    assert_eq!(prompt.messages[0].content, pi_mcp::ContentBlock::text("+x"));
    client.close().await.unwrap();
}

#[tokio::test]
async fn returns_structured_tool_content_and_surfaces_json_rpc_errors() {
    let (client, server) = connect().await;
    server.set_handler("tools/call", |request| {
        let params = request.params.clone().unwrap();
        Box::pin(async move {
            if params["name"] == "fail" {
                return Err(McpError::with_data(1234, "tool failed", Some(json!({ "retryable": false }))));
            }
            Ok(json!({
                "content": [{ "type": "text", "text": "ok" }],
                "structuredContent": { "count": params["arguments"]["count"] },
            }))
        })
    });
    let result = client.call_tool("count", args(json!({ "count": 3 })), McpRequestOptions::default()).await.unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({ "content": [{ "type": "text", "text": "ok" }], "structuredContent": { "count": 3 } })
    );
    let error = client.call_tool("fail", None, McpRequestOptions::default()).await.unwrap_err();
    assert_eq!(error.name(), "McpError");
    let Error::Rpc(error) = error else { panic!("expected an McpError") };
    assert_eq!(error.code, 1234);
    assert_eq!(error.message, "tool failed");
    assert_eq!(error.data, Some(json!({ "retryable": false })));
    // Without arguments, `tools/call` carries only the name.
    tick().await;
    let calls: Vec<Value> = server.messages().into_iter().filter(|m| m["method"] == "tools/call").collect();
    assert_eq!(calls[1]["params"], json!({ "name": "fail" }));
    client.close().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn renews_the_timeout_on_progress() {
    let (client, server) = connect().await;
    let notifier = server.clone();
    server.set_handler("tools/call", move |request| {
        let token = request.params.as_ref().unwrap()["_meta"]["progressToken"].clone();
        let notifier = notifier.clone();
        Box::pin(async move {
            let progress = notifier.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(40)).await;
                progress
                    .send(json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/progress",
                        "params": { "progressToken": token, "progress": 1, "total": 2 },
                    }))
                    .await;
            });
            tokio::time::sleep(Duration::from_millis(80)).await;
            Ok(json!({ "content": [{ "type": "text", "text": "done" }] }))
        })
    });
    let progress: Arc<Mutex<Vec<ProgressNotification>>> = Arc::default();
    let seen = progress.clone();
    let options = McpRequestOptions::default().timeout_ms(50).on_progress(move |update| seen.lock().push(update));
    let result = client.call_tool("slow", Some(Map::new()), options).await.unwrap();
    assert_eq!(serde_json::to_value(&result).unwrap(), json!({ "content": [{ "type": "text", "text": "done" }] }));
    assert_eq!(
        progress.lock().iter().map(|p| serde_json::to_value(p).unwrap()).collect::<Vec<_>>(),
        vec![json!({ "progressToken": 2, "progress": 1, "total": 2 })]
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn cancels_aborted_and_timed_out_requests() {
    let (client, server) = connect().await;
    server.set_handler("tools/call", |_| never());
    let controller = AbortController::new();
    let aborted = client.call_tool("wait", Some(Map::new()), McpRequestOptions::default().signal(controller.signal()));
    controller.abort_with("stop");
    let error = aborted.await.unwrap_err();
    assert!(matches!(error, Error::Aborted(_)), "{error:?}");
    assert_eq!(error.name(), "AbortError");
    tick().await;
    assert!(server.messages().contains(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": { "requestId": 2, "reason": "stop" },
    })));

    let error = client.call_tool("wait", Some(Map::new()), McpRequestOptions::default().timeout_ms(5)).await.unwrap_err();
    assert!(matches!(error, Error::Timeout { timeout_ms: 5 }));
    assert_eq!(error.to_string(), "MCP request timed out after 5ms");
    client.close().await.unwrap();
}

#[tokio::test]
async fn dropping_a_request_future_cancels_it() {
    let (client, server) = connect().await;
    server.set_handler("tools/call", |_| never());
    let call = client.call_tool("wait", None, McpRequestOptions::default());
    drop(call);
    tick().await;
    let cancelled: Vec<Value> =
        server.messages().into_iter().filter(|m| m["method"] == "notifications/cancelled").collect();
    assert_eq!(cancelled, vec![json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": { "requestId": 2, "reason": "Request cancelled" },
    })]);
    client.close().await.unwrap();
}

#[tokio::test]
async fn reports_transport_errors_without_failing_pending_requests() {
    let (transport, server) = create_server().await;
    let client = McpClient::new(McpClientOptions::new("test-client", "1.0.0"));
    client.connect(Arc::new(transport.clone())).await.unwrap();
    let errors: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = errors.clone();
    let _ = client.on_error(move |error| seen.lock().push(error.to_string()));
    let (respond, responded) = tokio::sync::oneshot::channel::<()>();
    let responded = futures::FutureExt::shared(responded);
    server.set_handler("tools/call", move |_| {
        let responded = responded.clone();
        Box::pin(async move {
            let _ = responded.await;
            Ok(json!({ "content": [] }))
        })
    });
    let call = client.call_tool("wait", None, McpRequestOptions::default());
    tick().await;
    transport.emit_error(Error::other("stray log line"));
    respond.send(()).unwrap();
    assert_eq!(serde_json::to_value(call.await.unwrap()).unwrap(), json!({ "content": [] }));
    assert_eq!(*errors.lock(), vec!["stray log line".to_string()]);
    client.close().await.unwrap();
}

#[tokio::test]
async fn accepts_servers_that_answer_with_an_older_protocol_version() {
    let (transport, server) = create_server().await;
    server.set_result(
        "initialize",
        json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": { "name": "old-server", "version": "0.1.0" } }),
    );
    let client = McpClient::new(McpClientOptions::new("test-client", "1.0.0"));
    client.connect(Arc::new(transport)).await.unwrap();
    assert_eq!(client.protocol_version().as_deref(), Some("2024-11-05"));
    client.close().await.unwrap();

    let (transport, server) = create_server().await;
    server.set_result(
        "initialize",
        json!({ "protocolVersion": "1999-01-01", "capabilities": {}, "serverInfo": { "name": "ancient-server", "version": "0.1.0" } }),
    );
    let rejected = McpClient::new(McpClientOptions::new("test-client", "1.0.0"));
    let error = rejected.connect(Arc::new(transport)).await.unwrap_err();
    assert!(error.to_string().contains("unsupported protocol version"));
    assert_eq!(rejected.connection_state(), ConnectionState::Closed);
}

#[tokio::test]
async fn rejects_invalid_initialize_results_and_second_connects() {
    let (transport, server) = create_server().await;
    server.set_result("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {} }));
    let client = McpClient::new(McpClientOptions::new("test-client", "1.0.0"));
    let error = client.connect(Arc::new(transport)).await.unwrap_err();
    assert_eq!(error.to_string(), "Invalid MCP initialize result");
    let (transport, _server) = create_server().await;
    let error = client.connect(Arc::new(transport)).await.unwrap_err();
    assert_eq!(error.to_string(), "Cannot connect MCP client in closed state");
}

#[tokio::test]
async fn defaults_missing_tool_result_content_to_an_empty_list() {
    let (client, server) = connect().await;
    server.set_result("tools/call", json!({ "structuredContent": { "ok": true } }));
    let result = client.call_tool("structured", None, McpRequestOptions::default()).await.unwrap();
    assert_eq!(serde_json::to_value(&result).unwrap(), json!({ "content": [], "structuredContent": { "ok": true } }));
    server.set_result("tools/call", json!({ "content": "not a list" }));
    let error = client.call_tool("broken", None, McpRequestOptions::default()).await.unwrap_err();
    assert_eq!(error.to_string(), "Invalid MCP tools/call result");
    client.close().await.unwrap();
}

// #10416: MCP 2026-07-28 allows any JSON value as structured content.
#[tokio::test]
async fn accepts_structured_content_that_is_not_an_object() {
    let (client, server) = connect().await;
    for structured in [json!([{ "id": 1 }, { "id": 2 }]), json!(null), json!("text"), json!(3), json!(false)] {
        server.set_result("tools/call", json!({ "content": [], "structuredContent": structured }));
        let result = client.call_tool("structured", None, McpRequestOptions::default()).await.unwrap();
        assert_eq!(result.structured_content, Some(structured.clone()));
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({ "content": [], "structuredContent": structured })
        );
    }
    client.close().await.unwrap();
}

#[tokio::test]
async fn does_not_send_notifications_cancelled_for_a_timed_out_initialize() {
    let (transport, server) = create_server().await;
    server.set_handler("initialize", |_| never());
    let client = McpClient::new(McpClientOptions::new("test-client", "1.0.0").request_timeout_ms(5));
    let error = client.connect(Arc::new(transport)).await.unwrap_err();
    assert!(matches!(error, Error::Timeout { .. }), "{error:?}");
    tick().await;
    assert!(!server.messages().iter().any(|message| message["method"] == "notifications/cancelled"));
}

#[tokio::test]
async fn notifies_close_listeners_once_when_the_transport_drops() {
    let (client, server) = connect().await;
    let closed = Arc::new(AtomicUsize::new(0));
    let counter = closed.clone();
    let _ = client.on_close(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    server.set_handler("tools/call", |_| never());
    let pending = client.call_tool("wait", None, McpRequestOptions::default());
    tick().await;
    server.transport.close().await.unwrap();
    let error = pending.await.unwrap_err();
    assert_eq!(error.to_string(), "MCP connection closed");
    assert_eq!(client.connection_state(), ConnectionState::Closed);
    client.close().await.unwrap();
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    let error = client.call_tool("late", None, McpRequestOptions::default()).await.unwrap_err();
    assert_eq!(error.to_string(), "MCP client is closed");
}

#[tokio::test]
async fn answers_roots_list_and_dispatches_notifications() {
    let (transport, server) = create_server().await;
    let client = McpClient::new(
        McpClientOptions::new("test-client", "1.0.0").roots(vec![Root::new("file:///workspace", Some("workspace".into()))]),
    );
    client.connect(Arc::new(transport)).await.unwrap();
    let changed: Arc<Mutex<Vec<Option<Value>>>> = Arc::default();
    let seen = changed.clone();
    let _ = client.on_notification("notifications/tools/list_changed", move |params| seen.lock().push(params));
    server.send(json!({ "jsonrpc": "2.0", "id": "roots", "method": "roots/list" })).await;
    server.send(json!({ "jsonrpc": "2.0", "id": "unknown", "method": "sampling/createMessage" })).await;
    server.send(json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" })).await;
    tick().await;
    let messages = server.messages();
    assert_eq!(messages[0]["params"]["capabilities"], json!({ "roots": {} }));
    assert!(messages.contains(&json!({
        "jsonrpc": "2.0",
        "id": "roots",
        "result": { "roots": [{ "uri": "file:///workspace", "name": "workspace" }] },
    })));
    assert!(messages.contains(&json!({
        "jsonrpc": "2.0",
        "id": "unknown",
        "error": { "code": -32601, "message": "Method not found: sampling/createMessage" },
    })));
    assert_eq!(*changed.lock(), vec![None]);
    client.close().await.unwrap();
}

#[tokio::test]
async fn aborts_server_requests_the_server_cancels() {
    let (transport, server) = create_server().await;
    let client = McpClient::new(McpClientOptions::new("test-client", "1.0.0"));
    client.connect(Arc::new(transport)).await.unwrap();
    let (aborted, reason) = tokio::sync::oneshot::channel::<Option<String>>();
    let aborted = Arc::new(Mutex::new(Some(aborted)));
    let _ = client.set_request_handler("elicitation/create", move |_, context| {
        let aborted = aborted.clone();
        Box::pin(async move {
            context.signal.aborted().await;
            if let Some(sender) = aborted.lock().take() {
                let _ = sender.send(context.signal.reason());
            }
            Err(Error::other("aborted"))
        })
    });
    server.send(json!({ "jsonrpc": "2.0", "id": 7, "method": "elicitation/create" })).await;
    tick().await;
    server
        .send(json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 7, "reason": "user" } }))
        .await;
    assert_eq!(reason.await.unwrap().as_deref(), Some("user"));
    tick().await;
    assert!(server.messages().contains(&json!({
        "jsonrpc": "2.0", "id": 7, "error": { "code": -32603, "message": "aborted" }
    })));
    client.close().await.unwrap();
}
