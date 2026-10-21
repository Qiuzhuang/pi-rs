//! Port of `packages/coding-agent/src/core/bug-report-upload.ts`.

use crate::core::bug_report::BugReportBundle;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct UploadBugReportOptions {
    pub token: Option<String>,
    pub signal: Option<CancellationToken>,
    pub gateway_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadBugReportUploadBugReportResult {
    pub id: String,
}

/// Upload a report as multipart form data, anonymously or attributed to a Radius account.
pub async fn upload_bug_report(
    bundle: &BugReportBundle,
    options: Option<&UploadBugReportOptions>,
) -> anyhow::Result<UploadBugReportUploadBugReportResult> {
    todo!("port core/bug-report-upload.ts:uploadBugReport")
}
