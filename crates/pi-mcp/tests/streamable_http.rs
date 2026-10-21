mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use common::{Request, Response, listen};
use futures::StreamExt;
use futures::future::BoxFuture;
use parking_lot::Mutex;
use pi_mcp::{
    AbortController, ConsumeSseOptions, Error, FnAuthProvider, LATEST_PROTOCOL_VERSION, McpClient,
    McpClientOptions, McpRequestOptions, SseEvent, StreamableHttpReconnectOptions,
    StreamableHttpTransport, StreamableHttpTransportOptions, consume_sse_stream,
};
use serde_json::{Map, Value, json};

#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    headers: std::collections::HashMap<String, String>,
    message: Option<Value>,
}

type Requests = Arc<Mutex<Vec<Recorded>>>;

async fn start_server(
    handler: impl Fn(Request, Response, Requests) -> BoxFuture<'static, ()> + Send + Sync + 'static,
) -> (String, Requests) {
    let requests: Requests = Arc::default();
    let recorded = requests.clone();
    let origin =
        listen(move |request, response, _| handler(request, response, recorded.clone())).await;
    (format!("{origin}/mcp"), requests)
}

/// The default protocol behavior of the test server.
async fn protocol_handler(request: Request, response: Response, requests: Requests) {
    if request.method == "GET" {
        requests.lock().push(Recorded {
            method: "GET".into(),
            headers: request.headers.clone(),
            message: None,
        });
        return response.send(405, &[], "").await;
    }
    if request.method == "DELETE" {
        requests.lock().push(Recorded {
            method: "DELETE".into(),
            headers: request.headers.clone(),
            message: None,
        });
        return response.send(200, &[], "").await;
    }
    let message = request.json();
    requests.lock().push(Recorded {
        method: request.method.clone(),
        headers: request.headers.clone(),
        message: Some(message.clone()),
    });
    if message.get("id").is_none() {
        return response.send(202, &[], "").await;
    }
    if message["method"] == "initialize" {
        let body = json!({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {
                "protocolVersion": LATEST_PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "http-fixture", "version": "1.0.0" },
            },
        });
        return response
            .send(
                200,
                &[
                    ("content-type", "application/json"),
                    ("mcp-session-id", "session-1"),
                ],
                body.to_string(),
            )
            .await;
    }
    if message["method"] == "tools/list" {
        let body = json!({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": { "tools": [{ "name": "echo", "inputSchema": { "type": "object" } }] },
        });
        return response
            .send(
                200,
                &[("content-type", "application/json")],
                body.to_string(),
            )
            .await;
    }
    let mut response = response;
    response
        .start(200, &[("content-type", "text/event-stream")])
        .await;
    response.write("id: tool-result\n").await;
    let result = json!({ "jsonrpc": "2.0", "id": message["id"], "result": { "content": [{ "type": "text", "text": "hello" }] } });
    response.end(format!("data: {result}\n\n")).await;
}

