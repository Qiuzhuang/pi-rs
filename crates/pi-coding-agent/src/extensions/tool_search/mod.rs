//! Port of `packages/coding-agent/src/extensions/tool-search/index.ts`.
//!
//! The `tool_search` tool as an extension. The CLI loads it as a built-in extension; SDK users add
//! `create_tool_search_extension()` to their extension factories.
//!
//! `tool_search` is registered inactive. Activate it with `--tools`, the `defaultTools` setting, or
//! `set_active_tools()`.

pub mod tool;

#[cfg(test)]
mod tests;

use crate::core::extensions::types::{ExtensionAPIRef, ExtensionFactory};
use std::sync::Arc;
use tool::{ToolSearchToolOptions, ToolSearchToolOptionsTools, create_tool_search_tool_definition};

pub fn create_tool_search_extension() -> ExtensionFactory {
    Arc::new(|pi: ExtensionAPIRef| {
        Box::pin(async move {
            let mut definition = create_tool_search_tool_definition(Some(&ToolSearchToolOptions {
                tools: Some(ToolSearchToolOptionsTools::from_api(pi.clone())),
            }))?;
            definition.default_active = Some(false);
            pi.register_tool(definition)
        })
    })
}
