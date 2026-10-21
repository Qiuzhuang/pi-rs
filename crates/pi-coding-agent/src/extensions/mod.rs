//! Port of `packages/coding-agent/src/extensions/index.ts`.

pub mod codemode;
pub mod llama;
pub mod mcp;
pub mod tool_search;

use crate::core::extensions::types::InlineExtension;
use std::sync::LazyLock;

pub static BUILT_IN_EXTENSIONS: LazyLock<Vec<InlineExtension>> =
    LazyLock::new(|| todo!("port extensions/index.ts:builtInExtensions"));
