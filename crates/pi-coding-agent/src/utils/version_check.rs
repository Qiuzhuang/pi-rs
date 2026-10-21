//! Port of `packages/coding-agent/src/utils/version-check.ts`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestPiRelease {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Include useful errno details hidden behind Node's generic "fetch failed" error.
pub fn format_version_check_error(error: Value) -> String {
    todo!("port utils/version-check.ts:formatVersionCheckError")
}

pub fn compare_package_versions(left_version: &str, right_version: &str) -> Option<i64> {
    todo!("port utils/version-check.ts:comparePackageVersions")
}

pub fn is_newer_package_version(candidate_version: &str, current_version: &str) -> bool {
    todo!("port utils/version-check.ts:isNewerPackageVersion")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLatestPiReleaseOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<bool>,
}

pub async fn get_latest_pi_release(
    current_version: &str,
    options: Option<&GetLatestPiReleaseOptions>,
) -> anyhow::Result<Option<LatestPiRelease>> {
    todo!("port utils/version-check.ts:getLatestPiRelease")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLatestPiVersionOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<bool>,
}

pub async fn get_latest_pi_version(
    current_version: &str,
    options: Option<&GetLatestPiVersionOptions>,
) -> anyhow::Result<Option<String>> {
    todo!("port utils/version-check.ts:getLatestPiVersion")
}

pub async fn check_for_new_pi_version(
    current_version: &str,
) -> anyhow::Result<Option<LatestPiRelease>> {
    todo!("port utils/version-check.ts:checkForNewPiVersion")
}