fn boxed(future: impl std::future::Future<Output = ()> + Send + 'static) -> BoxFuture<'static, ()> {
    Box::pin(future)
}

fn transport(url: &str) -> StreamableHttpTransportOptions {
    StreamableHttpTransportOptions::parse(url).unwrap()
}

fn hello() -> Value {
    json!({ "content": [{ "type": "text", "text": "hello" }] })
}

async fn call(client: &McpClient, name: &str, options: McpRequestOptions) -> pi_mcp::Result<Value> {
    client
        .call_tool(name, Some(Map::new()), options)
        .await
        .map(|result| serde_json::to_value(result).unwrap())
}

#[tokio::test]
async fn parses_chunked_crlf_events_comments_ids_and_multiline_data() {
    let chunks = [
        ": keepalive\r\nid: 7\r\ndata: {\"one\":\r\n",
        "data: 1}\r\n\r\n",
    ];
    let stream = futures::stream::iter(chunks.map(|chunk| Ok(Bytes::from(chunk))));
    let mut events = Vec::new();
    let mut on_event = |event: SseEvent| events.push(event);
    consume_sse_stream(
        stream,
        ConsumeSseOptions {
            max_event_bytes: None,
            on_event: &mut on_event,
            on_id: None,
            on_retry: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        events,
        vec![SseEvent {
            event: None,
            data: "{\"one\":\n1}".into(),
            id: Some("7".into())
        }]
    );
}

#[tokio::test]
async fn rejects_events_whose_data_lines_exceed_the_limit_without_a_blank_line() {
    let sent = Arc::new(AtomicUsize::new(0));
    let counter = sent.clone();
    // Never sends a blank line, so the event is never dispatched.
    let stream = futures::stream::unfold((), move |()| {
        let counter = counter.clone();
        async move {
            if counter.fetch_add(1, Ordering::SeqCst) > 1000 {
                None
            } else {
                Some((Ok(Bytes::from_static(b"data: xxxxxxxxxxxxxxxx\n")), ()))
            }
        }
    })
    .boxed();
    let mut on_event = |_: SseEvent| {};
    let error = consume_sse_stream(
        stream,
        ConsumeSseOptions {
            max_event_bytes: Some(256),
            on_event: &mut on_event,
            on_id: None,
            on_retry: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "MCP SSE event exceeds 256 bytes");
    assert!(sent.load(Ordering::SeqCst) < 100);
}

#[tokio::test]
async fn handles_json_and_sse_responses_with_session_and_protocol_headers() {
    let (url, requests) = start_server(|request, response, requests| {
        boxed(protocol_handler(request, response, requests))
    })
    .await;
    let http = StreamableHttpTransport::new(transport(&url));
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    client.connect(Arc::new(http.clone())).await.unwrap();
    assert_eq!(http.session_id().as_deref(), Some("session-1"));
    assert_eq!(
        serde_json::to_value(
            client
                .list_tools(McpRequestOptions::default())
                .await
                .unwrap()
        )
        .unwrap(),
        json!([{ "name": "echo", "inputSchema": { "type": "object" } }])
    );
    let result = client
        .call_tool(
            "echo",
            json!({ "text": "hello" }).as_object().cloned(),
            McpRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(result).unwrap(), hello());
    client.close().await.unwrap();

    let requests = requests.lock().clone();
    let list = requests
        .iter()
        .find(|entry| {
            entry
                .message
                .as_ref()
                .is_some_and(|m| m["method"] == "tools/list")
        })
        .unwrap();
    assert_eq!(
        list.headers.get("mcp-session-id").map(String::as_str),
        Some("session-1")
    );
    assert_eq!(
        list.headers.get("mcp-protocol-version").map(String::as_str),
        Some(LATEST_PROTOCOL_VERSION)
    );
    assert_eq!(
        list.headers.get("accept").map(String::as_str),
        Some("application/json, text/event-stream")
    );
    assert!(requests.iter().any(|entry| entry.method == "GET"));
    assert!(requests.iter().any(|entry| entry.method == "DELETE"));
}

#[tokio::test]
async fn classifies_authentication_failures() {
    let (url, _) = start_server(|_, response, _| {
        boxed(async move {
            response
                .send(
                    401,
                    &[(
                        "www-authenticate",
                        "Bearer resource_metadata=\"https://example.com/meta\"",
                    )],
                    "login required",
                )
                .await
        })
    })
    .await;
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    let error = client
        .connect(Arc::new(StreamableHttpTransport::new(transport(&url))))
        .await
        .unwrap_err();
    assert_eq!(error.name(), "McpAuthRequiredError");
    assert_eq!(error.http_status(), Some(401));
    let Error::AuthRequired {
        body,
        www_authenticate,
    } = error
    else {
        panic!("expected AuthRequired")
    };
    assert_eq!(body, "login required");
    assert_eq!(
        www_authenticate.as_deref(),
        Some("Bearer resource_metadata=\"https://example.com/meta\"")
    );
}

#[tokio::test]
async fn fails_only_the_request_whose_sse_stream_breaks() {
    let (release, gate) = tokio::sync::oneshot::channel::<()>();
    let gate = futures::FutureExt::shared(gate);
    let (url, _) = start_server(move |request, mut response, requests| {
        let gate = gate.clone();
        boxed(async move {
            if request.method != "POST" {
                return protocol_handler(request, response, requests).await;
            }
            let name = request.json()["params"]["name"]
                .as_str()
                .map(str::to_string);
            if name.as_deref() == Some("broken") {
                response
                    .start(200, &[("content-type", "text/event-stream")])
                    .await;
                return response.end("data: not json\n\n").await;
            }
            if name.as_deref() == Some("slow") {
                let _ = gate.await;
            }
            protocol_handler(request, response, requests).await
        })
    })
    .await;
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    let errors: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = errors.clone();
    let _ = client.on_error(move |error| seen.lock().push(error.to_string()));
    client
        .connect(Arc::new(StreamableHttpTransport::new(
            transport(&url).open_get_stream(false),
        )))
        .await
        .unwrap();
    let slow = tokio::spawn({
        let client = client.clone();
        async move { call(&client, "slow", McpRequestOptions::default()).await }
    });
    let error = call(&client, "broken", McpRequestOptions::default())
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("MCP response stream failed"),
        "{error}"
    );
    release.send(()).unwrap();
    assert_eq!(slow.await.unwrap().unwrap(), hello());
    assert_eq!(errors.lock().len(), 1);
    client.close().await.unwrap();
}

#[tokio::test]
async fn opens_the_get_stream_after_initialization_and_sends_last_event_id_only_when_resuming() {
    let order: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = order.clone();
    let (url, requests) = start_server(move |request, response, requests| {
        let log = log.clone();
        boxed(async move {
            if request.method == "POST" {
                log.lock().push(
                    request.json()["method"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                );
            } else {
                log.lock().push(request.method.clone());
            }
            protocol_handler(request, response, requests).await
        })
    })
    .await;
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    client
        .connect(Arc::new(StreamableHttpTransport::new(transport(&url))))
        .await
        .unwrap();
    call(&client, "echo", McpRequestOptions::default())
        .await
        .unwrap();
    client
        .list_tools(McpRequestOptions::default())
        .await
        .unwrap();
    client.close().await.unwrap();
    let order = order.lock().clone();
    let get = order
        .iter()
        .position(|entry| entry == "GET")
        .expect("GET stream opened");
    let initialized = order
        .iter()
        .position(|entry| entry == "notifications/initialized")
        .unwrap();
    assert!(get > initialized, "{order:?}");
    assert!(
        requests
            .lock()
            .iter()
            .all(|entry| !entry.headers.contains_key("last-event-id"))
    );
}

#[tokio::test]
async fn resumes_a_response_stream_the_server_closed_before_answering() {
    let resume_headers: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = resume_headers.clone();
    let (url, _) = start_server(move |request, mut response, requests| {
        let seen = seen.clone();
        boxed(async move {
            if request.method == "GET"
                && let Some(last) = request.header("last-event-id")
            {
                seen.lock().push(last.to_string());
                response.start(200, &[("content-type", "text/event-stream")]).await;
                let result = json!({ "jsonrpc": "2.0", "id": 2, "result": { "content": [{ "type": "text", "text": "resumed" }] } });
                return response.end(format!("id: 2\ndata: {result}\n\n")).await;
            }
            if request.method != "POST" || request.json()["method"] != "tools/call" {
                return protocol_handler(request, response, requests).await;
            }
            // Priming event (ID, no data) and a retry hint, then the server drops the stream.
            response.start(200, &[("content-type", "text/event-stream")]).await;
            response.end("id: 1\nretry: 5\ndata:\n\n").await
        })
    })
    .await;
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    let errors: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = errors.clone();
    let _ = client.on_error(move |error| seen.lock().push(error.to_string()));
    client
        .connect(Arc::new(StreamableHttpTransport::new(
            transport(&url).open_get_stream(false),
        )))
        .await
        .unwrap();
    assert_eq!(
        call(&client, "echo", McpRequestOptions::default())
            .await
            .unwrap(),
        json!({ "content": [{ "type": "text", "text": "resumed" }] })
    );
    assert_eq!(*resume_headers.lock(), vec!["1".to_string()]);
    assert!(errors.lock().is_empty());
    client.close().await.unwrap();
}

#[tokio::test]
async fn fails_a_request_whose_response_stream_ends_without_an_answer() {
    let (url, _) = start_server(|request, mut response, requests| {
        boxed(async move {
            if request.method != "POST" || request.json()["method"] != "tools/call" {
                return protocol_handler(request, response, requests).await;
            }
            response
                .start(200, &[("content-type", "text/event-stream")])
                .await;
            response.end(": nothing here\n\n").await
        })
    })
    .await;
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    client
        .connect(Arc::new(StreamableHttpTransport::new(
            transport(&url).open_get_stream(false),
        )))
        .await
        .unwrap();
    let error = call(
        &client,
        "echo",
        McpRequestOptions::default().timeout_ms(5_000),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "MCP response stream failed: stream ended without a response"
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn reconnects_the_get_stream_after_it_drops() {
    let gets = Arc::new(AtomicUsize::new(0));
    let last_event_ids: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let (counter, ids) = (gets.clone(), last_event_ids.clone());
    let (url, _) = start_server(move |request, mut response, requests| {
        let (counter, ids) = (counter.clone(), ids.clone());
        boxed(async move {
            if request.method != "GET" {
                return protocol_handler(request, response, requests).await;
            }
            let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
            ids.lock()
                .push(request.header("last-event-id").map(str::to_string));
            response
                .start(200, &[("content-type", "text/event-stream")])
                .await;
            let notification =
                json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" });
            if count == 1 {
                return response
                    .end(format!("id: g1\ndata: {notification}\n\n"))
                    .await;
            }
            response
                .write(format!("id: g2\ndata: {notification}\n\n"))
                .await;
            response.hold().await
        })
    })
    .await;
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    let changes = Arc::new(AtomicUsize::new(0));
    let (second, second_change) = tokio::sync::oneshot::channel::<()>();
    let second = Mutex::new(Some(second));
    let counter = changes.clone();
    let _ = client.on_notification("notifications/tools/list_changed", move |_| {
        if counter.fetch_add(1, Ordering::SeqCst) + 1 == 2
            && let Some(second) = second.lock().take()
        {
            let _ = second.send(());
        }
    });
    let options = transport(&url).reconnect(StreamableHttpReconnectOptions {
        initial_delay_ms: Some(1),
        ..Default::default()
    });
    client
        .connect(Arc::new(StreamableHttpTransport::new(options)))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), second_change)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*last_event_ids.lock(), vec![None, Some("g1".to_string())]);
    client.close().await.unwrap();
}

#[tokio::test]
async fn rejects_a_request_the_server_accepts_without_a_response() {
    let (url, _) = start_server(|request, response, requests| {
        boxed(async move {
            if request.method != "POST" || request.json()["method"] != "tools/call" {
                return protocol_handler(request, response, requests).await;
            }
            response.send(202, &[], "").await
        })
    })
    .await;
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    client
        .connect(Arc::new(StreamableHttpTransport::new(
            transport(&url).open_get_stream(false),
        )))
        .await
        .unwrap();
    let error = call(&client, "echo", McpRequestOptions::default())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("without a response"), "{error}");
    client.close().await.unwrap();
}

#[tokio::test]
async fn includes_the_response_body_in_http_errors() {
    let (url, _) = start_server(|_, response, _| {
        boxed(async move { response.send(400, &[], "Invalid Accept header").await })
    })
    .await;
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    let error = client
        .connect(Arc::new(StreamableHttpTransport::new(transport(&url))))
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "MCP HTTP request failed with status 400: Invalid Accept header"
    );
    assert_eq!(error.http_body(), Some("Invalid Accept header"));
}

