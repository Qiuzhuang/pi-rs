//! A tiny loopback HTTP/1.1 server for tests (port of `test/helpers.ts`). Every response closes
//! its connection, so streamed bodies end when the handler finishes.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub target: String,
    /// Lower-case names.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    pub fn url(&self, origin: &str) -> Url {
        Url::parse(origin).unwrap().join(&self.target).unwrap()
    }
}

pub struct Response {
    stream: TcpStream,
    started: bool,
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

impl Response {
    /// Send a complete response and close the connection.
    pub async fn send(mut self, status: u16, headers: &[(&str, &str)], body: impl AsRef<[u8]>) {
        let body = body.as_ref();
        let mut head = format!("HTTP/1.1 {status} {}\r\n", status_text(status));
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!("content-length: {}\r\nconnection: close\r\n\r\n", body.len()));
        let _ = self.stream.write_all(head.as_bytes()).await;
        let _ = self.stream.write_all(body).await;
        let _ = self.stream.shutdown().await;
    }

    /// Start a streamed response (no content length; the body ends when the connection closes).
    pub async fn start(&mut self, status: u16, headers: &[(&str, &str)]) {
        let mut head = format!("HTTP/1.1 {status} {}\r\n", status_text(status));
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("connection: close\r\n\r\n");
        let _ = self.stream.write_all(head.as_bytes()).await;
        self.started = true;
    }

    pub async fn write(&mut self, data: impl AsRef<[u8]>) {
        let _ = self.stream.write_all(data.as_ref()).await;
        let _ = self.stream.flush().await;
    }

    pub async fn end(mut self, data: impl AsRef<[u8]>) {
        let _ = self.stream.write_all(data.as_ref()).await;
        let _ = self.stream.shutdown().await;
    }

    /// Keep the connection open until the client goes away.
    pub async fn hold(mut self) {
        let mut buffer = [0u8; 1024];
        while let Ok(read) = self.stream.read(&mut buffer).await {
            if read == 0 {
                break;
            }
        }
    }
}

pub type Handler = Arc<dyn Fn(Request, Response, String) -> BoxFuture<'static, ()> + Send + Sync>;

async fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?.to_string();
    let target = request_line.next()?.to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let length: usize = headers.get("content-length").and_then(|value| value.parse().ok()).unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Some(Request { method, target, headers, body })
}

/// Start a loopback HTTP server and return its origin. The server stops when the test runtime ends.
pub async fn listen(handler: impl Fn(Request, Response, String) -> BoxFuture<'static, ()> + Send + Sync + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let handler: Handler = Arc::new(handler);
    let server_origin = origin.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let (handler, origin) = (handler.clone(), server_origin.clone());
            tokio::spawn(async move {
                if let Some(request) = read_request(&mut stream).await {
                    handler(request, Response { stream, started: false }, origin).await;
                }
            });
        }
    });
    origin
}
