//! Port of `packages/coding-agent/src/extensions/mcp/log.ts`.
//!
//! Log messages MCP servers send with `notifications/message`, appended to `mcp.log` in the agent
//! directory. Several pi processes may write to the same file, so every message is one synchronous
//! append. The file is rotated to `mcp.log.1` once it grows past `MAX_LOG_BYTES`.

use parking_lot::Mutex;
use serde_json::Value;
use std::io::Write;
use std::path::Path;

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

fn format_data(data: &Value) -> String {
    match data {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "undefined".to_string()),
    }
}

/// `Date.toISOString()`.
pub(crate) fn iso_timestamp(now: chrono::DateTime<chrono::Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Format one `notifications/message` from `server` as a log line; continuation lines are indented.
pub fn format_mcp_log_message(
    server: &str,
    params: Value,
    now: Option<chrono::DateTime<chrono::Utc>>,
) -> String {
    let now = now.unwrap_or_else(chrono::Utc::now);
    let message = match params {
        Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("data".into(), other);
            map
        }
    };
    let level = message
        .get("level")
        .and_then(Value::as_str)
        .unwrap_or("info");
    let logger = match message.get("logger").and_then(Value::as_str) {
        Some(logger) if !logger.is_empty() => format!(" {logger}:"),
        _ => String::new(),
    };
    // `JSON.stringify(undefined)` is undefined, which `String()` prints as "undefined".
    let text = match message.get("data") {
        Some(data) => format_data(data),
        None => "undefined".to_string(),
    };
    let text = text.replace("\r\n", "\n").replace('\n', "\n    ");
    format!("{} [{server}] {level}{logger} {text}\n", iso_timestamp(now))
}

/// Appends server log messages to one file. Write errors are ignored: logging must not break tools.
pub struct McpServerLog {
    path: String,
    size: Mutex<Option<u64>>,
}

impl McpServerLog {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            size: Mutex::new(None),
        }
    }

    pub fn path(&self) -> String {
        self.path.clone()
    }

    pub fn write(&self, server: &str, params: Value) {
        let line = format_mcp_log_message(server, params, None);
        let mut size = self.size.lock();
        let _ = (|| -> std::io::Result<()> {
            let current = match *size {
                Some(current) => current,
                None => {
                    if let Some(parent) = Path::new(&self.path).parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    self.current_size()
                }
            };
            *size = Some(current);
            if current > MAX_LOG_BYTES {
                // Another process may have rotated it already; check before renaming.
                if self.current_size() > MAX_LOG_BYTES {
                    std::fs::rename(&self.path, format!("{}.1", self.path))?;
                }
                *size = Some(self.current_size());
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            file.write_all(line.as_bytes())?;
            *size = Some(size.unwrap_or(0) + line.len() as u64);
            Ok(())
        })();
    }

    fn current_size(&self) -> u64 {
        std::fs::metadata(&self.path)
            .map(|meta| meta.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_messages() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-02T03:04:05.006Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            format_mcp_log_message(
                "fake",
                json!({ "level": "warning", "logger": "db", "data": "slow\nquery" }),
                Some(now)
            ),
            "2026-01-02T03:04:05.006Z [fake] warning db: slow\n    query\n"
        );
        assert_eq!(
            format_mcp_log_message("fake", json!({ "level": "error", "data": { "code": 7 } }), Some(now)),
            "2026-01-02T03:04:05.006Z [fake] error {\"code\":7}\n"
        );
        assert_eq!(
            format_mcp_log_message("fake", json!("plain"), Some(now)),
            "2026-01-02T03:04:05.006Z [fake] info plain\n"
        );
    }
}
