//! Port of `packages/coding-agent/src/extensions/codemode/index.ts`.

pub mod execute;
pub mod renderer;
pub mod tool;

use crate::core::extensions::types::ExtensionFactory;
use crate::core::settings_schema::CodemodeMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub fn create_codemode_extension(options: Option<&CodemodeExtensionOptions>) -> ExtensionFactory {
    todo!("port extensions/codemode/index.ts:createCodemodeExtension")
}
