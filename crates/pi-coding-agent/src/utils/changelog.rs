//! Port of `packages/coding-agent/src/utils/changelog.ts`.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

// Re-export getChangelogPath from config for convenience
pub use crate::config::get_changelog_path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogEntry {
    pub major: i64,
    pub minor: i64,
    pub patch: i64,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NormalizeChangelogLinksVersion {
    String(String),
    ChangelogEntry(ChangelogEntry),
}

impl From<&str> for NormalizeChangelogLinksVersion {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<&ChangelogEntry> for NormalizeChangelogLinksVersion {
    fn from(value: &ChangelogEntry) -> Self {
        Self::ChangelogEntry(value.clone())
    }
}

const GITHUB_REPO: &str = "earendil-works/pi";
const CHANGELOG_LINK_BASE_PATH: &str = "packages/coding-agent";

static LEGACY_REPO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https://github\.com/(?:badlogic|earendil-works)/pi-mono(/|$)").expect("valid regex")
});
static URL_SCHEME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^[a-z][a-z0-9+.-]*:").expect("valid regex"));
static INLINE_MARKDOWN_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(!?\[[^\]\n]+\]\()([^\s)]+)((?:\s+[^)]*)?\))").expect("valid regex")
});
static VERSION_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"##\s+\[?(\d+)\.(\d+)\.(\d+)\]?").expect("valid regex"));

fn entry_version(entry: &ChangelogEntry) -> String {
    format!("{}.{}.{}", entry.major, entry.minor, entry.patch)
}

fn normalize_tag(version: &NormalizeChangelogLinksVersion) -> String {
    let version_string = match version {
        NormalizeChangelogLinksVersion::String(s) => s.clone(),
        NormalizeChangelogLinksVersion::ChangelogEntry(entry) => entry_version(entry),
    };
    if version_string.starts_with('v') { version_string } else { format!("v{version_string}") }
}

struct LocalTarget<'a> {
    fragment: &'a str,
    path_part: &'a str,
    query: &'a str,
}

fn split_local_target(target: &str) -> LocalTarget<'_> {
    let (before_hash, fragment) = match target.find('#') {
        Some(index) => (&target[..index], &target[index..]),
        None => (target, ""),
    };
    match before_hash.find('?') {
        None => LocalTarget { fragment, path_part: before_hash, query: "" },
        Some(index) => {
            LocalTarget { fragment, path_part: &before_hash[..index], query: &before_hash[index..] }
        }
    }
}

/// `path.posix.normalize`.
pub(crate) fn posix_normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let is_absolute = path.starts_with('/');
    let trailing_slash = path.ends_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else if !is_absolute {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let mut result = parts.join("/");
    if result.is_empty() && !is_absolute {
        result = ".".to_string();
    }
    if !result.is_empty() && trailing_slash {
        result.push('/');
    }
    if is_absolute { format!("/{result}") } else { result }
}

fn posix_basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(index) => &trimmed[index + 1..],
        None => trimmed,
    }
}

