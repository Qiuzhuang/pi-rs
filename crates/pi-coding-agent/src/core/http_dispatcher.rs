//! Port of `packages/coding-agent/src/core/http-dispatcher.ts`.

use serde_json::Value;
use std::sync::LazyLock;

pub const DEFAULT_HTTP_IDLE_TIMEOUT_MS: u64 = 300000;

pub static HTTP_IDLE_TIMEOUT_CHOICES: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/http-dispatcher.ts:HTTP_IDLE_TIMEOUT_CHOICES"));

pub fn parse_http_idle_timeout_ms(value: Value) -> Option<i64> {
    todo!("port core/http-dispatcher.ts:parseHttpIdleTimeoutMs")
}

pub fn format_http_idle_timeout_ms(timeout_ms: i64) -> String {
    todo!("port core/http-dispatcher.ts:formatHttpIdleTimeoutMs")
}

pub fn apply_http_proxy_settings(http_proxy: Option<&str>) {
    todo!("port core/http-dispatcher.ts:applyHttpProxySettings")
}

pub fn configure_http_dispatcher(timeout_ms: Option<i64>) -> anyhow::Result<()> {
    todo!("port core/http-dispatcher.ts:configureHttpDispatcher")
}
