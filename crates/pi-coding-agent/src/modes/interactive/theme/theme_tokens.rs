//! Port of `packages/coding-agent/src/modes/interactive/theme/theme-tokens.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThemeTokenDescriptorSlot {
    #[serde(rename = "foreground")]
    Foreground,
    #[serde(rename = "background")]
    Background,
}

impl ThemeTokenDescriptorSlot {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Foreground => "foreground",
            Self::Background => "background",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeTokenDescriptor {
    pub slot: ThemeTokenDescriptorSlot,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

pub type ThemeTokenDescriptors = IndexMap<String, ThemeTokenDescriptor>;

/// Theme token metadata in the same order used by theme files and the published JSON Schema.
pub static THEME_TOKENS: LazyLock<Value> =
    LazyLock::new(|| todo!("port modes/interactive/theme/theme-tokens.ts:THEME_TOKENS"));

pub type ThemeToken = String;

pub type ThemeColor = Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThemeBgValue {
    #[serde(rename = "selectedBg")]
    SelectedBg,
    #[serde(rename = "searchMatchBg")]
    SearchMatchBg,
    #[serde(rename = "userMessageBg")]
    UserMessageBg,
    #[serde(rename = "customMessageBg")]
    CustomMessageBg,
    #[serde(rename = "toolPendingBg")]
    ToolPendingBg,
    #[serde(rename = "toolSuccessBg")]
    ToolSuccessBg,
    #[serde(rename = "toolErrorBg")]
    ToolErrorBg,
}

impl ThemeBgValue {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SelectedBg => "selectedBg",
            Self::SearchMatchBg => "searchMatchBg",
            Self::UserMessageBg => "userMessageBg",
            Self::CustomMessageBg => "customMessageBg",
            Self::ToolPendingBg => "toolPendingBg",
            Self::ToolSuccessBg => "toolSuccessBg",
            Self::ToolErrorBg => "toolErrorBg",
        }
    }
}

pub type ThemeBg = ThemeBgValue;

pub type OptionalToken = Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RequiredTokenValue {
    #[serde(rename = "accent")]
    Accent,
    #[serde(rename = "border")]
    Border,
    #[serde(rename = "borderAccent")]
    BorderAccent,
    #[serde(rename = "borderMuted")]
    BorderMuted,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "muted")]
    Muted,
    #[serde(rename = "dim")]
    Dim,
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "thinkingText")]
    ThinkingText,
    #[serde(rename = "selectedBg")]
    SelectedBg,
    #[serde(rename = "userMessageBg")]
    UserMessageBg,
    #[serde(rename = "userMessageText")]
    UserMessageText,
    #[serde(rename = "customMessageBg")]
    CustomMessageBg,
    #[serde(rename = "customMessageText")]
    CustomMessageText,
    #[serde(rename = "customMessageLabel")]
    CustomMessageLabel,
    #[serde(rename = "toolPendingBg")]
    ToolPendingBg,
    #[serde(rename = "toolSuccessBg")]
    ToolSuccessBg,
    #[serde(rename = "toolErrorBg")]
    ToolErrorBg,
    #[serde(rename = "toolTitle")]
    ToolTitle,
    #[serde(rename = "toolOutput")]
    ToolOutput,
    #[serde(rename = "mdHeading")]
    MdHeading,
    #[serde(rename = "mdLink")]
    MdLink,
    #[serde(rename = "mdLinkUrl")]
    MdLinkUrl,
    #[serde(rename = "mdCode")]
    MdCode,
    #[serde(rename = "mdCodeBlock")]
    MdCodeBlock,
    #[serde(rename = "mdCodeBlockBorder")]
    MdCodeBlockBorder,
    #[serde(rename = "mdQuote")]
    MdQuote,
    #[serde(rename = "mdQuoteBorder")]
    MdQuoteBorder,
    #[serde(rename = "mdHr")]
    MdHr,
    #[serde(rename = "mdListBullet")]
    MdListBullet,
    #[serde(rename = "toolDiffAdded")]
    ToolDiffAdded,
    #[serde(rename = "toolDiffRemoved")]
    ToolDiffRemoved,
    #[serde(rename = "toolDiffContext")]
    ToolDiffContext,
    #[serde(rename = "syntaxComment")]
    SyntaxComment,
    #[serde(rename = "syntaxKeyword")]
    SyntaxKeyword,
    #[serde(rename = "syntaxFunction")]
    SyntaxFunction,
    #[serde(rename = "syntaxVariable")]
    SyntaxVariable,
    #[serde(rename = "syntaxString")]
    SyntaxString,
    #[serde(rename = "syntaxNumber")]
    SyntaxNumber,
    #[serde(rename = "syntaxType")]
    SyntaxType,
    #[serde(rename = "syntaxOperator")]
    SyntaxOperator,
    #[serde(rename = "syntaxPunctuation")]
    SyntaxPunctuation,
    #[serde(rename = "thinkingOff")]
    ThinkingOff,
    #[serde(rename = "thinkingMinimal")]
    ThinkingMinimal,
    #[serde(rename = "thinkingLow")]
    ThinkingLow,
    #[serde(rename = "thinkingMedium")]
    ThinkingMedium,
    #[serde(rename = "thinkingHigh")]
    ThinkingHigh,
    #[serde(rename = "thinkingXhigh")]
    ThinkingXhigh,
    #[serde(rename = "bashMode")]
    BashMode,
}

