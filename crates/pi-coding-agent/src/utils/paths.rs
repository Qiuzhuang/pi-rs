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
    let meta = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mtime_ns = (meta.mtime() as i128) * 1_000_000_000 + meta.mtime_nsec() as i128;
        let ctime_ns = (meta.ctime() as i128) * 1_000_000_000 + meta.ctime_nsec() as i128;
        Some(format!(
            "{}:{}:{}:{}:{}",
            meta.dev(),
            meta.ino(),
            meta.size(),
            mtime_ns,
            ctime_ns
        ))
    }
    #[cfg(not(unix))]
    {
        let nanos = |t: std::io::Result<std::time::SystemTime>| -> i128 {
            t.ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as i128)
                .unwrap_or(0)
        };
        Some(format!(
            "0:0:{}:{}:{}",
            meta.len(),
            nanos(meta.modified()),
            nanos(meta.created())
        ))
    }
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
    let normalized = normalize_path(input, options);
    let base = match base_dir {
        Some(base) => normalize_path(base, None),
        None => current_dir_string(),
    };
    if is_absolute(&normalized) {
        node_resolve(&[&normalized])
    } else {
        node_resolve(&[&base, &normalized])
    }
}

pub fn get_cwd_relative_path(file_path: &str, cwd: &str) -> Option<String> {
    let resolved_cwd = resolve_path(cwd, None, None);
    let resolved_path = resolve_path(file_path, Some(&resolved_cwd), None);
    let relative_path = node_relative(&resolved_cwd, &resolved_path);
    let sep = std::path::MAIN_SEPARATOR;
    let is_inside_cwd = relative_path.is_empty()
        || (relative_path != ".."
            && !relative_path.starts_with(&format!("..{sep}"))
            && !is_absolute(&relative_path));
    if is_inside_cwd {
        Some(if relative_path.is_empty() {
            ".".to_string()
        } else {
            relative_path
        })
    } else {
        None
    }
}

pub fn format_path_relative_to_cwd_or_absolute(file_path: &str, cwd: &str) -> String {
    let absolute_path = resolve_path(file_path, Some(cwd), None);
    let path = get_cwd_relative_path(&absolute_path, cwd).unwrap_or(absolute_path);
    path.split(std::path::MAIN_SEPARATOR)
        .collect::<Vec<_>>()
        .join("/")
}

pub fn mark_path_ignored_by_cloud_sync(path: &str) {
    let attrs: &[&str] = if cfg!(target_os = "macos") {
        &["com.dropbox.ignored", "com.apple.fileprovider.ignore#P"]
    } else if cfg!(target_os = "linux") {
        &["user.com.dropbox.ignored"]
    } else {
        &[]
    };
    for attr in attrs {
        let mut command = if cfg!(target_os = "macos") {
            let mut c = std::process::Command::new("xattr");
            c.args(["-w", attr, "1", path]);
            c
        } else {
            let mut c = std::process::Command::new("setfattr");
            c.args(["-n", attr, "-v", "1", path]);
            c
        };
        let _ = command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

// ============================================================================
// Node `path` helpers (lexical, like `node:path` for the host platform)
// ============================================================================

/// The process working directory as a string (`process.cwd()`).
pub fn current_dir_string() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string())
}

/// `path.isAbsolute`.
pub fn is_absolute(path: &str) -> bool {
    if cfg!(windows) {
        let bytes = path.as_bytes();
        path.starts_with('/')
            || path.starts_with('\\')
            || (bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && (bytes[2] == b'/' || bytes[2] == b'\\'))
    } else {
        path.starts_with('/')
    }
}

fn split_root(path: &str) -> (String, &str) {
    if cfg!(windows) {
        let bytes = path.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            let rest = &path[2..];
            if rest.starts_with('/') || rest.starts_with('\\') {
                return (format!("{}\\", &path[..2].to_uppercase()), &rest[1..]);
            }
            return (path[..2].to_uppercase(), rest);
        }
        if path.starts_with('/') || path.starts_with('\\') {
            return ("\\".to_string(), &path[1..]);
        }
        (String::new(), path)
    } else if let Some(rest) = path.strip_prefix('/') {
        ("/".to_string(), rest)
    } else {
        (String::new(), path)
    }
}

