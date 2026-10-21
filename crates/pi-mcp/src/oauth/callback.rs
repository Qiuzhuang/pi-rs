//! A loopback HTTP server that receives the OAuth authorization response.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use url::Url;

use super::flow::get_search_param;
use crate::error::{Error, Result, parse_url};

const MAX_REQUEST_HEAD_BYTES: usize = 16 * 1024;

/// The authorization response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthCallback {
    pub code: String,
    pub state: String,
    pub iss: Option<String>,
}

/// Outcome shown on the browser page after the redirect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OAuthCallbackPage {
    Ok,
    Error { message: String, details: Option<String> },
}

pub type RenderPage = Arc<dyn Fn(&OAuthCallbackPage) -> String + Send + Sync>;

#[derive(Clone, Default)]
pub struct OAuthCallbackServerOptions {
    /// Address to listen on. Default: `127.0.0.1`.
    pub host: Option<String>,
    /// Host name in `redirect_url`, for example `localhost` for a client registered with it while
    /// listening on `127.0.0.1`. Default: `host`.
    pub redirect_host: Option<String>,
    pub port: Option<u16>,
    pub path: Option<String>,
    /// More paths that receive the callback, for example a server-specific path of a redirect URI.
    pub extra_paths: Vec<String>,
    pub timeout_ms: Option<u64>,
    /// Render the browser page as HTML. Default: a plain-text message.
    pub render_page: Option<RenderPage>,
}

fn plain_text(page: &OAuthCallbackPage) -> String {
    match page {
        OAuthCallbackPage::Ok => "Authorization complete. You may close this window.".to_string(),
        OAuthCallbackPage::Error { message, details: Some(details) } => format!("{message}\n\n{details}"),
        OAuthCallbackPage::Error { message, details: None } => message.clone(),
    }
}

struct Pending {
    sender: oneshot::Sender<Result<OAuthCallback>>,
    timer: Option<JoinHandle<()>>,
    path: Option<String>,
}

struct Inner {
    redirect_url: Url,
    paths: Vec<String>,
    timeout_ms: u64,
    render_page: Option<RenderPage>,
    pending: Mutex<HashMap<String, Pending>>,
}

/// Loopback server for OAuth redirects.
pub struct OAuthCallbackServer {
    redirect_url: String,
    inner: Arc<Inner>,
    accept: JoinHandle<()>,
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "",
    }
}

impl Inner {
    fn reject(&self, state: &str, error: Error) {
        if let Some(pending) = self.pending.lock().remove(state) {
            if let Some(timer) = pending.timer {
                timer.abort();
            }
            let _ = pending.sender.send(Err(error));
        }
    }

    fn reply(&self, status: u16, page: OAuthCallbackPage) -> Vec<u8> {
        let (content_type, extra, body) = match &self.render_page {
            Some(render) => ("text/html; charset=utf-8", "cache-control: no-store\r\n", render(&page)),
            None => ("text/plain; charset=utf-8", "", plain_text(&page)),
        };
        format!(
            "HTTP/1.1 {status} {}\r\ncontent-type: {content_type}\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n{body}",
            status_text(status),
            body.len()
        )
        .into_bytes()
    }

    fn error_page(&self, status: u16, message: &str) -> Vec<u8> {
        self.reply(status, OAuthCallbackPage::Error { message: message.to_string(), details: None })
    }

    fn handle(&self, raw_url: &str) -> Vec<u8> {
        let Ok(url) = self.redirect_url.join(raw_url) else {
            return self.error_page(404, "Not found");
        };
        if !self.paths.iter().any(|path| path == url.path()) {
            return self.error_page(404, "Not found");
        }
        let state = get_search_param(&url, "state").filter(|state| !state.is_empty());
        let pending = state.as_ref().and_then(|state| self.pending.lock().remove(state));
        let (Some(state), Some(pending)) = (state, pending) else {
            return self.error_page(400, "Invalid or expired OAuth state");
        };
        if let Some(timer) = pending.timer {
            timer.abort();
        }
        if pending.path.as_deref().is_some_and(|path| path != url.path()) {
            let _ = pending.sender.send(Err(Error::other("The authorization response arrived on another redirect URI")));
            return self.error_page(400, "Unexpected redirect URI");
        }
        if let Some(error) = get_search_param(&url, "error").filter(|error| !error.is_empty()) {
            let description = get_search_param(&url, "error_description").unwrap_or(error);
            let _ = pending.sender.send(Err(Error::Other(description.clone())));
            return self.reply(
                200,
                OAuthCallbackPage::Error {
                    message: "Authorization failed. You may close this window.".to_string(),
                    details: Some(description),
                },
            );
        }
        let Some(code) = get_search_param(&url, "code").filter(|code| !code.is_empty()) else {
            let _ = pending.sender.send(Err(Error::other("OAuth callback did not include an authorization code")));
            return self.error_page(400, "Missing authorization code");
        };
        let iss = get_search_param(&url, "iss").filter(|iss| !iss.is_empty());
        let _ = pending.sender.send(Ok(OAuthCallback { code, state, iss }));
        self.reply(200, OAuthCallbackPage::Ok)
    }

