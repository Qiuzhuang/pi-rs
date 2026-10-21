//! Port of `packages/coding-agent/src/core/http-dispatcher.ts`.
//!
//! The TS version installs an undici dispatcher. The Rust port configures the shared reqwest
//! clients of pi-ai (`pi_ai::utils::http`), which honor `HTTP(S)_PROXY`/`NO_PROXY` and apply the
//! idle timeout as a read timeout for response headers and body chunks.

use serde_json::Value;
use std::time::Duration;

pub const DEFAULT_HTTP_IDLE_TIMEOUT_MS: i64 = 300000;

/// One selectable HTTP idle timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpIdleTimeoutChoice {
    pub label: &'static str,
    pub timeout_ms: i64,
}

pub const HTTP_IDLE_TIMEOUT_CHOICES: &[HttpIdleTimeoutChoice] = &[
    HttpIdleTimeoutChoice { label: "30 sec", timeout_ms: 30_000 },
    HttpIdleTimeoutChoice { label: "1 min", timeout_ms: 60_000 },
    HttpIdleTimeoutChoice { label: "2 min", timeout_ms: 120_000 },
    HttpIdleTimeoutChoice { label: "5 min", timeout_ms: 300_000 },
    HttpIdleTimeoutChoice { label: "disabled", timeout_ms: 0 },
];

/// JS `Number(string)` for the subset of inputs that matter here.
fn js_number_from_str(value: &str) -> f64 {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    let lower = trimmed.to_ascii_lowercase();
    let parse_radix = |digits: &str, radix: u32| {
        u64::from_str_radix(digits, radix)
            .map(|n| n as f64)
            .unwrap_or(f64::NAN)
    };
    if let Some(hex) = lower.strip_prefix("0x") {
        return parse_radix(hex, 16);
    }
    if let Some(oct) = lower.strip_prefix("0o") {
        return parse_radix(oct, 8);
    }
    if let Some(bin) = lower.strip_prefix("0b") {
        return parse_radix(bin, 2);
    }
    match trimmed {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if lower.contains("inf") || lower.contains("nan") {
        return f64::NAN;
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

pub fn parse_http_idle_timeout_ms(value: Value) -> Option<i64> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.eq_ignore_ascii_case("disabled") {
                return Some(0);
            }
            if trimmed.is_empty() {
                return None;
            }
            parse_number(js_number_from_str(trimmed))
        }
        Value::Number(number) => parse_number(number.as_f64().unwrap_or(f64::NAN)),
        _ => None,
    }
}

fn parse_number(value: f64) -> Option<i64> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Some(value.floor() as i64)
}

pub fn format_http_idle_timeout_ms(timeout_ms: i64) -> String {
    if let Some(choice) = HTTP_IDLE_TIMEOUT_CHOICES
        .iter()
        .find(|item| item.timeout_ms == timeout_ms)
    {
        return choice.label.to_string();
    }
    let seconds = timeout_ms as f64 / 1000.0;
    format!("{} sec", pi_ai::serde_util::js_number(seconds))
}

pub fn apply_http_proxy_settings(http_proxy: Option<&str>) {
    let Some(proxy) = http_proxy.map(str::trim).filter(|p| !p.is_empty()) else {
        return;
    };
    // SAFETY: called during single-threaded startup, before worker threads read the environment.
    unsafe {
        if std::env::var_os("HTTP_PROXY").is_none() {
            std::env::set_var("HTTP_PROXY", proxy);
        }
        if std::env::var_os("HTTPS_PROXY").is_none() {
            std::env::set_var("HTTPS_PROXY", proxy);
        }
    }
}

pub fn configure_http_dispatcher(timeout_ms: Option<i64>) -> anyhow::Result<()> {
    let timeout_ms = timeout_ms.unwrap_or(DEFAULT_HTTP_IDLE_TIMEOUT_MS);
    let Some(normalized) = parse_http_idle_timeout_ms(Value::from(timeout_ms)) else {
        anyhow::bail!("Invalid HTTP idle timeout: {timeout_ms}");
    };
    pi_ai::utils::http::set_http_idle_timeout(
        (normalized > 0).then(|| Duration::from_millis(normalized as u64)),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_idle_timeouts() {
        assert_eq!(parse_http_idle_timeout_ms(json!("disabled")), Some(0));
        assert_eq!(parse_http_idle_timeout_ms(json!(" Disabled ")), Some(0));
        assert_eq!(parse_http_idle_timeout_ms(json!("  ")), None);
        assert_eq!(parse_http_idle_timeout_ms(json!("1500.7")), Some(1500));
        assert_eq!(parse_http_idle_timeout_ms(json!(30000)), Some(30000));
        assert_eq!(parse_http_idle_timeout_ms(json!(-1)), None);
        assert_eq!(parse_http_idle_timeout_ms(json!("abc")), None);
        assert_eq!(parse_http_idle_timeout_ms(json!(true)), None);
    }

    #[test]
    fn formats_idle_timeouts() {
        assert_eq!(format_http_idle_timeout_ms(60_000), "1 min");
        assert_eq!(format_http_idle_timeout_ms(0), "disabled");
        assert_eq!(format_http_idle_timeout_ms(45_000), "45 sec");
        assert_eq!(format_http_idle_timeout_ms(1_500), "1.5 sec");
    }

    #[test]
    fn rejects_invalid_dispatcher_timeout() {
        let error = configure_http_dispatcher(Some(-5)).unwrap_err();
        assert_eq!(error.to_string(), "Invalid HTTP idle timeout: -5");
    }
}