impl RequiredTokenValue {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Accent => "accent",
            Self::Border => "border",
            Self::BorderAccent => "borderAccent",
            Self::BorderMuted => "borderMuted",
            Self::Success => "success",
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Muted => "muted",
            Self::Dim => "dim",
            Self::Text => "text",
            Self::ThinkingText => "thinkingText",
            Self::SelectedBg => "selectedBg",
            Self::UserMessageBg => "userMessageBg",
            Self::UserMessageText => "userMessageText",
            Self::CustomMessageBg => "customMessageBg",
            Self::CustomMessageText => "customMessageText",
            Self::CustomMessageLabel => "customMessageLabel",
            Self::ToolPendingBg => "toolPendingBg",
            Self::ToolSuccessBg => "toolSuccessBg",
            Self::ToolErrorBg => "toolErrorBg",
            Self::ToolTitle => "toolTitle",
            Self::ToolOutput => "toolOutput",
            Self::MdHeading => "mdHeading",
            Self::MdLink => "mdLink",
            Self::MdLinkUrl => "mdLinkUrl",
            Self::MdCode => "mdCode",
            Self::MdCodeBlock => "mdCodeBlock",
            Self::MdCodeBlockBorder => "mdCodeBlockBorder",
            Self::MdQuote => "mdQuote",
            Self::MdQuoteBorder => "mdQuoteBorder",
            Self::MdHr => "mdHr",
            Self::MdListBullet => "mdListBullet",
            Self::ToolDiffAdded => "toolDiffAdded",
            Self::ToolDiffRemoved => "toolDiffRemoved",
            Self::ToolDiffContext => "toolDiffContext",
            Self::SyntaxComment => "syntaxComment",
            Self::SyntaxKeyword => "syntaxKeyword",
            Self::SyntaxFunction => "syntaxFunction",
            Self::SyntaxVariable => "syntaxVariable",
            Self::SyntaxString => "syntaxString",
            Self::SyntaxNumber => "syntaxNumber",
            Self::SyntaxType => "syntaxType",
            Self::SyntaxOperator => "syntaxOperator",
            Self::SyntaxPunctuation => "syntaxPunctuation",
            Self::ThinkingOff => "thinkingOff",
            Self::ThinkingMinimal => "thinkingMinimal",
            Self::ThinkingLow => "thinkingLow",
            Self::ThinkingMedium => "thinkingMedium",
            Self::ThinkingHigh => "thinkingHigh",
            Self::ThinkingXhigh => "thinkingXhigh",
            Self::BashMode => "bashMode",
        }
    }
}

pub type RequiredToken = RequiredTokenValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeColorValuesValue {}

pub type ThemeColorValues = ThemeColorValuesValue;

pub type ResolvedThemeColorValues = IndexMap<ThemeToken, Value>;
