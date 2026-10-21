//! Port of `packages/coding-agent/src/core/tools/path-utils.ts`.
//!
//! Also hosts lexical equivalents of Node's `path.resolve` / `path.relative`, which the tools use
//! to produce the same paths as the TS version.

use crate::utils::paths::{PathInputOptions, normalize_path};
use std::path::{Component, Path, PathBuf};

const NARROW_NO_BREAK_SPACE: char = '\u{202F}';

fn try_mac_os_screenshot_path(file_path: &str) -> String {
    // filePath.replace(/ (AM|PM)\./gi, `${NARROW_NO_BREAK_SPACE}$1.`)
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?i) (AM|PM)\.").unwrap());
    RE.replace_all(file_path, format!("{NARROW_NO_BREAK_SPACE}$1."))
        .into_owned()
}

fn try_nfd_variant(file_path: &str) -> String {
    // macOS stores filenames in NFD (decomposed) form, try converting user input to NFD.
    use unicode_normalization::UnicodeNormalization;
    file_path.nfd().collect()
}

fn try_curly_quote_variant(file_path: &str) -> String {
    // macOS uses U+2019 (right single quotation mark) in screenshot names like "Capture d'écran".
    file_path.replace('\'', "\u{2019}")
}

fn file_exists(file_path: &str) -> bool {
    crate::core::tools::node_fs::access_sync(file_path, crate::core::tools::node_fs::F_OK).is_ok()
}

pub async fn path_exists(file_path: &str) -> anyhow::Result<bool> {
    Ok(crate::core::tools::node_fs::access(file_path, crate::core::tools::node_fs::F_OK)
        .await
        .is_ok())
}

fn tool_path_options() -> PathInputOptions {
    PathInputOptions {
        normalize_unicode_spaces: Some(true),
        strip_at_prefix: Some(true),
        ..Default::default()
    }
}

pub fn expand_path(file_path: &str) -> String {
    normalize_path(file_path, Some(&tool_path_options()))
}

/// Lexically normalize an absolute path like Node's `path.resolve` does (`.`/`..` segments,
/// duplicate and trailing separators).
fn normalize_absolute(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(Component::RootDir.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(Component::RootDir.as_os_str());
    }
    out
}

/// Node's `path.resolve(...segments)`: resolves right to left until an absolute path is formed,
/// falling back to the process working directory, then normalizes lexically.
pub fn node_path_resolve(segments: &[&str]) -> String {
    let mut resolved = PathBuf::new();
    for segment in segments.iter().rev() {
        if segment.is_empty() {
            continue;
        }
        resolved = if resolved.as_os_str().is_empty() {
            PathBuf::from(segment)
        } else {
            Path::new(segment).join(&resolved)
        };
        if resolved.is_absolute() {
            break;
        }
    }
    if !resolved.is_absolute() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        resolved = cwd.join(resolved);
    }
    normalize_absolute(&resolved).to_string_lossy().into_owned()
}

/// Node's `path.relative(from, to)` for resolved paths.
pub fn node_path_relative(from: &str, to: &str) -> String {
    let from = node_path_resolve(&[from]);
    let to = node_path_resolve(&[to]);
    if from == to {
        return String::new();
    }
    let from_parts: Vec<Component> = Path::new(&from).components().collect();
    let to_parts: Vec<Component> = Path::new(&to).components().collect();
    let mut common = 0;
    while common < from_parts.len() && common < to_parts.len() && from_parts[common] == to_parts[common] {
        common += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from_parts.len() {
        parts.push("..".to_string());
    }
    for part in &to_parts[common..] {
        parts.push(part.as_os_str().to_string_lossy().into_owned());
    }
    parts.join(std::path::MAIN_SEPARATOR_STR)
}

/// Node's `path.join(...segments)` (normalized, relative stays relative).
pub fn node_path_join(segments: &[&str]) -> String {
    let joined = segments
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(std::path::MAIN_SEPARATOR_STR);
    if joined.is_empty() {
        return ".".to_string();
    }
    let path = Path::new(&joined);
    if path.is_absolute() {
        let normalized = normalize_absolute(path).to_string_lossy().into_owned();
        if joined.ends_with('/') && !normalized.ends_with('/') {
            return format!("{normalized}/");
        }
        return normalized;
    }
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.last().is_some_and(|p| p != "..") {
                    parts.pop();
                } else {
                    parts.push("..".to_string());
                }
            }
            other => parts.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    let mut out = parts.join(std::path::MAIN_SEPARATOR_STR);
    if out.is_empty() {
        out = ".".to_string();
    }
    if joined.ends_with('/') && !out.ends_with('/') {
        out.push('/');
    }
    out
}

/// `resolvePath(input, baseDir, options)` from `utils/paths.ts`.
pub(crate) fn resolve_path_with_options(input: &str, base_dir: &str, options: &PathInputOptions) -> String {
    let normalized = normalize_path(input, Some(options));
    let normalized_base_dir = normalize_path(base_dir, None);
    if Path::new(&normalized).is_absolute() {
        node_path_resolve(&[&normalized])
    } else {
        node_path_resolve(&[&normalized_base_dir, &normalized])
    }
}

/// Resolve a path relative to the given cwd.
/// Handles ~ expansion and absolute paths.
pub fn resolve_to_cwd(file_path: &str, cwd: &str) -> String {
    resolve_path_with_options(file_path, cwd, &tool_path_options())
}

fn read_path_variants(resolved: &str) -> Vec<String> {
    let am_pm_variant = try_mac_os_screenshot_path(resolved);
    let nfd_variant = try_nfd_variant(resolved);
    let curly_variant = try_curly_quote_variant(resolved);
    let nfd_curly_variant = try_curly_quote_variant(&nfd_variant);
    vec![am_pm_variant, nfd_variant, curly_variant, nfd_curly_variant]
}

pub fn resolve_read_path(file_path: &str, cwd: &str) -> String {
    let resolved = resolve_to_cwd(file_path, cwd);
    if file_exists(&resolved) {
        return resolved;
    }
    for variant in read_path_variants(&resolved) {
        if variant != resolved && file_exists(&variant) {
            return variant;
        }
    }
    resolved
}

pub async fn resolve_read_path_async(file_path: &str, cwd: &str) -> anyhow::Result<String> {
    let resolved = resolve_to_cwd(file_path, cwd);
    if path_exists(&resolved).await? {
        return Ok(resolved);
    }
    for variant in read_path_variants(&resolved) {
        if variant != resolved && path_exists(&variant).await? {
            return Ok(variant);
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_path_helpers() {
        assert_eq!(node_path_resolve(&["/a/b", "../c/./d/"]), "/a/c/d");
        assert_eq!(node_path_resolve(&["/a", "/x//y"]), "/x/y");
        assert_eq!(node_path_relative("/a/b", "/a/b/c/d"), "c/d");
        assert_eq!(node_path_relative("/a/b", "/a/x"), "../x");
        assert_eq!(node_path_relative("/a/b", "/a/b"), "");
        assert_eq!(node_path_join(&["/a/b", "c"]), "/a/b/c");
        assert_eq!(node_path_join(&["a", "../b"]), "b");
    }
}
