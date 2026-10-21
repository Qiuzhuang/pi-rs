//! Port of `packages/coding-agent/src/utils/output-files.ts`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WriteOutputFileData {
    String(String),
    List(Vec<u8>),
}

/// Write `data` to a new output file and return its path.
pub async fn write_output_file(
    prefix: &str,
    extension: &str,
    data: &WriteOutputFileData,
) -> anyhow::Result<String> {
    todo!("port utils/output-files.ts:writeOutputFile")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOutputFileStreamCreateOutputFileStreamResult {
    pub path: String,
    pub stream: Value,
}

/// Open a new output file for streamed output.
pub fn create_output_file_stream(
    prefix: &str,
    extension: &str,
) -> CreateOutputFileStreamCreateOutputFileStreamResult {
    todo!("port utils/output-files.ts:createOutputFileStream")
}