#[tokio::test]
async fn hands_401_and_insufficient_scope_403_responses_to_the_auth_provider_with_the_rejected_token()
 {
    type Seen = Vec<(u16, Option<String>)>;
    let seen: Arc<Mutex<Seen>> = Arc::default();
    let token = Arc::new(Mutex::new("old".to_string()));
    let (url, _) = start_server(|request, response, requests| {
        boxed(async move {
            if request.method != "POST" {
                return protocol_handler(request, response, requests).await;
            }
            if request.header("authorization") == Some("Bearer old") {
                return response
                    .send(401, &[("www-authenticate", "Bearer")], "")
                    .await;
            }
            if request.json()["method"] == "tools/call"
                && request.header("authorization") == Some("Bearer new")
            {
                return response
                    .send(
                        403,
                        &[(
                            "www-authenticate",
                            "Bearer error=\"insufficient_scope\", scope=\"admin\"",
                        )],
                        "",
                    )
                    .await;
            }
            protocol_handler(request, response, requests).await
        })
    })
    .await;
    let (current, updated, log) = (token.clone(), token.clone(), seen.clone());
    let provider = FnAuthProvider::new(move || {
        let token = current.lock().clone();
        Box::pin(async move { Ok(Some(token)) })
    })
    .on_unauthorized(move |context| {
        let (updated, log) = (updated.clone(), log.clone());
        Box::pin(async move {
            log.lock()
                .push((context.response.status, context.token.clone()));
            *updated.lock() = if context.response.status == 401 {
                "new".into()
            } else {
                "admin".into()
            };
            Ok(())
        })
    });
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    client
        .connect(Arc::new(StreamableHttpTransport::new(
            transport(&url)
                .open_get_stream(false)
                .auth_provider(Arc::new(provider)),
        )))
        .await
        .unwrap();
    assert_eq!(
        call(&client, "echo", McpRequestOptions::default())
            .await
            .unwrap(),
        hello()
    );
    assert_eq!(
        *seen.lock(),
        vec![
            (401, Some("old".to_string())),
            (403, Some("new".to_string()))
        ]
    );
    client.close().await.unwrap();
}