/// JS `encodeURI`.
pub(crate) fn encode_uri(value: &str) -> String {
    const UNRESERVED: &[u8] = b";,/?:@&=+$-_.!~*'()#";
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || (ch.is_ascii() && UNRESERVED.contains(&(ch as u8))) {
            out.push(ch);
        } else {
            let mut buf = [0u8; 4];
            for byte in ch.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

fn resolve_repository_path(target_path: &str) -> Option<String> {
    let normalized_target = target_path.replace('\\', "/");
    let joined = if normalized_target.starts_with('/') {
        posix_normalize(normalized_target.trim_start_matches('/'))
    } else {
        posix_normalize(&format!("{CHANGELOG_LINK_BASE_PATH}/{normalized_target}"))
    };
    if joined == "." || joined.starts_with("../") || joined == ".." {
        return None;
    }
    Some(joined)
}

fn is_directory_target(original_path: &str, repository_path: &str) -> bool {
    if original_path.ends_with('/') {
        return true;
    }
    !posix_basename(repository_path).contains('.')
}

fn normalize_changelog_link_target(target: &str, tag: &str) -> String {
    let repo_url = format!("https://github.com/{GITHUB_REPO}");
    let mut canonical_target = LEGACY_REPO_RE.replace(target, format!("{repo_url}$1").as_str()).into_owned();

    for route in ["blob", "tree"] {
        for branch in ["main", "master"] {
            let floating_ref_prefix = format!("{repo_url}/{route}/{branch}/");
            if let Some(rest) = canonical_target.strip_prefix(&floating_ref_prefix) {
                canonical_target = format!("{repo_url}/{route}/{tag}/{rest}");
            }
        }
    }

    if canonical_target.starts_with('#')
        || canonical_target.starts_with("//")
        || URL_SCHEME_RE.is_match(&canonical_target)
    {
        return canonical_target;
    }

    let LocalTarget { fragment, path_part, query } = split_local_target(&canonical_target);
    if path_part.is_empty() {
        return canonical_target;
    }
    let Some(repository_path) = resolve_repository_path(path_part) else {
        return canonical_target;
    };
    let route = if is_directory_target(path_part, &repository_path) { "tree" } else { "blob" };
    format!(
        "https://github.com/{GITHUB_REPO}/{route}/{tag}/{}{query}{fragment}",
        encode_uri(&repository_path)
    )
}

pub fn normalize_changelog_links(markdown: &str, version: &NormalizeChangelogLinksVersion) -> String {
    let tag = normalize_tag(version);
    INLINE_MARKDOWN_LINK_RE
        .replace_all(markdown, |caps: &regex::Captures| {
            format!("{}{}{}", &caps[1], normalize_changelog_link_target(&caps[2], &tag), &caps[3])
        })
        .into_owned()
}

/// JS `Number.parseInt(value, 10)` for digit strings (saturating instead of losing precision).
fn parse_int(value: &str) -> i64 {
    value.parse::<i64>().unwrap_or(i64::MAX)
}

/// Parse changelog entries from CHANGELOG.md
/// Scans for ## lines and collects content until next ## or EOF
pub fn parse_changelog(changelog_path: &str) -> Vec<ChangelogEntry> {
    let path = std::path::Path::new(changelog_path);
    if !path.exists() {
        return Vec::new();
    }
    let content = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) => {
            eprintln!("Warning: Could not parse changelog: Error: {error}");
            return Vec::new();
        }
    };

    let mut entries = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_version: Option<(i64, i64, i64)> = None;

    let push_entry = |entries: &mut Vec<ChangelogEntry>, version: (i64, i64, i64), lines: &[&str]| {
        entries.push(ChangelogEntry {
            major: version.0,
            minor: version.1,
            patch: version.2,
            content: lines.join("\n").trim().to_string(),
        });
    };

    for line in content.split('\n') {
        // Check if this is a version header (## [x.y.z] ...)
        if line.starts_with("## ") {
            // Save previous entry if exists
            if let Some(version) = current_version
                && !current_lines.is_empty()
            {
                push_entry(&mut entries, version, &current_lines);
            }

            // Try to parse version from this line
            if let Some(caps) = VERSION_HEADER_RE.captures(line) {
                current_version = Some((parse_int(&caps[1]), parse_int(&caps[2]), parse_int(&caps[3])));
                current_lines = vec![line];
            } else {
                // Reset if we can't parse version
                current_version = None;
                current_lines = Vec::new();
            }
        } else if current_version.is_some() {
            // Collect lines for current version
            current_lines.push(line);
        }
    }

    // Save last entry
    if let Some(version) = current_version
        && !current_lines.is_empty()
    {
        push_entry(&mut entries, version, &current_lines);
    }

    entries
}

/// Compare versions. Returns: -1 if v1 < v2, 0 if v1 === v2, 1 if v1 > v2
/// (like TS, the magnitude is the difference of the first differing component).
pub fn compare_versions(v1: &ChangelogEntry, v2: &ChangelogEntry) -> i64 {
    if v1.major != v2.major {
        return v1.major - v2.major;
    }
    if v1.minor != v2.minor {
        return v1.minor - v2.minor;
    }
    v1.patch - v2.patch
}

