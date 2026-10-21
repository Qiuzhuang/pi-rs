//! Port of `packages/coding-agent/src/core/tools/output-accumulator.ts`.

use crate::core::tools::truncate::TruncationResult;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputAccumulatorOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_file_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputSnapshot {
    pub content: String,
    pub truncation: TruncationResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullOutput {
    pub content: String,
    /// Whether `content` omits part of the output.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputAccumulatorOptions2 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persist_if_truncated: Option<bool>,
}

/// Incrementally tracks streaming output with bounded memory.
///
/// Appends decode chunks with a streaming UTF-8 decoder, keeps only a decoded
/// tail for display snapshots, and opens a temp file when the full output needs
/// to be preserved.
pub struct OutputAccumulator {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl OutputAccumulator {
    pub fn new(options: Option<&OutputAccumulatorOptions>) -> Self {
        todo!("port core/tools/output-accumulator.ts:OutputAccumulator.constructor")
    }

    pub fn append(&self, data: &[u8]) -> anyhow::Result<()> {
        todo!("port core/tools/output-accumulator.ts:OutputAccumulator.append")
    }

    pub fn finish(&self) {
        todo!("port core/tools/output-accumulator.ts:OutputAccumulator.finish")
    }

    pub fn snapshot(&self, options: Option<&OutputAccumulatorOptions2>) -> OutputSnapshot {
        todo!("port core/tools/output-accumulator.ts:OutputAccumulator.snapshot")
    }

    pub async fn close_temp_file(&self) -> anyhow::Result<()> {
        todo!("port core/tools/output-accumulator.ts:OutputAccumulator.closeTempFile")
    }

    /// The complete output, for callers that can take more than the display snapshot. Call after
    /// `finish()` and `closeTempFile()`. Output longer than `maxBytes` raw bytes keeps its first and
    /// last `maxBytes / 2` bytes around an omission marker.
    pub async fn read_full_output(&self, max_bytes: i64) -> anyhow::Result<FullOutput> {
        todo!("port core/tools/output-accumulator.ts:OutputAccumulator.readFullOutput")
    }

    pub fn get_last_line_bytes(&self) -> i64 {
        todo!("port core/tools/output-accumulator.ts:OutputAccumulator.getLastLineBytes")
    }
}
