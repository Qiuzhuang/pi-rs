//! Port of `packages/coding-agent/src/core/crash-log.ts`.

use crate::config::{VERSION, get_agent_dir};
use crate::core::source_info::{SourceInfo, SourceOrigin, is_synthetic_path};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CrashRecordKind {
    #[serde(rename = "uncaught_exception")]
    UncaughtException,
    #[serde(rename = "fatal_error")]
    FatalError,
}

impl CrashRecordKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UncaughtException => "uncaught_exception",
            Self::FatalError => "fatal_error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashRecord {
    pub timestamp: String,
    pub version: String,
    pub kind: CrashRecordKind,
    pub message: String,
    #[serde(default)]
    pub stack: Option<String>,
    #[serde(default)]
    pub session_file: Option<String>,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notified: Option<bool>,
}

const MAX_CRASH_RECORDS: usize = 5;
const MAX_AGE: f64 = 7.0 * 24.0 * 60.0 * 60.0 * 1000.0;

fn crash_log_path() -> PathBuf {
    Path::new(&get_agent_dir()).join("crashes.json")
}

fn resolve_path(path: Option<&str>) -> PathBuf {
    path.map(PathBuf::from).unwrap_or_else(crash_log_path)
}

pub fn read_crash_log(path: Option<&str>) -> Vec<CrashRecord> {
    let path = resolve_path(path);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(Value::Array(records)) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    records
        .into_iter()
        .filter(|record| {
            record.get("timestamp").is_some_and(Value::is_string)
                && record.get("message").is_some_and(Value::is_string)
        })
        .filter_map(|record| serde_json::from_value::<CrashRecord>(record).ok())
        .collect()
}

fn write_crash_log(records: &[CrashRecord], path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(records).map_err(std::io::Error::other)?;
    std::fs::write(path, format!("{json}\n"))
}

// TS: Pick<Extension, "path" | "resolvedPath" | "sourceInfo">
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStackMetadata {
    pub path: String,
    pub resolved_path: String,
    pub source_info: SourceInfo,
}

fn normalize_stack_path(value: &str) -> String {
    value.replace('\\', "/").trim_end_matches('/').to_string()
}

