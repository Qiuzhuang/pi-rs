//! Agent errors.

/// An error raised by the agent API (the TypeScript `Error` messages are kept verbatim).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct AgentError {
    pub message: String,
}

impl AgentError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
