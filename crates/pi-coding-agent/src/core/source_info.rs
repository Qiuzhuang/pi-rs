//! Port of `packages/coding-agent/src/core/source-info.ts`.

use crate::core::package_manager::PathMetadata;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceScope {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "project")]
    Project,
    #[serde(rename = "temporary")]
    Temporary,
}

impl SourceScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Temporary => "temporary",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceOrigin {
    #[serde(rename = "package")]
    Package,
    #[serde(rename = "top-level")]
    TopLevel,
}

impl SourceOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::TopLevel => "top-level",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub path: String,
    pub source: String,
    pub scope: SourceScope,
    pub origin: SourceOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,
}

/// Prefix of built-in tool and extension paths, such as `builtin:read` or `builtin:mcp`.
pub const BUILTIN_PATH_PREFIX: &str = "builtin:";

/// Source of a path that names no file: `builtin` for `builtin:<name>`, or the prefix of an
/// angle-bracket path such as `inline` for `<inline:name>`. Undefined for file paths.
pub fn get_synthetic_path_source(path: &str) -> Option<String> {
    todo!("port core/source-info.ts:getSyntheticPathSource")
}

pub fn is_synthetic_path(path: &str) -> bool {
    todo!("port core/source-info.ts:isSyntheticPath")
}

pub fn create_source_info(path: &str, metadata: &PathMetadata) -> SourceInfo {
    todo!("port core/source-info.ts:createSourceInfo")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSyntheticSourceInfoOptions {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<SourceScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<SourceOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,
}

pub fn create_synthetic_source_info(
    path: &str,
    options: &CreateSyntheticSourceInfoOptions,
) -> SourceInfo {
    todo!("port core/source-info.ts:createSyntheticSourceInfo")
}
