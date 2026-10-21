//! `ModelsError` (port of `utils/models-error.ts`).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelsErrorCode {
    ModelSource,
    ModelValidation,
    Provider,
    Stream,
    Auth,
    OAuth,
}

impl ModelsErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelsErrorCode::ModelSource => "model_source",
            ModelsErrorCode::ModelValidation => "model_validation",
            ModelsErrorCode::Provider => "provider",
            ModelsErrorCode::Stream => "stream",
            ModelsErrorCode::Auth => "auth",
            ModelsErrorCode::OAuth => "oauth",
        }
    }
}

/// Error raised by the `Models` collection. The message keeps the cause's detail.
#[derive(Debug, Clone)]
pub struct ModelsError {
    pub code: ModelsErrorCode,
    pub message: String,
    /// Message of the underlying error (`error.cause.message`), when constructed with a cause.
    pub cause: Option<String>,
}

impl ModelsError {
    pub fn new(code: ModelsErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), cause: None }
    }

    /// Construct with a cause whose text is appended unless already contained.
    pub fn with_cause(code: ModelsErrorCode, message: impl Into<String>, cause: &dyn fmt::Display) -> Self {
        let message = message.into();
        let detail = cause.to_string().trim().to_string();
        let message = if detail.is_empty() || message.contains(&detail) { message } else { format!("{message}: {detail}") };
        Self { code, message, cause: Some(cause.to_string()) }
    }
}

impl fmt::Display for ModelsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ModelsError {}
