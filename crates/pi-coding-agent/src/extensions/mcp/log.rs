//! Port of `packages/coding-agent/src/extensions/mcp/log.ts`.

use serde_json::Value;

/// Format one `notifications/message` from `server` as a log line; continuation lines are indented.
pub fn format_mcp_log_message(
    server: &str,
    params: Value,
    now: Option<chrono::DateTime<chrono::Utc>>,
) -> String {
    todo!("port extensions/mcp/log.ts:formatMcpLogMessage")
}

/// Appends server log messages to one file. Write errors are ignored: logging must not break tools.
pub struct McpServerLog {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl McpServerLog {
    pub fn new(path: &str) -> Self {
        todo!("port extensions/mcp/log.ts:McpServerLog.constructor")
    }

    pub fn path(&self) -> String {
        todo!("port extensions/mcp/log.ts:McpServerLog.path")
    }

    pub fn write(&self, server: &str, params: Value) {
        todo!("port extensions/mcp/log.ts:McpServerLog.write")
    }
}
