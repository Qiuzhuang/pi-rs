//! Port of `packages/coding-agent/src/utils/management-http.ts`.
//!
//! The TS helper wraps the global `fetch`; here requests go through the shared pi-ai HTTP
//! client (`pi_ai::utils::http::client_for`, which honors the proxy environment). The request
//! is described by [`FetchInit`] so it can be rebuilt for every attempt.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

pub type FetchInput = str;

const RETRYABLE_STATUS_CODES: [u16; 7] = [408, 425, 429, 500, 502, 503, 504];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchRetryOptions {
    /// Number of additional attempts after the initial request. Defaults to two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<i64>,
    /// Retry transient HTTP responses as well as transport failures. Defaults to true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_on_status: Option<bool>,
    /// Overall time budget shared by all attempts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<i64>,
    /// Per-attempt timeout. A new timeout is created for every attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_timeout_ms: Option<i64>,
}

/// `RequestInit` subset used by management requests.
#[derive(Debug, Clone, Default)]
pub struct FetchInit {
    /// HTTP method (defaults to GET).
    pub method: Option<reqwest::Method>,
    /// Request headers, in order.
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// Caller cancellation.
    pub signal: Option<CancellationToken>,
    /// Client override (defaults to the shared proxy-aware client for the URL).
    pub client: Option<reqwest::Client>,
}

/// The error thrown when a request is aborted (`AbortError`) or timed out (`TimeoutError`).
#[derive(Debug, Clone, thiserror::Error)]
pub enum FetchAbortError {
    #[error("This operation was aborted")]
    Aborted,
    #[error("The operation was aborted due to timeout")]
    Timeout,
}

impl FetchAbortError {
    /// The JS error name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Aborted => "AbortError",
            Self::Timeout => "TimeoutError",
        }
    }
}

async fn send_once(input: &str, init: Option<&FetchInit>) -> anyhow::Result<reqwest::Response> {
    let client = match init.and_then(|i| i.client.clone()) {
        Some(client) => client,
        None => pi_ai::utils::http::client_for(input, None)?,
    };
    let method = init.and_then(|i| i.method.clone()).unwrap_or(reqwest::Method::GET);
    let mut request = client.request(method, input);
    if let Some(init) = init {
        for (name, value) in &init.headers {
            request = request.header(name, value);
        }
        if let Some(body) = &init.body {
            request = request.body(body.clone());
        }
    }
    // A transport failure is undici's `TypeError: fetch failed` with the network error as cause.
    request.send().await.map_err(|error| anyhow::Error::new(pi_ai::utils::http::FetchFailed::new(&error)))
}

/// Fetch a management HTTP resource with a bounded immediate retry.
///
/// This is intentionally a transport-level helper for idempotent management
/// requests (version checks, catalogs, and downloads). It must not be used for
/// agent/model operations: those can fail after the HTTP request starts and are
/// retried by their semantic caller instead.
///
/// Caller cancellation and timeoutMs are terminal. attemptTimeoutMs aborts
/// only the current attempt so a hung connection can be retried.
pub async fn fetch_with_retry(
    input: &FetchInput,
    init: Option<&FetchInit>,
    options: Option<&FetchRetryOptions>,
) -> anyhow::Result<reqwest::Response> {
    let default_options = FetchRetryOptions::default();
    let options = options.unwrap_or(&default_options);
    let max_retries = options.max_retries.map(|n| n.max(0) as u64).unwrap_or(2);
    let retry_on_status = options.retry_on_status.unwrap_or(true);
    let parent_signal = init.and_then(|i| i.signal.clone());
    let deadline = options
        .timeout_ms
        .filter(|ms| *ms > 0)
        .map(|ms| tokio::time::Instant::now() + Duration::from_millis(ms as u64));
    let attempt_timeout = options.attempt_timeout_ms.filter(|ms| *ms > 0).map(|ms| Duration::from_millis(ms as u64));

    let mut attempt: u64 = 0;
    loop {
        if parent_signal.as_ref().is_some_and(CancellationToken::is_cancelled) {
            return Err(FetchAbortError::Aborted.into());
        }
        if deadline.is_some_and(|d| tokio::time::Instant::now() >= d) {
            return Err(FetchAbortError::Timeout.into());
        }

        enum Outcome {
            Done(anyhow::Result<reqwest::Response>),
            ParentAborted,
            TimedOut,
            AttemptTimedOut,
        }

        let request = send_once(input, init);
        let parent = parent_signal.clone();
        let outcome = tokio::select! {
            result = request => Outcome::Done(result),
            _ = async { match &parent { Some(s) => s.cancelled().await, None => std::future::pending().await } } => Outcome::ParentAborted,
            _ = async { match deadline { Some(d) => tokio::time::sleep_until(d).await, None => std::future::pending().await } } => Outcome::TimedOut,
            _ = async { match attempt_timeout { Some(t) => tokio::time::sleep(t).await, None => std::future::pending().await } } => Outcome::AttemptTimedOut,
        };

        match outcome {
            Outcome::ParentAborted => return Err(FetchAbortError::Aborted.into()),
            Outcome::TimedOut => return Err(FetchAbortError::Timeout.into()),
            Outcome::AttemptTimedOut => {
                if attempt >= max_retries {
                    return Err(FetchAbortError::Timeout.into());
                }
            }
            Outcome::Done(Ok(response)) => {
                let should_retry = retry_on_status
                    && RETRYABLE_STATUS_CODES.contains(&response.status().as_u16())
                    && attempt < max_retries;
                if !should_retry {
                    return Ok(response);
                }
                // The response is discarded before a retry.
                drop(response);
            }
            Outcome::Done(Err(error)) => {
                if attempt >= max_retries {
                    return Err(error);
                }
            }
        }
        attempt += 1;
    }
}