// #10598: a request cancelled or timed out while waiting for a shared refresh must not be retried.
#[tokio::test]
async fn does_not_retry_requests_that_were_cancelled_while_authorization_was_recovering() {
    let token = Arc::new(Mutex::new("old".to_string()));
    let executed: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = executed.clone();
    let (url, _) = start_server(move |request, response, requests| {
        let log = log.clone();
        boxed(async move {
            if request.method != "POST" {
                return protocol_handler(request, response, requests).await;
            }
            let message = request.json();
            if message["method"] == "tools/call"
                && request.header("authorization") == Some("Bearer old")
            {
                return response
                    .send(401, &[("www-authenticate", "Bearer")], "")
                    .await;
            }
            if message["method"] == "tools/call" {
                log.lock()
                    .push(message["params"]["name"].as_str().unwrap().to_string());
            }
            protocol_handler(request, response, requests).await
        })
    })
    .await;
    let (release, refresh_gate) = tokio::sync::oneshot::channel::<()>();
    let refresh_gate = futures::FutureExt::shared(refresh_gate);
    let refresh: Arc<Mutex<Option<futures::future::Shared<BoxFuture<'static, ()>>>>> =
        Arc::default();
    let refreshes = Arc::new(AtomicUsize::new(0));
    let waiting = Arc::new(AtomicUsize::new(0));
    let (all_waiting, waiting_gate) = tokio::sync::oneshot::channel::<()>();
    let all_waiting = Arc::new(Mutex::new(Some(all_waiting)));
    let current = token.clone();
    let provider = FnAuthProvider::new(move || {
        let token = current.lock().clone();
        Box::pin(async move { Ok(Some(token)) })
    })
    .on_unauthorized({
        let (token, refresh, refreshes, waiting, all_waiting) = (
            token.clone(),
            refresh.clone(),
            refreshes.clone(),
            waiting.clone(),
            all_waiting.clone(),
        );
        move |_| {
            let shared = {
                let mut refresh = refresh.lock();
                if refresh.is_none() {
                    refreshes.fetch_add(1, Ordering::SeqCst);
                    let (token, gate) = (token.clone(), refresh_gate.clone());
                    let future: BoxFuture<'static, ()> = Box::pin(async move {
                        let _ = gate.await;
                        *token.lock() = "new".into();
                    });
                    *refresh = Some(futures::FutureExt::shared(future));
                }
                refresh.clone().unwrap()
            };
            if waiting.fetch_add(1, Ordering::SeqCst) + 1 == 3
                && let Some(sender) = all_waiting.lock().take()
            {
                let _ = sender.send(());
            }
            Box::pin(async move {
                shared.await;
                Ok(())
            })
        }
    });
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    client
        .connect(Arc::new(StreamableHttpTransport::new(
            transport(&url)
                .open_get_stream(false)
                .auth_provider(Arc::new(provider)),
        )))
        .await
        .unwrap();
    let controller = AbortController::new();
    let aborted = client.call_tool(
        "aborted",
        Some(Map::new()),
        McpRequestOptions::default().signal(controller.signal()),
    );
    let timed_out = client.call_tool(
        "timed-out",
        Some(Map::new()),
        McpRequestOptions::default().timeout_ms(200),
    );
    let kept = client.call_tool("kept", Some(Map::new()), McpRequestOptions::default());
    let (aborted, timed_out, kept) = (
        tokio::spawn(aborted),
        tokio::spawn(timed_out),
        tokio::spawn(kept),
    );
    waiting_gate.await.unwrap();
    controller.abort();
    assert_eq!(aborted.await.unwrap().unwrap_err().name(), "AbortError");
    assert_eq!(
        timed_out.await.unwrap().unwrap_err().name(),
        "McpTimeoutError"
    );
    release.send(()).unwrap();
    assert_eq!(
        serde_json::to_value(kept.await.unwrap().unwrap()).unwrap(),
        hello()
    );
    // Give a wrongly retried request the chance to reach the server.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(*executed.lock(), vec!["kept".to_string()]);
    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    client.close().await.unwrap();
}

