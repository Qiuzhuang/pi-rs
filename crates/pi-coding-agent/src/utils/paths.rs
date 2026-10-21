//! Port of `packages/coding-agent/src/utils/paths.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathInputOptions {
    /// Trim leading/trailing whitespace before normalization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<bool>,
    /// Expand leading `~` to a home directory. Defaults to true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand_tilde: Option<bool>,
    /// Home directory used for `~` expansion. Defaults to `os.homedir()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_dir: Option<String>,
    /// Strip a leading `@`, used for CLI @file paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_at_prefix: Option<bool>,
    /// Normalize unicode space variants to regular spaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalize_unicode_spaces: Option<bool>,
}

/// Resolve a path to its canonical (real) form, following symlinks.
/// Falls back to the raw path if resolution fails (e.g. the target does
/// not exist yet), so that callers never crash on missing filesystem
/// entries.
pub fn canonicalize_path(path: &str) -> String {
    todo!("port utils/paths.ts:canonicalizePath")
}

pub fn get_file_revision(path: &str) -> Option<String> {
    todo!("port utils/paths.ts:getFileRevision")
}

/// Returns true if the value is NOT a package source (npm:, git:, etc.),
/// a built-in extension (builtin:), or a remote URL protocol. Bare names,
/// relative paths, and file: URLs are considered local.
pub fn is_local_path(value: &str) -> bool {
    todo!("port utils/paths.ts:isLocalPath")
}

/// Convert Git Bash, MSYS, Cygwin, and WSL drive paths to a form native Windows APIs accept.
pub fn normalize_windows_shell_path(file_path: &str) -> String {
    todo!("port utils/paths.ts:normalizeWindowsShellPath")
}

pub fn normalize_path(input: &str, options: Option<&PathInputOptions>) -> String {
    todo!("port utils/paths.ts:normalizePath")
}

pub fn resolve_path(
    input: &str,
    base_dir: Option<&str>,
    options: Option<&PathInputOptions>,
) -> String {
    todo!("port utils/paths.ts:resolvePath")
}

pub fn get_cwd_relative_path(file_path: &str, cwd: &str) -> Option<String> {
    todo!("port utils/paths.ts:getCwdRelativePath")
}

pub fn format_path_relative_to_cwd_or_absolute(file_path: &str, cwd: &str) -> String {
    todo!("port utils/paths.ts:formatPathRelativeToCwdOrAbsolute")
}

pub fn mark_path_ignored_by_cloud_sync(path: &str) {
    todo!("port utils/paths.ts:markPathIgnoredByCloudSync")
}
