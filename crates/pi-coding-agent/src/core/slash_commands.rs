//! Port of `packages/coding-agent/src/core/slash-commands.ts`.

use crate::core::source_info::SourceInfo;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SlashCommandSource {
    #[serde(rename = "extension")]
    Extension,
    #[serde(rename = "prompt")]
    Prompt,
    #[serde(rename = "skill")]
    Skill,
}

impl SlashCommandSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::Prompt => "prompt",
            Self::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommandInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source: SlashCommandSource,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinSlashCommand {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
}

pub static BUILTIN_SLASH_COMMANDS: LazyLock<Vec<BuiltinSlashCommand>> =
    LazyLock::new(|| todo!("port core/slash-commands.ts:BUILTIN_SLASH_COMMANDS"));