fn is_windows_drive_path(target: &str) -> bool {
    let bytes = target.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

fn stack_contains_path(stack: &str, target_path: &str, include_descendants: bool) -> bool {
    let target = normalize_stack_path(target_path);
    if target.is_empty() || is_synthetic_path(&target) {
        return false;
    }
    let case_insensitive = is_windows_drive_path(&target);
    let haystack = if case_insensitive {
        stack.to_lowercase()
    } else {
        stack.to_string()
    };
    let needle = if case_insensitive {
        target.to_lowercase()
    } else {
        target
    };
    if include_descendants {
        return haystack.contains(&format!("{needle}/"));
    }
    let mut start = 0;
    while let Some(offset) = haystack[start..].find(&needle) {
        let index = start + offset;
        let next = haystack[index + needle.len()..].chars().next();
        match next {
            None => return true,
            Some(c) if c == ':' || c == ')' || c.is_whitespace() => return true,
            _ => {}
        }
        start = index + needle.len();
    }
    false
}

/// JS `decodeURI`: decodes percent escapes except reserved URI characters. Errors on malformed
/// escapes or invalid UTF-8.
fn decode_uri(input: &str) -> Option<String> {
    const RESERVED: &[u8] = b";/?:@&=+$,#";
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = input.get(i + 1..i + 3)?;
            let value = u8::from_str_radix(hex, 16).ok()?;
            if value < 0x80 && RESERVED.contains(&value) {
                out.extend_from_slice(&bytes[i..i + 3]);
            } else {
                out.push(value);
            }
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn is_stack_frame_line(line: &str) -> bool {
    // /^\s+at\s/
    let trimmed = line.trim_start();
    trimmed.len() < line.len()
        && trimmed
            .strip_prefix("at")
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

fn is_remote_source(source: &str) -> bool {
    ["npm:", "git:", "http://", "https://", "ssh://"]
        .iter()
        .any(|prefix| source.starts_with(prefix))
}

fn is_script_file(path: &str) -> bool {
    // /\.[cm]?[jt]s$/
    [".js", ".ts", ".cjs", ".mjs", ".cts", ".mts"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

fn is_directory_entry(path: &str) -> bool {
    // /\/index\.[cm]?[jt]s$/
    ["js", "ts", "cjs", "mjs", "cts", "mts"]
        .iter()
        .any(|ext| path.ends_with(&format!("/index.{ext}")))
}

/// Find loaded extensions with source files in a stack trace.
pub fn find_extension_stack_matches(
    stack: Option<&str>,
    extensions: &[ExtensionStackMetadata],
) -> Vec<String> {
    let Some(stack) = stack.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let normalized_stack = stack
        .split('\n')
        .skip(1)
        .filter(|line| is_stack_frame_line(line))
        .map(|line| decode_uri(line).unwrap_or_else(|| line.to_string()))
        .collect::<Vec<_>>()
        .join("\n")
        .replace('\\', "/");
    let mut matches = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for extension in extensions {
        let resolved_path = normalize_stack_path(&extension.resolved_path);
        let is_package = extension.source_info.origin == SourceOrigin::Package;
        let single_file_package = is_package
            && !is_remote_source(&extension.source_info.source)
            && is_script_file(&extension.source_info.source);
        let package_root = if is_package && !single_file_package {
            extension
                .source_info
                .base_dir
                .as_deref()
                .filter(|d| !d.is_empty())
        } else {
            None
        };
        let slash_index = resolved_path.rfind('/');
        let directory_entry = is_directory_entry(&resolved_path);
        let matched = if let Some(root) = package_root {
            stack_contains_path(&normalized_stack, root, true)
        } else if let (true, Some(slash)) = (directory_entry, slash_index) {
            stack_contains_path(&normalized_stack, &resolved_path[..slash], true)
        } else {
            stack_contains_path(&normalized_stack, &resolved_path, false)
        };
        if !matched {
            continue;
        }
        let label = if is_package && !extension.source_info.source.is_empty() {
            extension.source_info.source.clone()
        } else {
            extension.path.clone()
        };
        if seen.insert(label.clone()) {
            matches.push(label);
        }
    }
    matches
}

/// `{ kind, error, sessionFile?, cwd }`. `error` is the thrown value: a string, or an object
/// with `message`, `name` and `stack` (as produced from a Rust error by [`error_to_value`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordCrashCrash {
    pub kind: CrashRecordKind,
    pub error: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    pub cwd: String,
}

/// Describe an error as the JSON shape [`record_crash`] reads (`{ name, message, stack }`).
pub fn error_to_value(error: &anyhow::Error) -> Value {
    // A panic carries where it happened, like a JS error's stack.
    if let Some(panic) = error.downcast_ref::<crate::utils::uncaught::PanicError>() {
        return serde_json::json!({ "name": "Error", "message": panic.message, "stack": panic.stack });
    }
    let chain: Vec<String> = error.chain().skip(1).map(|e| e.to_string()).collect();
    let mut stack = format!("Error: {error}");
    for cause in chain {
        stack.push_str(&format!("\n    caused by: {cause}"));
    }
    let backtrace = error.backtrace().to_string();
    if !backtrace.is_empty() && !backtrace.starts_with("disabled") {
        stack.push('\n');
        stack.push_str(&backtrace);
    }
    serde_json::json!({ "name": "Error", "message": error.to_string(), "stack": stack })
}

fn error_message(error: &Value) -> (String, Option<String>) {
    match error {
        Value::Object(map) if map.contains_key("message") || map.contains_key("stack") => {
            let message = map.get("message").and_then(Value::as_str).unwrap_or("");
            let message = if message.is_empty() {
                map.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Error")
                    .to_string()
            } else {
                message.to_string()
            };
            let stack = map
                .get("stack")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            (message, stack)
        }
        Value::String(s) => (s.clone(), None),
        Value::Null => ("null".to_string(), None),
        other => (other.to_string(), None),
    }
}

/// Best-effort persistence for callers that are already crashing.
pub fn record_crash(crash: &RecordCrashCrash, path: Option<&str>) -> Option<CrashRecord> {
    let path_buf = resolve_path(path);
    let (message, stack) = error_message(&crash.error);
    let record = CrashRecord {
        timestamp: chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
        version: VERSION.to_string(),
        kind: crash.kind,
        message,
        stack,
        session_file: crash.session_file.clone(),
        cwd: crash.cwd.clone(),
        notified: None,
    };
    let mut records = read_crash_log(Some(&path_buf.to_string_lossy()));
    records.push(record.clone());
    let start = records.len().saturating_sub(MAX_CRASH_RECORDS);
    write_crash_log(&records[start..], &path_buf).ok()?;
    Some(record)
}

fn parse_timestamp_ms(timestamp: &str) -> Option<f64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|d| d.timestamp_millis() as f64)
}

/// Return the newest recent crash, marking pending records as announced.
pub fn take_unnotified_crash(path: Option<&str>, now: Option<f64>) -> Option<CrashRecord> {
    let path_buf = resolve_path(path);
    let now = now.unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as f64);
    let records = read_crash_log(Some(&path_buf.to_string_lossy()));
    let crash = records
        .iter()
        .rev()
        .find(|record| {
            record.notified != Some(true)
                && parse_timestamp_ms(&record.timestamp).is_some_and(|t| now - t <= MAX_AGE)
        })?
        .clone();
    let updated: Vec<CrashRecord> = records
        .into_iter()
        .map(|mut record| {
            if record.notified != Some(true) {
                record.notified = Some(true);
            }
            record
        })
        .collect();
    // Showing the notice again is harmless.
    let _ = write_crash_log(&updated, &path_buf);
    Some(crash)
}

pub fn clear_crash_log(path: Option<&str>) {
    // The records can be attached again if cleanup fails.
    let _ = std::fs::remove_file(resolve_path(path));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::source_info::SourceScope;

    fn package_extension(source: &str, base_dir: &str, entry: &str) -> ExtensionStackMetadata {
        let normalized = base_dir.replace('\\', "/");
        let resolved = format!("{normalized}/{entry}");
        ExtensionStackMetadata {
            path: resolved.clone(),
            resolved_path: resolved.clone(),
            source_info: SourceInfo {
                path: resolved,
                source: source.into(),
                scope: SourceScope::User,
                origin: SourceOrigin::Package,
                base_dir: Some(base_dir.into()),
            },
        }
    }

    #[test]
    fn matches_stack_frames_beneath_loaded_package_roots() {
        let memory = package_extension(
            "npm:pi-observational-memory",
            "/home/fedora/.pi/agent/npm/node_modules/pi-observational-memory",
            "extensions/index.ts",
        );
        let unrelated = package_extension(
            "npm:unrelated",
            "/home/fedora/.pi/agent/npm/node_modules/unrelated",
            "extensions/index.ts",
        );
        let stack = "TypeError: Cannot read properties of undefined (reading 'runtime')\n    at streamSimple (file:///home/fedora/.local/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/chunks/chunk-CMRUVXTE.js:1093:16944)\n    at /home/fedora/.pi/agent/npm/node_modules/pi-observational-memory/src/agents/worker-stream.ts:43:45";
        assert_eq!(
            find_extension_stack_matches(Some(stack), &[memory, unrelated]),
            vec!["npm:pi-observational-memory"]
        );
    }

    #[test]
    fn normalizes_windows_paths_and_deduplicates() {
        let root = "C:\\Users\\reporter\\.pi\\agent\\npm\\node_modules\\@scope\\memory";
        let first = package_extension("npm:@scope/memory", root, "extensions/first.ts");
        let second = package_extension("npm:@scope/memory", root, "extensions/second.ts");
        let stack = "Error: broken\n    at run (c:\\users\\reporter\\.pi\\agent\\npm\\node_modules\\@scope\\memory\\src\\worker.ts:4:2)";
        assert_eq!(
            find_extension_stack_matches(Some(stack), &[first, second]),
            vec!["npm:@scope/memory"]
        );
    }

    #[test]
    fn does_not_attribute_sibling_single_file_packages() {
        let ext = |name: &str| ExtensionStackMetadata {
            path: format!("/plugins/{name}.ts"),
            resolved_path: format!("/plugins/{name}.ts"),
            source_info: SourceInfo {
                path: format!("/plugins/{name}.ts"),
                source: format!("/plugins/{name}.ts"),
                scope: SourceScope::User,
                origin: SourceOrigin::Package,
                base_dir: Some("/plugins".into()),
            },
        };
        let stack = "Error: broken\n    at run (file:///plugins/b.ts:4:2)";
        assert_eq!(
            find_extension_stack_matches(Some(stack), &[ext("a"), ext("b")]),
            vec!["/plugins/b.ts"]
        );
    }

    #[test]
    fn ignores_extension_paths_in_the_error_message() {
        let extension = package_extension("npm:memory", "/tmp/node_modules/memory", "extensions/index.ts");
        let stack = format!(
            "Error: Failed to read {}\n    at run (file:///opt/pi/dist/core.js:4:2)",
            extension.resolved_path
        );
        assert!(find_extension_stack_matches(Some(&stack), &[extension]).is_empty());
    }

    #[test]
    fn decodes_frame_paths_independently_from_malformed_error_text() {
        let path = "/Users/reporter/.pi/agent/extensions/local memory/index.ts";
        let extension = ExtensionStackMetadata {
            path: path.into(),
            resolved_path: path.into(),
            source_info: SourceInfo {
                path: path.into(),
                source: "local".into(),
                scope: SourceScope::User,
                origin: SourceOrigin::TopLevel,
                base_dir: Some("/Users/reporter/.pi/agent/extensions".into()),
            },
        };
        let stack = "Error: progress 100%\n    at run (file:///Users/reporter/.pi/agent/extensions/local%20memory/worker.ts:4:2)";
        assert_eq!(
            find_extension_stack_matches(Some(stack), &[extension]),
            vec![path.to_string()]
        );
    }

    #[test]
    fn records_and_takes_crashes() {
        let dir = std::env::temp_dir().join(format!("pi-crash-{}", uuid::Uuid::new_v4()));
        let path = dir.join("crashes.json");
        let path_str = path.to_string_lossy().to_string();
        for i in 0..7 {
            record_crash(
                &RecordCrashCrash {
                    kind: CrashRecordKind::FatalError,
                    error: serde_json::json!({"message": format!("boom {i}"), "stack": "Error: boom"}),
                    session_file: None,
                    cwd: "/tmp".into(),
                },
                Some(&path_str),
            )
            .unwrap();
        }
        let records = read_crash_log(Some(&path_str));
        assert_eq!(records.len(), 5);
        assert_eq!(records[4].message, "boom 6");
        let crash = take_unnotified_crash(Some(&path_str), None).unwrap();
        assert_eq!(crash.message, "boom 6");
        assert!(take_unnotified_crash(Some(&path_str), None).is_none());
        clear_crash_log(Some(&path_str));
        assert!(read_crash_log(Some(&path_str)).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
