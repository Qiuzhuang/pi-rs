//! Assistant message diagnostics (port of `utils/diagnostics.ts`).

use serde::{Deserialize, Serialize};

use crate::types::JsonObject;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DiagnosticErrorCode {
    String(String),
    Number(serde_json::Number),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticErrorInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<DiagnosticErrorCode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessageDiagnostic {
    #[serde(rename = "type")]
    pub diagnostic_type: String,
    pub timestamp: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<DiagnosticErrorInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<JsonObject>,
}

/// Render a thrown value as a message (`formatThrownValue`).
pub fn format_thrown_value(error: &(dyn std::error::Error + 'static)) -> String {
    error.to_string()
}

/// Extract diagnostic info from an error (`extractDiagnosticError`).
pub fn extract_diagnostic_error(error: &(dyn std::error::Error + 'static)) -> DiagnosticErrorInfo {
    DiagnosticErrorInfo { name: Some("Error".to_string()), message: error.to_string(), stack: None, code: None }
}

/// Diagnostic info for a plain message (`extractDiagnosticError` on a non-Error value).
pub fn diagnostic_error_from_message(name: Option<&str>, message: impl Into<String>) -> DiagnosticErrorInfo {
    DiagnosticErrorInfo { name: name.map(str::to_string), message: message.into(), stack: None, code: None }
}

pub fn create_assistant_message_diagnostic(
    diagnostic_type: impl Into<String>,
    error: DiagnosticErrorInfo,
    details: Option<JsonObject>,
) -> AssistantMessageDiagnostic {
    AssistantMessageDiagnostic {
        diagnostic_type: diagnostic_type.into(),
        timestamp: crate::utils::now_ms(),
        error: Some(error),
        details,
    }
}

pub fn append_assistant_message_diagnostic(
    diagnostics: &mut Option<Vec<AssistantMessageDiagnostic>>,
    diagnostic: AssistantMessageDiagnostic,
) {
    diagnostics.get_or_insert_with(Vec::new).push(diagnostic);
}
