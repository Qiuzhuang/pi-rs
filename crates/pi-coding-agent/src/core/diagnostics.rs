//! Port of `packages/coding-agent/src/core/diagnostics.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceCollisionResourceType {
    #[serde(rename = "extension")]
    Extension,
    #[serde(rename = "skill")]
    Skill,
    #[serde(rename = "prompt")]
    Prompt,
    #[serde(rename = "theme")]
    Theme,
}

impl ResourceCollisionResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::Skill => "skill",
            Self::Prompt => "prompt",
            Self::Theme => "theme",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceCollision {
    pub resource_type: ResourceCollisionResourceType,
    pub name: String,
    pub winner_path: String,
    pub loser_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loser_source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceDiagnosticType {
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "collision")]
    Collision,
}

impl ResourceDiagnosticType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Collision => "collision",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDiagnostic {
    pub r#type: ResourceDiagnosticType,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision: Option<ResourceCollision>,
}
