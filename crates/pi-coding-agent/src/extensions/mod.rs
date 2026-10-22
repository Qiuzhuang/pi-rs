//! Port of `packages/coding-agent/src/extensions/index.ts`.

pub mod codemode;
pub mod llama;
pub mod mcp;
pub mod tool_search;

use crate::core::extensions::types::{ExtensionAPIRef, ExtensionFactory, InlineExtension, NamedInlineExtension};
use std::sync::Arc;
use std::sync::LazyLock;

fn named(name: &str, factory: ExtensionFactory, replaceable: bool) -> InlineExtension {
    InlineExtension::Named(NamedInlineExtension {
        name: name.to_string(),
        factory,
        hidden: None,
        replaceable: replaceable.then_some(true),
        builtin: Some(true),
    })
}

pub static BUILT_IN_EXTENSIONS: LazyLock<Vec<InlineExtension>> = LazyLock::new(|| {
    let llama: ExtensionFactory =
        Arc::new(|pi: ExtensionAPIRef| Box::pin(async move { llama::llama_extension(pi) }));
    let mcp_factory = mcp::create_mcp_extension(None).expect("default MCP extension options are valid");
    vec![
        named("llama.cpp", llama, false),
        // Replaceable: an extension that registers `codemode`, `tool_search`, or `/mcp` (such as a
        // third-party MCP extension) takes over instead of running alongside the built-in one.
        named("codemode", codemode::create_codemode_extension(None), true),
        named("tool-search", tool_search::create_tool_search_extension(), true),
        named("mcp", mcp_factory, true),
    ]
});