#[cfg(test)]
pub(crate) mod test_server {
    //! A tiny HTTP/1.1 server for management HTTP tests.

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// What the server does for one request.
    #[derive(Clone)]
    pub enum Reply {
        /// Close the connection without answering (a transport failure).
        Drop,
        /// Never answer.
        Hang,
        /// Answer with status and body.
        Respond(u16, String),
    }

    pub struct TestServer {
        pub url: String,
        pub hits: Arc<AtomicUsize>,
        pub requests: Arc<parking_lot::Mutex<Vec<String>>>,
    }

    /// Serve `replies` in order; the last one repeats.
    pub async fn serve(replies: Vec<Reply>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let requests: Arc<parking_lot::Mutex<Vec<String>>> = Arc::default();
        let h = hits.clone();
        let r = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let index = h.fetch_add(1, Ordering::SeqCst);
                let reply = replies.get(index).or(replies.last()).cloned().unwrap_or(Reply::Drop);
                let r = r.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    r.lock().push(String::from_utf8_lossy(&buf[..n]).into_owned());
                    match reply {
                        Reply::Drop => {}
                        Reply::Hang => {
                            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        }
                        Reply::Respond(status, body) => {
                            let response = format!(
                                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            let _ = socket.write_all(response.as_bytes()).await;
                            let _ = socket.shutdown().await;
                        }
                    }
                });
            }
        });
        TestServer { url, hits, requests }
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{Reply, serve};
    use super::*;
    use std::sync::atomic::Ordering;

    fn plain_client() -> FetchInit {
        FetchInit { client: Some(reqwest::Client::builder().no_proxy().build().unwrap()), ..Default::default() }
    }

    #[tokio::test]
    async fn retries_a_transient_transport_failure_once() {
        let server = serve(vec![Reply::Drop, Reply::Drop, Reply::Respond(200, r#"{"ok":true}"#.into())]).await;
        let response = fetch_with_retry(&server.url, Some(&plain_client()), None).await.unwrap();
        assert!(response.status().is_success());
        assert_eq!(server.hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn shares_the_timeout_budget_across_attempts() {
        let server = serve(vec![Reply::Drop, Reply::Respond(200, "{}".into())]).await;
        let options = FetchRetryOptions { timeout_ms: Some(1000), ..Default::default() };
        let response = fetch_with_retry(&server.url, Some(&plain_client()), Some(&options)).await.unwrap();
        assert!(response.status().is_success());
        assert_eq!(server.hits.load(Ordering::SeqCst), 2);

        let server = serve(vec![Reply::Hang]).await;
        let options = FetchRetryOptions { timeout_ms: Some(200), ..Default::default() };
        let err = fetch_with_retry(&server.url, Some(&plain_client()), Some(&options)).await.unwrap_err();
        assert_eq!(err.to_string(), "The operation was aborted due to timeout");
        assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retries_an_attempt_timeout() {
        let server = serve(vec![Reply::Hang, Reply::Respond(200, "{}".into())]).await;
        let options = FetchRetryOptions { attempt_timeout_ms: Some(200), ..Default::default() };
        let response = fetch_with_retry(&server.url, Some(&plain_client()), Some(&options)).await.unwrap();
        assert!(response.status().is_success());
        assert_eq!(server.hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retries_transient_http_responses_and_returns_the_successful_response() {
        let server = serve(vec![Reply::Respond(503, "busy".into()), Reply::Respond(200, "{}".into())]).await;
        let response = fetch_with_retry(&server.url, Some(&plain_client()), None).await.unwrap();
        assert!(response.status().is_success());
        assert_eq!(server.hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn does_not_retry_caller_cancellation() {
        let server = serve(vec![Reply::Respond(200, "{}".into())]).await;
        let signal = CancellationToken::new();
        signal.cancel();
        let init = FetchInit { signal: Some(signal), ..plain_client() };
        assert!(fetch_with_retry(&server.url, Some(&init), None).await.is_err());
        assert_eq!(server.hits.load(Ordering::SeqCst), 0);
    }
}
