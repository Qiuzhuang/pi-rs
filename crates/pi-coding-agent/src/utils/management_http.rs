//! Port of `packages/coding-agent/src/utils/management-http.ts`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub type FetchInput = Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    init: Option<Value /* TS: RequestInit */>,
    options: Option<&FetchRetryOptions>,
) -> anyhow::Result<Arc<reqwest::Response>> {
    todo!("port utils/management-http.ts:fetchWithRetry")
}
