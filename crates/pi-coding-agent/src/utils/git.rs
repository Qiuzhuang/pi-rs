//! Port of `packages/coding-agent/src/utils/git.ts`.

use serde::{Deserialize, Serialize};

/// Parsed git URL information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSource {
    /// Always "git" for git sources
    /// Always "git".
    pub r#type: String,
    /// Clone URL (always valid for git clone, without ref suffix)
    pub repo: String,
    /// Git host domain (e.g., "github.com")
    pub host: String,
    /// Repository path (e.g., "user/repo")
    pub path: String,
    /// Git ref (branch, tag, commit) if specified
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    /// True if ref was specified (package won't be auto-updated)
    pub pinned: bool,
}

/// Parse git source into a GitSource.
///
/// Rules:
/// - With git: prefix, accept all historical shorthand forms.
/// - Without git: prefix, only accept explicit protocol URLs.
pub fn parse_git_url(source: &str) -> Option<GitSource> {
    todo!("port utils/git.ts:parseGitUrl")
}
