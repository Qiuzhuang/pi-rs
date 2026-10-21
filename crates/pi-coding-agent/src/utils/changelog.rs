//! Port of `packages/coding-agent/src/utils/changelog.ts`.

use serde::{Deserialize, Serialize};

pub use crate::config::get_changelog_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogEntry {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub major: f64,
    pub minor: i64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub patch: f64,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NormalizeChangelogLinksVersion {
    String(String),
    ChangelogEntry(ChangelogEntry),
}

pub fn normalize_changelog_links(
    markdown: &str,
    version: &NormalizeChangelogLinksVersion,
) -> String {
    todo!("port utils/changelog.ts:normalizeChangelogLinks")
}

/// Parse changelog entries from CHANGELOG.md
/// Scans for ## lines and collects content until next ## or EOF
pub fn parse_changelog(changelog_path: &str) -> Vec<ChangelogEntry> {
    todo!("port utils/changelog.ts:parseChangelog")
}

/// Compare versions. Returns: -1 if v1 < v2, 0 if v1 === v2, 1 if v1 > v2
pub fn compare_versions(v1: &ChangelogEntry, v2: &ChangelogEntry) -> i64 {
    todo!("port utils/changelog.ts:compareVersions")
}

/// Get entries newer than lastVersion
pub fn get_new_entries(entries: &[ChangelogEntry], last_version: &str) -> Vec<ChangelogEntry> {
    todo!("port utils/changelog.ts:getNewEntries")
}
