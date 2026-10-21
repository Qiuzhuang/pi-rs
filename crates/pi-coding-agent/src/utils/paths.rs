//! Port of `packages/coding-agent/src/utils/paths.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

pub fn get_file_revision(path: &str) -> Option<String> {
    todo!("port utils/paths.ts:getFileRevision")
}

/// Returns true if the value is NOT a package source (npm:, git:, etc.),
/// a built-in extension (builtin:), or a remote URL protocol. Bare names,
/// relative paths, and file: URLs are considered local.
pub fn is_local_path(value: &str) -> bool {
    let trimmed = value.trim();
    const NON_LOCAL: &[&str] = &[
        "npm:", "git:", "github:", "http:", "https:", "ssh:", "builtin:",
    ];
    !NON_LOCAL.iter().any(|prefix| trimmed.starts_with(prefix))
}

/// Convert Git Bash, MSYS, Cygwin, and WSL drive paths to a form native Windows APIs accept.
pub fn normalize_windows_shell_path(file_path: &str) -> String {
    if !file_path.starts_with('/') || file_path.starts_with("//") || file_path.contains('\\') {
        return file_path.to_string();
    }
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^/(?:mnt/|cygdrive/)?([a-z])(?:/(.*))?$").unwrap()
    });
    let Some(caps) = RE.captures(file_path) else {
        return file_path.to_string();
    };
    let drive = caps[1].to_uppercase();
    let suffix = caps
        .get(2)
        .map(|m| m.as_str().replace('/', "\\"))
        .unwrap_or_default();
    format!("{drive}:\\{suffix}")
}

pub fn normalize_path(input: &str, options: Option<&PathInputOptions>) -> String {
    let default_options = PathInputOptions::default();
    let options = options.unwrap_or(&default_options);
    let mut normalized = if options.trim.unwrap_or(false) {
        input.trim().to_string()
    } else {
        input.to_string()
    };
    if options.normalize_unicode_spaces.unwrap_or(false) {
        normalized = normalized
            .chars()
            .map(|c| match c {
                '\u{00A0}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
                c => c,
            })
            .collect();
    }
    if options.strip_at_prefix.unwrap_or(false) && normalized.starts_with('@') {
        normalized.remove(0);
    }
    if cfg!(windows) {
        normalized = normalize_windows_shell_path(&normalized);
    }
    if options.expand_tilde.unwrap_or(true) {
        let home = options.home_dir.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        if normalized == "~" {
            return home;
        }
        if normalized.starts_with("~/") || (cfg!(windows) && normalized.starts_with("~\\")) {
            return std::path::Path::new(&home)
                .join(&normalized[2..])
                .to_string_lossy()
                .into_owned();
        }
    }
    if normalized.starts_with("file://")
        && let Ok(url) = url::Url::parse(&normalized)
        && let Ok(path) = url.to_file_path()
    {
        return path.to_string_lossy().into_owned();
    }
    normalized
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
