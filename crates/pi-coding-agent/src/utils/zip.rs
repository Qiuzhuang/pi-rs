//! Port of `packages/coding-agent/src/utils/zip.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ZipEntryData {
    String(String),
    List(Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZipEntry {
    pub name: String,
    pub data: ZipEntryData,
}

pub async fn write_zip_archive(file_path: &str, entries: &[ZipEntry]) -> anyhow::Result<()> {
    todo!("port utils/zip.ts:writeZipArchive")
}
