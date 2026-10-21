//! Port of `packages/coding-agent/src/extensions/llama/index.ts`.

pub mod client;
pub mod huggingface;
pub mod provider;
pub mod ui;

use crate::core::extensions::types::ExtensionAPI;
use std::sync::Arc;

pub fn llama_extension(pi: Arc<dyn ExtensionAPI>) -> anyhow::Result<()> {
    todo!("port extensions/llama/index.ts:llamaExtension")
}
