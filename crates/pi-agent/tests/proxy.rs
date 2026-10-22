//! Port of `packages/agent/test/proxy.test.ts`, against a local HTTP server.

use pi_agent::{ProxyAssistantMessageEvent, ProxyStreamOptions, stream_proxy};
use pi_ai::utils::transcript::transcript_from_messages;
use pi_ai::{
    AssistantContent, AssistantMessageEvent, Model, ModelInputModality, StopReason, ToolCall, Usage,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn model() -> Model {
    Model {
        id: "gpt-5.4".into(),
        name: "GPT-5.4".into(),
        api: "openai-responses".into(),
        provider: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        reasoning: true,
        input: vec![ModelInputModality::Text],
        context_window: 400000,
        max_tokens: 128000,
        ..Default::default()
    }
}

/// Serve one HTTP response with `body` and return the proxy URL.
async fn serve(status: &'static str, body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let n = socket.read(&mut buffer).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..n]);
            let text = String::from_utf8_lossy(&request);
            if let Some(header_end) = text.find("\r\n\r\n") {
                let content_length = text[..header_end]
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
        }
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.shutdown().await.ok();
    });
    format!("http://{address}")
}

fn options(proxy_url: String) -> ProxyStreamOptions {
    ProxyStreamOptions {
        auth_token: "test-token".into(),
        proxy_url,
        http_client: Some(reqwest::Client::builder().no_proxy().build().unwrap()),
        ..Default::default()
    }
}

#[tokio::test]
async fn preserves_tool_call_metadata_received_only_on_toolcall_end() {
    let events = [
        ProxyAssistantMessageEvent::Start,
        ProxyAssistantMessageEvent::ToolCallStart {
            content_index: 0,
            id: "call_test|fc_test".into(),
            tool_name: "lookup".into(),
        },
        ProxyAssistantMessageEvent::ToolCallDelta {
            content_index: 0,
            delta: r#"{"value":"hello"}"#.into(),
        },
        ProxyAssistantMessageEvent::ToolCallEnd {
            content_index: 0,
            tool_call: ToolCall {
                id: "call_test|fc_test".into(),
                name: "lookup".into(),
                arguments: json!({ "value": "hello" }).as_object().unwrap().clone(),
                namespace: Some("dynamic_tools".into()),
                ..Default::default()
            },
        },
        ProxyAssistantMessageEvent::Done {
            reason: StopReason::ToolUse,
            usage: Usage::default(),
            provider_thinking_level: None,
        },
    ];
    let body: String = events
        .iter()
        .map(|event| format!("data: {}\n\n", serde_json::to_string(event).unwrap()))
        .collect();
    let url = serve("200 OK", body).await;

    let (events, result) = stream_proxy(model(), transcript_from_messages(vec![]), options(url))
        .collect_all()
        .await;
    let end = events
        .iter()
        .find(|e| matches!(e, AssistantMessageEvent::ToolCallEnd { .. }))
        .expect("toolcall_end");
    if let AssistantMessageEvent::ToolCallEnd { tool_call, .. } = end {
        assert_eq!(tool_call.namespace.as_deref(), Some("dynamic_tools"));
    }
    match &result.content[0] {
        AssistantContent::ToolCall(call) => {
            assert_eq!(
                call.arguments,
                *json!({ "value": "hello" }).as_object().unwrap()
            );
            assert_eq!(call.namespace.as_deref(), Some("dynamic_tools"));
        }
        other => panic!("unexpected content {other:?}"),
    }
}

#[tokio::test]
async fn processes_terminal_metadata_when_the_event_is_not_newline_terminated() {
    let start = format!("data: {}\n\n", json!({ "type": "start" }));
    let done = format!(
        "data: {}",
        json!({
            "type": "done",
            "reason": "stop",
            "usage": serde_json::to_value(Usage::default()).unwrap(),
            "providerThinkingLevel": "high"
        })
    );
    let url = serve("200 OK", start + &done).await;
    let (events, result) = stream_proxy(model(), transcript_from_messages(vec![]), options(url))
        .collect_all()
        .await;
    assert_eq!(
        events
            .iter()
            .map(AssistantMessageEvent::type_name)
            .collect::<Vec<_>>(),
        vec!["start", "done"]
    );
    assert_eq!(result.stop_reason, StopReason::Stop);
    assert_eq!(result.provider_thinking_level.as_deref(), Some("high"));
}

#[tokio::test]
async fn emits_an_error_instead_of_hanging_when_the_stream_ends_without_a_terminal_event() {
    let body = format!("data: {}\n\n", json!({ "type": "start" }));
    let url = serve("200 OK", body).await;
    let (events, result) = stream_proxy(model(), transcript_from_messages(vec![]), options(url))
        .collect_all()
        .await;
    assert_eq!(
        events
            .iter()
            .map(AssistantMessageEvent::type_name)
            .collect::<Vec<_>>(),
        vec!["start", "error"]
    );
    assert_eq!(result.stop_reason, StopReason::Error);
    assert!(
        result
            .error_message
            .unwrap()
            .contains("Connection closed by proxy server")
    );
}

#[tokio::test]
async fn reports_proxy_http_errors() {
    let url = serve(
        "502 Bad Gateway",
        json!({ "error": "upstream down" }).to_string(),
    )
    .await;
    let (events, result) = stream_proxy(model(), transcript_from_messages(vec![]), options(url))
        .collect_all()
        .await;
    assert_eq!(
        events
            .iter()
            .map(AssistantMessageEvent::type_name)
            .collect::<Vec<_>>(),
        vec!["error"]
    );
    assert_eq!(
        result.error_message.as_deref(),
        Some("Proxy error: upstream down")
    );
}

#[tokio::test]
async fn skips_unknown_proxy_event_types_like_typescript() {
    let body = format!(
        "data: {}\n\ndata: {}\n\ndata: {}\n\n",
        json!({ "type": "start" }),
        json!({ "type": "future_event", "contentIndex": 0 }),
        json!({
            "type": "done",
            "reason": "stop",
            "usage": serde_json::to_value(Usage::default()).unwrap()
        })
    );
    let url = serve("200 OK", body).await;
    let (events, result) = stream_proxy(model(), transcript_from_messages(vec![]), options(url))
        .collect_all()
        .await;
    assert_eq!(
        events
            .iter()
            .map(AssistantMessageEvent::type_name)
            .collect::<Vec<_>>(),
        vec!["start", "done"]
    );
    assert_eq!(result.stop_reason, StopReason::Stop);
}