    async fn serve(self: Arc<Self>, mut stream: TcpStream) {
        let mut head = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(read) => head.extend_from_slice(&chunk[..read]),
            }
            if head.windows(4).any(|window| window == b"\r\n\r\n") || head.windows(2).any(|window| window == b"\n\n") {
                break;
            }
            if head.len() > MAX_REQUEST_HEAD_BYTES {
                return;
            }
        }
        let text = String::from_utf8_lossy(&head);
        let target = text.lines().next().and_then(|line| line.split_whitespace().nth(1)).unwrap_or("/").to_string();
        let response = self.handle(&target);
        let _ = stream.write_all(&response).await;
        let _ = stream.shutdown().await;
    }
}

impl OAuthCallbackServer {
    pub async fn listen(options: OAuthCallbackServerOptions) -> Result<Self> {
        let host = options.host.clone().unwrap_or_else(|| "127.0.0.1".to_string());
        let redirect_host = options.redirect_host.clone().unwrap_or_else(|| host.clone());
        let path = options.path.clone().unwrap_or_else(|| "/callback".to_string());
        let bind_host = host.trim_start_matches('[').trim_end_matches(']').to_string();
        let listener = TcpListener::bind((bind_host.as_str(), options.port.unwrap_or(0)))
            .await
            .map_err(|error| Error::Other(format!("listen {}: {error}", error_code(&error))))?;
        let port = listener.local_addr().map_err(|error| Error::Other(error.to_string()))?.port();
        let redirect_host =
            if redirect_host.contains(':') && !redirect_host.starts_with('[') { format!("[{redirect_host}]") } else { redirect_host };
        let redirect_url = format!("http://{redirect_host}:{port}{path}");
        let mut paths = vec![path];
        paths.extend(options.extra_paths.iter().cloned());
        let inner = Arc::new(Inner {
            redirect_url: parse_url(&redirect_url)?,
            paths,
            timeout_ms: options.timeout_ms.unwrap_or(5 * 60_000),
            render_page: options.render_page.clone(),
            pending: Mutex::new(HashMap::new()),
        });
        let serving = inner.clone();
        let accept = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serving.clone().serve(stream));
            }
        });
        Ok(Self { redirect_url, inner, accept })
    }

    pub fn redirect_url(&self) -> &str {
        &self.redirect_url
    }

    /// Wait for the authorization response with `state`. With `path`, a response on another path
    /// fails, so a server-specific redirect URI can tell authorization servers apart (RFC 9700
    /// section 4.4.2.2). Registration happens when this is called.
    pub fn wait_for_callback(
        &self,
        state: &str,
        path: Option<&str>,
    ) -> Result<impl Future<Output = Result<OAuthCallback>> + Send + 'static + use<>> {
        let mut pending = self.inner.pending.lock();
        if pending.contains_key(state) {
            return Err(Error::other("OAuth state is already pending"));
        }
        let (sender, receiver) = oneshot::channel();
        let (inner, key, timeout_ms) = (Arc::downgrade(&self.inner), state.to_string(), self.inner.timeout_ms);
        let timer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
            if let Some(inner) = inner.upgrade() {
                inner.reject(&key, Error::other("OAuth callback timed out"));
            }
        });
        pending.insert(state.to_string(), Pending { sender, timer: Some(timer), path: path.map(str::to_string) });
        Ok(async move { receiver.await.unwrap_or_else(|_| Err(Error::other("OAuth callback server closed"))) })
    }

    /// Stop listening; pending waits fail.
    pub async fn close(&self) -> Result<()> {
        let pending: Vec<Pending> = self.inner.pending.lock().drain().map(|(_, pending)| pending).collect();
        for pending in pending {
            if let Some(timer) = pending.timer {
                timer.abort();
            }
            let _ = pending.sender.send(Err(Error::other("OAuth callback server closed")));
        }
        self.accept.abort();
        Ok(())
    }
}

impl Drop for OAuthCallbackServer {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

fn error_code(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => "EADDRINUSE".to_string(),
        std::io::ErrorKind::PermissionDenied => "EACCES".to_string(),
        _ => error.to_string(),
    }
}
