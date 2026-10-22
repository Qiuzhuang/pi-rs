//! Port of `packages/coding-agent/src/extensions/codemode/index.ts`.
//!
//! The `codemode` tool as an extension. The CLI loads it as a built-in extension; SDK users
//! add `create_codemode_extension()` to their extension factories.
//!
//! `codemode` is registered inactive. Activate it with `--tools`, the `defaultTools` setting, or
//! `set_active_tools()`; the MCP extension activates it when MCP tools are only reachable from scripts.

pub mod execute;
pub mod renderer;
pub mod tool;

use crate::core::extensions::types::{ExtensionAPIRef, ExtensionFactory};
use crate::core::settings_schema::CodemodeMode;
use std::sync::Arc;
use tool::{CodemodeStoreEntryData, CodemodeToolOptions, create_codemode_tool_definition};

#[cfg(test)]
mod tests;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodemodeExtensionOptions {
    /// Overrides the `codemode.mode` setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<CodemodeMode>,
    /// Overrides the `codemode.inlineBudget` setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_budget: Option<i64>,
    /// Expose the model catalog and classifiers to scripts as `models`. Default: `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<bool>,
}

fn read_mode(pi: &ExtensionAPIRef) -> CodemodeMode {
    match pi.get_settings().ok().and_then(|s| s.codemode).and_then(|c| c.mode) {
        Some(CodemodeMode::Only) => CodemodeMode::Only,
        _ => CodemodeMode::On,
    }
}

fn read_inline_budget(pi: &ExtensionAPIRef) -> Option<f64> {
    let budget = pi.get_settings().ok()?.codemode?.inline_budget?;
    (budget >= 0).then_some(budget as f64)
}

pub fn create_codemode_extension(options: Option<&CodemodeExtensionOptions>) -> ExtensionFactory {
    let options = options.cloned().unwrap_or_default();
    Arc::new(move |pi: ExtensionAPIRef| {
        let options = options.clone();
        Box::pin(async move {
            let entry_api = pi.clone();
            let namespace_api = pi.clone();
            let guidelines_api = pi.clone();
            let mode_api = pi.clone();
            let budget_api = pi.clone();
            let mut definition = create_codemode_tool_definition(Some(&CodemodeToolOptions {
                append_entry: Some(Arc::new(move |custom_type: String, data: CodemodeStoreEntryData| {
                    let _ = entry_api.append_entry(&custom_type, serde_json::to_value(data).ok());
                })),
                models: Some(options.models.unwrap_or(true)),
                get_tool_namespace: Some(Arc::new(move |name: String| {
                    namespace_api
                        .get_all_tools()
                        .ok()?
                        .into_iter()
                        .find(|tool| tool.name == name)
                        .and_then(|tool| tool.namespace)
                })),
                get_tool_guidelines: Some(Arc::new(move || {
                    guidelines_api
                        .get_all_tools()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|tool| (tool.name, tool.prompt_guidelines.unwrap_or_default()))
                        .collect()
                })),
                get_mode: Some(Arc::new(move || options.mode.unwrap_or_else(|| read_mode(&mode_api)))),
                get_inline_budget: Some(Arc::new(move || {
                    options.inline_budget.map(|b| b as f64).or_else(|| read_inline_budget(&budget_api))
                })),
            }));
            definition.default_active = Some(false);
            pi.register_tool(definition)
        })
    })
}