// #10565: the auth provider may refresh over the network, which closing must not wait for.
#[tokio::test]
async fn closes_the_session_with_the_last_requests_token_without_asking_the_auth_provider() {
    let (url, requests) = start_server(|request, response, requests| {
        boxed(protocol_handler(request, response, requests))
    })
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let provider = FnAuthProvider::new(move || {
        let call = counter.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move { Ok(Some(format!("token-{call}"))) })
    });
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    client
        .connect(Arc::new(StreamableHttpTransport::new(
            transport(&url)
                .open_get_stream(false)
                .auth_provider(Arc::new(provider)),
        )))
        .await
        .unwrap();
    let before = calls.load(Ordering::SeqCst);
    client.close().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), before);
    let requests = requests.lock().clone();
    let deletes: Vec<&Recorded> = requests
        .iter()
        .filter(|entry| entry.method == "DELETE")
        .collect();
    assert_eq!(
        deletes
            .iter()
            .map(|entry| entry.headers.get("authorization").cloned())
            .collect::<Vec<_>>(),
        vec![Some(format!("Bearer token-{before}"))]
    );
    assert_eq!(
        deletes[0].headers.get("mcp-session-id").map(String::as_str),
        Some("session-1")
    );
}

// #10188: the TypeScript transport must call fetch without a receiver. In Rust the equivalent is
// that a custom fetch is used for every request, including the one the auth provider makes.
#[tokio::test]
async fn uses_the_configured_fetch_everywhere() {
    let (url, _) = start_server(|request, response, requests| {
        boxed(async move {
            if request.method == "POST" && request.header("authorization").is_none() {
                return response
                    .send(401, &[("www-authenticate", "Bearer")], "")
                    .await;
            }
            protocol_handler(request, response, requests).await
        })
    })
    .await;
    let fetches = Arc::new(AtomicUsize::new(0));
    let counter = fetches.clone();
    let base = pi_mcp::default_fetch();
    let fetch: pi_mcp::McpFetch = Arc::new(move |request| {
        counter.fetch_add(1, Ordering::SeqCst);
        base(request)
    });
    let token: Arc<Mutex<Option<String>>> = Arc::default();
    let (current, updated) = (token.clone(), token.clone());
    let probe = url.clone();
    let provider = FnAuthProvider::new(move || {
        let token = current.lock().clone();
        Box::pin(async move { Ok(token) })
    })
    .on_unauthorized(move |context| {
        let (updated, probe) = (updated.clone(), probe.clone());
        Box::pin(async move {
            let _ =
                (context.fetch)(pi_mcp::HttpRequest::get(url::Url::parse(&probe).unwrap())).await?;
            *updated.lock() = Some("token".into());
            Ok(())
        })
    });
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    client
        .connect(Arc::new(StreamableHttpTransport::new(
            transport(&url)
                .fetch(fetch)
                .open_get_stream(false)
                .auth_provider(Arc::new(provider)),
        )))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(
            client
                .list_tools(McpRequestOptions::default())
                .await
                .unwrap()
        )
        .unwrap(),
        json!([{ "name": "echo", "inputSchema": { "type": "object" } }])
    );
    client.close().await.unwrap();
    // initialize (401), probe GET, initialize, initialized, tools/list, DELETE.
    assert_eq!(fetches.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn classifies_an_expired_established_session() {
    let posts = Arc::new(AtomicUsize::new(0));
    let counter = posts.clone();
    let (url, _) = start_server(move |request, response, requests| {
        let counter = counter.clone();
        boxed(async move {
            if request.method == "POST" && counter.fetch_add(1, Ordering::SeqCst) >= 2 {
                return response.send(404, &[], "gone").await;
            }
            protocol_handler(request, response, requests).await
        })
    })
    .await;
    let client = McpClient::new(McpClientOptions::new("http-test", "1.0.0"));
    client
        .connect(Arc::new(StreamableHttpTransport::new(
            transport(&url).open_get_stream(false),
        )))
        .await
        .unwrap();
    let error = client
        .list_tools(McpRequestOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::SessionExpired { .. }), "{error:?}");
    assert_eq!(error.to_string(), "MCP session expired");
    client.close().await.unwrap();
}