/// `path.normalize` for an absolute or relative path (no trailing separator).
pub fn node_normalize(path: &str) -> String {
    let sep = std::path::MAIN_SEPARATOR;
    let (root, rest) = split_root(path);
    let mut parts: Vec<&str> = Vec::new();
    for segment in rest.split(|c| c == '/' || (cfg!(windows) && c == '\\')) {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if root.is_empty() {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join(&sep.to_string());
    if root.is_empty() {
        if joined.is_empty() {
            ".".to_string()
        } else {
            joined
        }
    } else {
        format!("{root}{joined}")
    }
}

/// `path.resolve(...segments)`: resolves right to left until an absolute path is formed, then
/// prepends `process.cwd()` when needed and normalizes the result.
pub fn node_resolve(segments: &[&str]) -> String {
    let mut resolved = String::new();
    for segment in segments.iter().rev() {
        if segment.is_empty() {
            continue;
        }
        resolved = if resolved.is_empty() {
            (*segment).to_string()
        } else {
            format!("{}{}{}", segment, std::path::MAIN_SEPARATOR, resolved)
        };
        if is_absolute(segment) {
            return node_normalize(&resolved);
        }
    }
    let cwd = current_dir_string();
    if resolved.is_empty() {
        return node_normalize(&cwd);
    }
    node_normalize(&format!("{}{}{}", cwd, std::path::MAIN_SEPARATOR, resolved))
}

/// `path.join(...segments)`.
pub fn node_join(segments: &[&str]) -> String {
    let joined = segments
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(&std::path::MAIN_SEPARATOR.to_string());
    if joined.is_empty() {
        return ".".to_string();
    }
    node_normalize(&joined)
}

/// `path.dirname`.
pub fn node_dirname(path: &str) -> String {
    let normalized = node_normalize(path);
    let (root, rest) = split_root(&normalized);
    match rest.rfind(|c| c == '/' || (cfg!(windows) && c == '\\')) {
        Some(index) => format!("{root}{}", &rest[..index]),
        None => {
            if root.is_empty() {
                ".".to_string()
            } else {
                root
            }
        }
    }
}

/// `path.basename`.
pub fn node_basename(path: &str) -> String {
    let trimmed = path.trim_end_matches(|c| c == '/' || (cfg!(windows) && c == '\\'));
    match trimmed.rfind(|c| c == '/' || (cfg!(windows) && c == '\\')) {
        Some(index) => trimmed[index + 1..].to_string(),
        None => trimmed.to_string(),
    }
}

/// `path.relative(from, to)`.
pub fn node_relative(from: &str, to: &str) -> String {
    let from = node_resolve(&[from]);
    let to = node_resolve(&[to]);
    if from == to {
        return String::new();
    }
    let (from_root, from_rest) = split_root(&from);
    let (to_root, to_rest) = split_root(&to);
    if !from_root.eq_ignore_ascii_case(&to_root) {
        return to;
    }
    let is_sep = |c: char| c == '/' || (cfg!(windows) && c == '\\');
    let from_parts: Vec<&str> = from_rest.split(is_sep).filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to_rest.split(is_sep).filter(|s| !s.is_empty()).collect();
    let mut common = 0;
    while common < from_parts.len()
        && common < to_parts.len()
        && (if cfg!(windows) {
            from_parts[common].eq_ignore_ascii_case(to_parts[common])
        } else {
            from_parts[common] == to_parts[common]
        })
    {
        common += 1;
    }
    let mut parts: Vec<&str> = Vec::new();
    for _ in common..from_parts.len() {
        parts.push("..");
    }
    parts.extend_from_slice(&to_parts[common..]);
    parts.join(&std::path::MAIN_SEPARATOR.to_string())
}
