//! Local HTTP and WebSocket mock servers for the OpenAI Responses / Codex / pi-messages / OAuth tests.
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request as WsRequest, Response as WsResponse};

#[derive(Debug, Clone, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("request body is JSON")
    }
    /// JSON body, decompressing zstd when `content-encoding: zstd`.
    pub fn decoded_json(&self) -> serde_json::Value {
        if self.header("content-encoding") == Some("zstd") {
            let mut decoder = ruzstd::decoding::StreamingDecoder::new(self.body.as_slice()).expect("zstd body");
            let mut out = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut out).unwrap();
            serde_json::from_slice(&out).expect("JSON body")
        } else {
            self.json()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Body chunks, each sent after an optional delay.
    pub chunks: Vec<(u64, Vec<u8>)>,
    /// Keep the connection open after the last chunk (the body never ends).
    pub keep_open: bool,
    /// Delay before the status line is sent.
    pub header_delay_ms: u64,
}

impl Reply {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), content_type.into())],
            chunks: vec![(0, body.into())],
            keep_open: false,
            header_delay_ms: 0,
        }
    }
    pub fn sse(body: impl Into<String>) -> Self {
        Self::new(200, "text/event-stream", body.into().into_bytes())
    }
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self::new(status, "application/json", body.into().into_bytes())
    }
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self::new(status, "text/plain", body.into().into_bytes())
    }
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
    pub fn chunk(mut self, delay_ms: u64, body: impl Into<Vec<u8>>) -> Self {
        self.chunks.push((delay_ms, body.into()));
        self
    }
    pub fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }
    pub fn delay_headers(mut self, ms: u64) -> Self {
        self.header_delay_ms = ms;
        self
    }
}

pub type Handler = Arc<dyn Fn(&Request) -> Reply + Send + Sync>;

pub struct MockHttp {
    pub base_url: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
}

impl MockHttp {
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().clone()
    }
}

/// Serve the replies in order; the last reply repeats.
pub async fn serve_sequence(replies: Vec<Reply>) -> MockHttp {
    let replies = Arc::new(Mutex::new(replies));
    serve(Arc::new(move |_req: &Request| {
        let mut replies = replies.lock();
        if replies.len() > 1 { replies.remove(0) } else { replies[0].clone() }
    }))
    .await
}

pub async fn serve(handler: Handler) -> MockHttp {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests: Arc<Mutex<Vec<Request>>> = Arc::default();
    let captured = requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else { return };
            let handler = handler.clone();
            let captured = captured.clone();
            tokio::spawn(async move { handle_connection(socket, handler, captured).await });
        }
    });
    MockHttp { base_url: format!("http://{addr}"), requests }
}

async fn read_request(socket: &mut TcpStream) -> Option<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().split(' ');
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
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    let body = buffer[header_end..(header_end + length).min(buffer.len())].to_vec();
    Some(Request { method, path, headers, body })
}

async fn handle_connection(mut socket: TcpStream, handler: Handler, captured: Arc<Mutex<Vec<Request>>>) {
    let Some(request) = read_request(&mut socket).await else { return };
    captured.lock().push(request.clone());
    let reply = handler(&request);
    if reply.header_delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(reply.header_delay_ms)).await;
    }
    let mut head = format!("HTTP/1.1 {} X\r\ntransfer-encoding: chunked\r\nconnection: close\r\n", reply.status);
    for (k, v) in &reply.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    if socket.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    for (delay, bytes) in &reply.chunks {
        if *delay > 0 {
            tokio::time::sleep(Duration::from_millis(*delay)).await;
        }
        if bytes.is_empty() {
            continue;
        }
        let frame = [format!("{:x}\r\n", bytes.len()).into_bytes(), bytes.clone(), b"\r\n".to_vec()].concat();
        if socket.write_all(&frame).await.is_err() {
            return;
        }
        let _ = socket.flush().await;
    }
    if reply.keep_open {
        tokio::time::sleep(Duration::from_secs(3600)).await;
        return;
    }
    let _ = socket.write_all(b"0\r\n\r\n").await;
    let _ = socket.shutdown().await;
}

/// What the WebSocket mock does after receiving a message.
#[derive(Debug, Clone)]
pub enum WsAction {
    Send(String),
    Close(u16, String),
    Sleep(u64),
}

pub type WsHandler = Arc<dyn Fn(usize, &serde_json::Value) -> Vec<WsAction> + Send + Sync>;

#[derive(Debug, Clone, Default)]
pub struct WsConnection {
    pub headers: Vec<(String, String)>,
    pub messages: Vec<serde_json::Value>,
}

impl WsConnection {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

pub struct MockWs {
    pub base_url: String,
    pub connections: Arc<Mutex<Vec<WsConnection>>>,
}

impl MockWs {
    pub fn connections(&self) -> Vec<WsConnection> {
        self.connections.lock().clone()
    }
}

/// A WebSocket server; `handler(connection_index, message)` scripts the replies.
/// `accept_delay_ms` delays the handshake (to exercise connect timeouts).
pub async fn serve_ws(handler: WsHandler, accept_delay_ms: u64) -> MockWs {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let connections: Arc<Mutex<Vec<WsConnection>>> = Arc::default();
    let captured = connections.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else { return };
            let handler = handler.clone();
            let captured = captured.clone();
            tokio::spawn(async move {
                if accept_delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(accept_delay_ms)).await;
                }
                let headers: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
                let header_sink = headers.clone();
                let callback = move |request: &WsRequest, response: WsResponse| {
                    *header_sink.lock() = request
                        .headers()
                        .iter()
                        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or_default().to_string()))
                        .collect();
                    Ok(response)
                };
                let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(socket, callback).await else { return };
                let index = {
                    let mut list = captured.lock();
                    list.push(WsConnection { headers: headers.lock().clone(), messages: Vec::new() });
                    list.len() - 1
                };
                while let Some(Ok(message)) = ws.next().await {
                    let Message::Text(text) = message else { continue };
                    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                    captured.lock()[index].messages.push(value.clone());
                    for action in handler(index, &value) {
                        match action {
                            WsAction::Send(text) => {
                                if ws.send(Message::text(text)).await.is_err() {
                                    return;
                                }
                            }
                            WsAction::Close(code, reason) => {
                                let frame = tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                    code: code.into(),
                                    reason: reason.into(),
                                };
                                let _ = ws.close(Some(frame)).await;
                                return;
                            }
                            WsAction::Sleep(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
                        }
                    }
                }
            });
        }
    });
    MockWs { base_url: format!("http://{addr}"), connections }
}

/// `data: <json>` frames joined by blank lines.
pub fn data_sse(events: &[serde_json::Value]) -> String {
    events.iter().map(|e| format!("data: {e}\n\n")).collect()
}

/// An HTTP client whose connections always fail (for payload-only tests).
pub fn refusing_client() -> reqwest::Client {
    reqwest::Client::builder().proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap()).build().unwrap()
}
