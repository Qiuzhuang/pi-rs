//! Port of `packages/coding-agent/src/core/bug-report.ts`.

use crate::core::crash_log::CrashRecord;
use crate::core::extensions::types::Extension;
use crate::core::model_runtime::ModelRuntime;
use crate::core::session_manager::ReadonlySessionManager;
use crate::core::settings_schema::Settings;
use indexmap::IndexMap;
use pi_agent::types::AgentMessage;
use pi_agent::types::StreamFn;
use pi_agent::types::ThinkingLevel;
use pi_ai::types::Model;
use pi_ai::utils::retry::RetryPolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub const BUG_REPORT_CUSTOM_ENTRY_TYPE: &str = "pi.bug-report";

/// Strip credentials and secret-looking query parameters from a URL.
pub fn redact_url(value: &str) -> String {
    todo!("port core/bug-report.ts:redactUrl")
}

/// Copy a JSON value while removing values that may contain credentials.
pub fn redact_json_value(value: Value) -> Value {
    todo!("port core/bug-report.ts:redactJsonValue")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectBugReportMetadataOptionsExtensionErrors {
    pub path: String,
    pub error: String,
}

#[derive(Clone)]
pub struct CollectBugReportMetadataOptions {
    pub id: Option<String>,
    pub hint: Option<String>,
    pub session_id: String,
    pub cwd: String,
    pub include_session: bool,
    pub include_summary: bool,
    pub message_count: f64,
    pub model: Option<Model>,
    pub model_runtime: Arc<ModelRuntime>,
    pub thinking_level: ThinkingLevel,
    pub extensions: Vec<Extension>,
    pub extension_errors: Vec<CollectBugReportMetadataOptionsExtensionErrors>,
    pub global_settings: Settings,
    pub project_settings: Settings,
}

pub fn collect_bug_report_metadata(options: &CollectBugReportMetadataOptions) {
    todo!("port core/bug-report.ts:collectBugReportMetadata")
}

/// Collect failed assistant turns without collecting conversation content.
pub fn collect_bug_report_diagnostics(
    session_manager: &ReadonlySessionManager,
    crashes: Option<&[CrashRecord]>,
) {
    todo!("port core/bug-report.ts:collectBugReportDiagnostics")
}

pub type BugReportMetadata = Value;

pub type BugReportDiagnostics = Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportBundle {
    pub metadata: BugReportMetadata,
    pub diagnostics: BugReportDiagnostics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_jsonl: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BugReportSessionEntryDataDelivery {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "upload")]
    Upload,
}

impl BugReportSessionEntryDataDelivery {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Upload => "upload",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportSessionEntryData {
    pub id: String,
    pub created_at: String,
    #[serde(default)]
    pub hint: Option<String>,
    pub session_included: bool,
    pub summary_included: bool,
    pub delivery: BugReportSessionEntryDataDelivery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportFile {
    pub name: String,
    pub content_type: String,
    pub data: String,
}

/// Files shared by upload and zip export.
pub fn bug_report_files(bundle: &BugReportBundle) -> Vec<BugReportFile> {
    todo!("port core/bug-report.ts:bugReportFiles")
}

pub async fn write_bug_report_archive(
    bundle: &BugReportBundle,
    file_path: &str,
) -> anyhow::Result<()> {
    todo!("port core/bug-report.ts:writeBugReportArchive")
}

pub fn bug_report_archive_file_name(id: &str) -> String {
    todo!("port core/bug-report.ts:bugReportArchiveFileName")
}

#[derive(Clone)]
pub struct GenerateBugReportSummaryOptions {
    pub messages: Vec<AgentMessage>,
    pub hint: Option<String>,
    pub model: Model,
    pub api_key: Option<String>,
    pub headers: Option<IndexMap<String, String>>,
    pub env: Option<IndexMap<String, String>>,
    pub signal: CancellationToken,
    pub thinking_level: Option<ThinkingLevel>,
    pub stream_fn: Option<StreamFn>,
    pub retry: Option<RetryPolicy>,
    pub session_id: Option<String>,
}

/// Ask the session model for a report when the user does not share the transcript.
pub async fn generate_bug_report_summary(
    options: &GenerateBugReportSummaryOptions,
) -> anyhow::Result<String> {
    todo!("port core/bug-report.ts:generateBugReportSummary")
}
