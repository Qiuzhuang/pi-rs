//! A tiny local HTTP server for offline API tests.
#![allow(dead_code)]

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone, Default)]
pub struct CapturedRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl CapturedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("request body is JSON")
    }
}

#[derive(Debug, Clone)]
pub struct MockResponse {
    pub status: u16,
    pub content_type: String,
    pub body: String,
    pub extra_headers: Vec<(String, String)>,
}

impl MockResponse {
    pub fn sse(body: impl Into<String>) -> Self {
        Self { status: 200, content_type: "text/event-stream".into(), body: body.into(), extra_headers: vec![] }
    }
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self { status, content_type: "application/json".into(), body: body.into(), extra_headers: vec![] }
    }
}

pub struct MockServer {
    pub base_url: String,
    pub requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

/// Serve the given responses in order (one per connection).
pub async fn serve(responses: Vec<MockResponse>) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests: Arc<Mutex<Vec<CapturedRequest>>> = Arc::default();
    let captured = requests.clone();
    tokio::spawn(async move {
        for response in responses {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 8192];
            let header_end;
            loop {
                let n = socket.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                buffer.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                    header_end = pos + 4;
                    break;
                }
            }
            let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
            let mut lines = head.split("\r\n");
            let request_line = lines.next().unwrap_or_default();
            let mut parts = request_line.split(' ');
            let method = parts.next().unwrap_or_default().to_string();
            let path = parts.next().unwrap_or_default().to_string();
            let headers: Vec<(String, String)> = lines
                .filter(|l| !l.is_empty())
                .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
                .collect();
            let length: usize = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.parse().ok())
                .unwrap_or(0);
            while buffer.len() < header_end + length {
                let n = socket.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..n]);
            }
            let body = String::from_utf8_lossy(&buffer[header_end..(header_end + length).min(buffer.len())]).to_string();
            captured.lock().push(CapturedRequest { method, path, headers, body });
            let mut out = format!(
                "HTTP/1.1 {} X\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n",
                response.status,
                response.content_type,
                response.body.len()
            );
            for (k, v) in &response.extra_headers {
                out.push_str(&format!("{k}: {v}\r\n"));
            }
            out.push_str("\r\n");
            out.push_str(&response.body);
            let _ = socket.write_all(out.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    MockServer { base_url: format!("http://{addr}"), requests }
}

/// Encode `(event, data)` pairs as an SSE body.
pub fn sse_body(events: &[(&str, serde_json::Value)]) -> String {
    events.iter().map(|(event, data)| format!("event: {event}\ndata: {data}\n\n")).collect()
}

/// Encode OpenAI-style `data:` chunks plus `[DONE]`.
pub fn openai_sse_body(chunks: &[serde_json::Value]) -> String {
    let mut body: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    body.push_str("data: [DONE]\n\n");
    body
}
