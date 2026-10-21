//! Port of `packages/coding-agent/src/modes/interactive/theme/theme-schema.ts`.

use crate::modes::interactive::theme::theme::TerminalTheme;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use crate::modes::interactive::theme::theme_tokens::{THEME_TOKENS, ThemeColorValues};
use std::sync::LazyLock;

fn color_value_schema(description: Option<&str>) -> Value {
    let mut schema = json!({
        "anyOf": [
            {
                "type": "string",
                "description": "Hex color (#RGB or #RRGGBB), OKLCH or OKHSL color, variable reference, or empty string for terminal default"
            },
            { "type": "integer", "minimum": 0, "maximum": 255, "description": "256-color palette index (0-255)" }
        ]
    });
    if let Some(description) = description {
        schema["description"] = Value::String(description.to_string());
    }
    schema
}

/// JSON schema of one color value (TypeBox `ColorValueSchema`).
pub static COLOR_VALUE_SCHEMA: LazyLock<Value> = LazyLock::new(|| color_value_schema(None));

/// A theme color value: a color string or a 256-color palette index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemeColorValue {
    String(String),
    Index(i64),
}

/// The published theme JSON schema (TypeBox `ThemeJsonSchema`).
pub static THEME_JSON_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (token, descriptor) in THEME_TOKENS {
        properties.insert(token.as_str().to_string(), color_value_schema(Some(descriptor.description)));
        if descriptor.fallback.is_none() {
            required.push(Value::String(token.as_str().to_string()));
        }
    }
    json!({
        "title": "Pi Coding Agent Theme",
        "description": "Theme schema for Pi coding agent",
        "additionalProperties": false,
        "type": "object",
        "properties": {
            "$schema": { "type": "string", "description": "JSON schema reference" },
            "name": {
                "type": "string",
                "pattern": "^[^/]+$",
                "description": "Theme name. Must not contain '/' because it is reserved for automatic light/dark theme settings."
            },
            "appearance": {
                "anyOf": [{ "const": "dark", "type": "string" }, { "const": "light", "type": "string" }],
                "description": "Background the theme is designed for. Detected from the theme colors when omitted."
            },
            "vars": {
                "type": "object",
                "patternProperties": { "^.*$": color_value_schema(None) },
                "description": "Reusable color variables"
            },
            "colors": {
                "description": "Theme color definitions (scrollbar, thinkingMax, and search highlight colors are optional and use compatible fallbacks)",
                "additionalProperties": false,
                "type": "object",
                "properties": properties,
                "required": required
            },
            "export": {
                "description": "Optional colors for HTML export (defaults derived from userMessageBg if not specified)",
                "additionalProperties": false,
                "type": "object",
                "properties": {
                    "pageBg": color_value_schema(Some("Page background color")),
                    "cardBg": color_value_schema(Some("Card/container background color")),
                    "infoBg": color_value_schema(Some("Info sections background (system prompt, notices)"))
                }
            }
        },
        "required": ["name", "colors"]
    })
});

/// Optional colors for HTML export.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedThemeJsonExport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_bg: Option<ThemeColorValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_bg: Option<ThemeColorValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_bg: Option<ThemeColorValue>,
}

/// A theme document that passed [`validate_theme_json`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedThemeJson {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<TerminalTheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vars: Option<IndexMap<String, ThemeColorValue>>,
    pub colors: ThemeColorValues<ThemeColorValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<ValidatedThemeJsonExport>,
}

/// Validate one theme document, failing with a message that names the offending tokens.
pub fn validate_theme_json(label: &str, json: Value) -> anyhow::Result<ValidatedThemeJson> {
    todo!("port modes/interactive/theme/theme-schema.ts:validateThemeJson")
}