/// JS `Number(part) || 0` for one version component.
fn version_part(part: Option<&str>) -> i64 {
    let Some(part) = part else { return 0 };
    let trimmed = part.trim();
    if trimmed.is_empty() {
        return 0;
    }
    match trimmed.parse::<f64>() {
        Ok(value) if value.is_finite() => value.trunc() as i64,
        _ => 0,
    }
}

/// Get entries newer than lastVersion
pub fn get_new_entries(entries: &[ChangelogEntry], last_version: &str) -> Vec<ChangelogEntry> {
    let mut parts = last_version.split('.');
    let last = ChangelogEntry {
        major: version_part(parts.next()),
        minor: version_part(parts.next()),
        patch: version_part(parts.next()),
        content: String::new(),
    };
    entries.iter().filter(|entry| compare_versions(entry, &last) > 0).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> ChangelogEntry {
        ChangelogEntry { major: 0, minor: 79, patch: 0, content: String::new() }
    }

    #[test]
    fn rewrites_package_relative_changelog_links_to_tag_pinned_github_source_links() {
        let markdown = [
            "[Project Trust](README.md#project-trust)",
            "[Extensions](docs/extensions.md#project_trust)",
            "[Examples](examples/extensions/)",
            "[Root README](../../README.md#supply-chain-hardening)",
        ]
        .join("\n");
        assert_eq!(
            normalize_changelog_links(&markdown, &(&entry()).into()),
            [
                "[Project Trust](https://github.com/earendil-works/pi/blob/v0.79.0/packages/coding-agent/README.md#project-trust)",
                "[Extensions](https://github.com/earendil-works/pi/blob/v0.79.0/packages/coding-agent/docs/extensions.md#project_trust)",
                "[Examples](https://github.com/earendil-works/pi/tree/v0.79.0/packages/coding-agent/examples/extensions/)",
                "[Root README](https://github.com/earendil-works/pi/blob/v0.79.0/README.md#supply-chain-hardening)",
            ]
            .join("\n")
        );
    }

    #[test]
    fn canonicalizes_old_repository_urls_without_changing_external_links() {
        let markdown = [
            "[#5167](https://github.com/earendil-works/pi-mono/pull/5167)",
            "[#4163](https://github.com/badlogic/pi-mono/issues/4163)",
            "[Agent README](https://github.com/badlogic/pi-mono/blob/main/packages/agent/README.md)",
            "[External](https://example.com/docs)",
            "[Local anchor](#settings)",
        ]
        .join("\n");
        assert_eq!(
            normalize_changelog_links(&markdown, &"0.79.0".into()),
            [
                "[#5167](https://github.com/earendil-works/pi/pull/5167)",
                "[#4163](https://github.com/earendil-works/pi/issues/4163)",
                "[Agent README](https://github.com/earendil-works/pi/blob/v0.79.0/packages/agent/README.md)",
                "[External](https://example.com/docs)",
                "[Local anchor](#settings)",
            ]
            .join("\n")
        );
    }

    #[test]
    fn parses_and_filters_entries() {
        let dir = std::env::temp_dir().join(format!("pi-changelog-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("CHANGELOG.md");
        std::fs::write(&path, "# Changelog\n\n## [Unreleased]\n\n- x\n\n## [1.2.0] - 2024\n\n- a\n\n## 1.1.10\n- b\n")
            .unwrap();
        let entries = parse_changelog(path.to_str().unwrap());
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].major, entries[0].minor, entries[0].patch), (1, 2, 0));
        assert_eq!(entries[0].content, "## [1.2.0] - 2024\n\n- a");
        assert_eq!(entries[1].content, "## 1.1.10\n- b");
        let newer = get_new_entries(&entries, "1.1.10");
        assert_eq!(newer.len(), 1);
        assert_eq!(get_new_entries(&entries, "garbage").len(), 2);
        assert!(parse_changelog(dir.join("missing.md").to_str().unwrap()).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
