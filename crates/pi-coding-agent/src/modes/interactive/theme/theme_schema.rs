//! Port of `packages/coding-agent/src/modes/interactive/theme/theme-schema.ts`.

use crate::modes::interactive::theme::theme::TerminalTheme;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

pub static COLOR_VALUE_SCHEMA: LazyLock<Value> =
    LazyLock::new(|| todo!("port modes/interactive/theme/theme-schema.ts:ColorValueSchema"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemeColorValueValue {
    String(String),
    Number(i64),
}

pub type ThemeColorValue = ThemeColorValueValue;

pub static THEME_JSON_SCHEMA: LazyLock<Value> =
    LazyLock::new(|| todo!("port modes/interactive/theme/theme-schema.ts:ThemeJsonSchema"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ValidatedThemeJsonValueExportPageBg {
    String(String),
    Number(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ValidatedThemeJsonValueExportCardBg {
    String(String),
    Number(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ValidatedThemeJsonValueExportInfoBg {
    String(String),
    Number(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedThemeJsonValueExport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_bg: Option<ValidatedThemeJsonValueExportPageBg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_bg: Option<ValidatedThemeJsonValueExportCardBg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_bg: Option<ValidatedThemeJsonValueExportInfoBg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedThemeJsonValue {
    /// JSON schema reference
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Theme name. Must not contain '/' because it is reserved for automatic light/dark theme settings.
    pub name: String,
    /// Background the theme is designed for. Detected from the theme colors when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<TerminalTheme>,
    /// Reusable color variables
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vars: Option<IndexMap<String, ThemeColorValue>>,
    pub colors: Value,
    /// Optional colors for HTML export (defaults derived from userMessageBg if not specified)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<ValidatedThemeJsonValueExport>,
}

pub type ValidatedThemeJson = ValidatedThemeJsonValue;

/// Validate one theme document, throwing a message that names the offending tokens.
pub fn validate_theme_json(label: &str, json: Value) -> anyhow::Result<ValidatedThemeJson> {
    todo!("port modes/interactive/theme/theme-schema.ts:validateThemeJson")
}
