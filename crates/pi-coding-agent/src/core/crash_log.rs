//! Port of `packages/coding-agent/src/core/crash-log.ts`.

use crate::core::source_info::SourceInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

pub fn read_crash_log(path: Option<&str>) -> Vec<CrashRecord> {
    todo!("port core/crash-log.ts:readCrashLog")
}

// TS: Pick<Extension, "path" | "resolvedPath" | "sourceInfo">
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStackMetadata {
    pub path: String,
    pub resolved_path: String,
    pub source_info: SourceInfo,
}

/// Find loaded extensions with source files in a stack trace.
pub fn find_extension_stack_matches(
    stack: Option<&str>,
    extensions: &[ExtensionStackMetadata],
) -> Vec<String> {
    todo!("port core/crash-log.ts:findExtensionStackMatches")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordCrashCrash {
    pub kind: CrashRecordKind,
    pub error: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    pub cwd: String,
}

/// Best-effort persistence for callers that are already crashing.
pub fn record_crash(crash: &RecordCrashCrash, path: Option<&str>) -> Option<CrashRecord> {
    todo!("port core/crash-log.ts:recordCrash")
}

/// Return the newest recent crash, marking pending records as announced.
pub fn take_unnotified_crash(path: Option<&str>, now: Option<f64>) -> Option<CrashRecord> {
    todo!("port core/crash-log.ts:takeUnnotifiedCrash")
}

pub fn clear_crash_log(path: Option<&str>) {
    todo!("port core/crash-log.ts:clearCrashLog")
}
