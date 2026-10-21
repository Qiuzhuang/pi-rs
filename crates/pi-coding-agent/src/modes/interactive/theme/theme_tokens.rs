//! Port of `packages/coding-agent/src/modes/interactive/theme/theme-tokens.ts`.
//!
//! Theme token metadata in the same order used by theme files and the published JSON Schema.
//! [`ThemeToken`] is every token, [`ThemeColor`] the foreground tokens and [`ThemeBg`] the
//! background tokens (the TS mapped types).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Color slot of a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeTokenSlot {
    Foreground,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeTokenDescriptor {
    pub slot: ThemeTokenSlot,
    pub description: &'static str,
    /// Token used when this optional token is omitted (same slot).
    pub fallback: Option<ThemeToken>,
}

/// `Readonly<Record<string, ThemeTokenDescriptor>>`, in declaration order.
pub type ThemeTokenDescriptors = &'static [(ThemeToken, ThemeTokenDescriptor)];

/// Every theme token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ThemeToken {
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
    #[serde(rename = "scrollbarTrack")]
    ScrollbarTrack,
    #[serde(rename = "scrollbarThumb")]
    ScrollbarThumb,
    #[serde(rename = "selectedBg")]
    SelectedBg,
    #[serde(rename = "searchMatchBg")]
    SearchMatchBg,
    #[serde(rename = "searchMatchText")]
    SearchMatchText,
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
    #[serde(rename = "thinkingMax")]
    ThinkingMax,
    #[serde(rename = "bashMode")]
    BashMode,
}

/// Token metadata in declaration order (`THEME_TOKENS`).
pub static THEME_TOKENS: &[(ThemeToken, ThemeTokenDescriptor)] = &[
    (
        ThemeToken::Accent,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Primary accent color (logo, selected items, cursor)",
            fallback: None,
        },
    ),
    (
        ThemeToken::Border,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Normal borders",
            fallback: None,
        },
    ),
    (
        ThemeToken::BorderAccent,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Highlighted borders",
            fallback: None,
        },
    ),
    (
        ThemeToken::BorderMuted,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Subtle borders",
            fallback: None,
        },
    ),
    (
        ThemeToken::Success,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Success states",
            fallback: None,
        },
    ),
    (
        ThemeToken::Error,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Error states",
            fallback: None,
        },
    ),
    (
        ThemeToken::Warning,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Warning states",
            fallback: None,
        },
    ),
    (
        ThemeToken::Muted,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Secondary/dimmed text",
            fallback: None,
        },
    ),
    (
        ThemeToken::Dim,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Very dimmed text (more subtle than muted)",
            fallback: None,
        },
    ),
    (
        ThemeToken::Text,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Default text color (usually empty string)",
            fallback: None,
        },
    ),
    (
        ThemeToken::ThinkingText,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Thinking block text color",
            fallback: None,
        },
    ),
    (
        ThemeToken::ScrollbarTrack,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Fullscreen scrollbar track foreground (falls back to muted when omitted)",
            fallback: Some(ThemeToken::Muted),
        },
    ),
    (
        ThemeToken::ScrollbarThumb,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Fullscreen scrollbar thumb foreground (falls back to text when omitted)",
            fallback: Some(ThemeToken::Text),
        },
    ),
    (
        ThemeToken::SelectedBg,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Background,
            description: "Selected item background",
            fallback: None,
        },
    ),
    (
        ThemeToken::SearchMatchBg,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Background,
            description: "Transcript search match background and current-match text (falls back to selectedBg when omitted)",
            fallback: Some(ThemeToken::SelectedBg),
        },
    ),
    (
        ThemeToken::SearchMatchText,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Transcript search match text and current-match background (falls back to text when omitted)",
            fallback: Some(ThemeToken::Text),
        },
    ),
    (
        ThemeToken::UserMessageBg,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Background,
            description: "User message background",
            fallback: None,
        },
    ),
    (
        ThemeToken::UserMessageText,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "User message text color",
            fallback: None,
        },
    ),
    (
        ThemeToken::CustomMessageBg,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Background,
            description: "Custom message background (hook-injected messages)",
            fallback: None,
        },
    ),
    (
        ThemeToken::CustomMessageText,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Custom message text color",
            fallback: None,
        },
    ),
    (
        ThemeToken::CustomMessageLabel,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Custom message type label color",
            fallback: None,
        },
    ),
    (
        ThemeToken::ToolPendingBg,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Background,
            description: "Tool execution box (pending state)",
            fallback: None,
        },
    ),
    (
        ThemeToken::ToolSuccessBg,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Background,
            description: "Tool execution box (success state)",
            fallback: None,
        },
    ),
    (
        ThemeToken::ToolErrorBg,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Background,
            description: "Tool execution box (error state)",
            fallback: None,
        },
    ),
    (
        ThemeToken::ToolTitle,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Tool execution box title color",
            fallback: None,
        },
    ),
    (
        ThemeToken::ToolOutput,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Tool execution box output text color",
            fallback: None,
        },
    ),
    (
        ThemeToken::MdHeading,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Markdown heading text",
            fallback: None,
        },
    ),
    (
        ThemeToken::MdLink,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Markdown link text",
            fallback: None,
        },
    ),
    (
        ThemeToken::MdLinkUrl,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Markdown link URL",
            fallback: None,
        },
    ),
    (
        ThemeToken::MdCode,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Markdown inline code",
            fallback: None,
        },
    ),
    (
        ThemeToken::MdCodeBlock,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Markdown code block content",
            fallback: None,
        },
    ),
    (
        ThemeToken::MdCodeBlockBorder,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Markdown code block fences",
            fallback: None,
        },
    ),
    (
        ThemeToken::MdQuote,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Markdown blockquote text",
            fallback: None,
        },
    ),
    (
        ThemeToken::MdQuoteBorder,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Markdown blockquote border",
            fallback: None,
        },
    ),
    (
        ThemeToken::MdHr,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Markdown horizontal rule",
            fallback: None,
        },
    ),
    (
        ThemeToken::MdListBullet,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Markdown list bullets/numbers",
            fallback: None,
        },
    ),
    (
        ThemeToken::ToolDiffAdded,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Added lines in tool diffs",
            fallback: None,
        },
    ),
    (
        ThemeToken::ToolDiffRemoved,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Removed lines in tool diffs",
            fallback: None,
        },
    ),
    (
        ThemeToken::ToolDiffContext,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Context lines in tool diffs",
            fallback: None,
        },
    ),
    (
        ThemeToken::SyntaxComment,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Syntax highlighting: comments",
            fallback: None,
        },
    ),
    (
        ThemeToken::SyntaxKeyword,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Syntax highlighting: keywords",
            fallback: None,
        },
    ),
    (
        ThemeToken::SyntaxFunction,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Syntax highlighting: function names",
            fallback: None,
        },
    ),
    (
        ThemeToken::SyntaxVariable,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Syntax highlighting: variable names",
            fallback: None,
        },
    ),
    (
        ThemeToken::SyntaxString,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Syntax highlighting: string literals",
            fallback: None,
        },
    ),
    (
        ThemeToken::SyntaxNumber,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Syntax highlighting: number literals",
            fallback: None,
        },
    ),
    (
        ThemeToken::SyntaxType,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Syntax highlighting: type names",
            fallback: None,
        },
    ),
    (
        ThemeToken::SyntaxOperator,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Syntax highlighting: operators",
            fallback: None,
        },
    ),
    (
        ThemeToken::SyntaxPunctuation,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Syntax highlighting: punctuation",
            fallback: None,
        },
    ),
    (
        ThemeToken::ThinkingOff,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Thinking level border: off",
            fallback: None,
        },
    ),
    (
        ThemeToken::ThinkingMinimal,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Thinking level border: minimal",
            fallback: None,
        },
    ),
    (
        ThemeToken::ThinkingLow,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Thinking level border: low",
            fallback: None,
        },
    ),
    (
        ThemeToken::ThinkingMedium,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Thinking level border: medium",
            fallback: None,
        },
    ),
    (
        ThemeToken::ThinkingHigh,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Thinking level border: high",
            fallback: None,
        },
    ),
    (
        ThemeToken::ThinkingXhigh,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Thinking level border: xhigh",
            fallback: None,
        },
    ),
    (
        ThemeToken::ThinkingMax,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Thinking level border: max (falls back to thinkingXhigh when omitted)",
            fallback: Some(ThemeToken::ThinkingXhigh),
        },
    ),
    (
        ThemeToken::BashMode,
        ThemeTokenDescriptor {
            slot: ThemeTokenSlot::Foreground,
            description: "Editor border color in bash mode",
            fallback: None,
        },
    ),
];

impl ThemeToken {
    /// All tokens in declaration order.
    pub fn all() -> impl Iterator<Item = ThemeToken> {
        THEME_TOKENS.iter().map(|(t, _)| *t)
    }

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
            Self::ScrollbarTrack => "scrollbarTrack",
            Self::ScrollbarThumb => "scrollbarThumb",
            Self::SelectedBg => "selectedBg",
            Self::SearchMatchBg => "searchMatchBg",
            Self::SearchMatchText => "searchMatchText",
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
            Self::ThinkingMax => "thinkingMax",
            Self::BashMode => "bashMode",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "accent" => Some(Self::Accent),
            "border" => Some(Self::Border),
            "borderAccent" => Some(Self::BorderAccent),
            "borderMuted" => Some(Self::BorderMuted),
            "success" => Some(Self::Success),
            "error" => Some(Self::Error),
            "warning" => Some(Self::Warning),
            "muted" => Some(Self::Muted),
            "dim" => Some(Self::Dim),
            "text" => Some(Self::Text),
            "thinkingText" => Some(Self::ThinkingText),
            "scrollbarTrack" => Some(Self::ScrollbarTrack),
            "scrollbarThumb" => Some(Self::ScrollbarThumb),
            "selectedBg" => Some(Self::SelectedBg),
            "searchMatchBg" => Some(Self::SearchMatchBg),
            "searchMatchText" => Some(Self::SearchMatchText),
            "userMessageBg" => Some(Self::UserMessageBg),
            "userMessageText" => Some(Self::UserMessageText),
            "customMessageBg" => Some(Self::CustomMessageBg),
            "customMessageText" => Some(Self::CustomMessageText),
            "customMessageLabel" => Some(Self::CustomMessageLabel),
            "toolPendingBg" => Some(Self::ToolPendingBg),
            "toolSuccessBg" => Some(Self::ToolSuccessBg),
            "toolErrorBg" => Some(Self::ToolErrorBg),
            "toolTitle" => Some(Self::ToolTitle),
            "toolOutput" => Some(Self::ToolOutput),
            "mdHeading" => Some(Self::MdHeading),
            "mdLink" => Some(Self::MdLink),
            "mdLinkUrl" => Some(Self::MdLinkUrl),
            "mdCode" => Some(Self::MdCode),
            "mdCodeBlock" => Some(Self::MdCodeBlock),
            "mdCodeBlockBorder" => Some(Self::MdCodeBlockBorder),
            "mdQuote" => Some(Self::MdQuote),
            "mdQuoteBorder" => Some(Self::MdQuoteBorder),
            "mdHr" => Some(Self::MdHr),
            "mdListBullet" => Some(Self::MdListBullet),
            "toolDiffAdded" => Some(Self::ToolDiffAdded),
            "toolDiffRemoved" => Some(Self::ToolDiffRemoved),
            "toolDiffContext" => Some(Self::ToolDiffContext),
            "syntaxComment" => Some(Self::SyntaxComment),
            "syntaxKeyword" => Some(Self::SyntaxKeyword),
            "syntaxFunction" => Some(Self::SyntaxFunction),
            "syntaxVariable" => Some(Self::SyntaxVariable),
            "syntaxString" => Some(Self::SyntaxString),
            "syntaxNumber" => Some(Self::SyntaxNumber),
            "syntaxType" => Some(Self::SyntaxType),
            "syntaxOperator" => Some(Self::SyntaxOperator),
            "syntaxPunctuation" => Some(Self::SyntaxPunctuation),
            "thinkingOff" => Some(Self::ThinkingOff),
            "thinkingMinimal" => Some(Self::ThinkingMinimal),
            "thinkingLow" => Some(Self::ThinkingLow),
            "thinkingMedium" => Some(Self::ThinkingMedium),
            "thinkingHigh" => Some(Self::ThinkingHigh),
            "thinkingXhigh" => Some(Self::ThinkingXhigh),
            "thinkingMax" => Some(Self::ThinkingMax),
            "bashMode" => Some(Self::BashMode),
            _ => None,
        }
    }

    pub fn descriptor(&self) -> &'static ThemeTokenDescriptor {
        &THEME_TOKENS
            .iter()
            .find(|(t, _)| t == self)
            .expect("every token has a descriptor")
            .1
    }

    pub fn slot(&self) -> ThemeTokenSlot {
        self.descriptor().slot
    }

    /// Whether the token may be omitted from theme files.
    pub fn is_optional(&self) -> bool {
        self.descriptor().fallback.is_some()
    }
}

/// Foreground tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ThemeColor {
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
    #[serde(rename = "scrollbarTrack")]
    ScrollbarTrack,
    #[serde(rename = "scrollbarThumb")]
    ScrollbarThumb,
    #[serde(rename = "searchMatchText")]
    SearchMatchText,
    #[serde(rename = "userMessageText")]
    UserMessageText,
    #[serde(rename = "customMessageText")]
    CustomMessageText,
    #[serde(rename = "customMessageLabel")]
    CustomMessageLabel,
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
    #[serde(rename = "thinkingMax")]
    ThinkingMax,
    #[serde(rename = "bashMode")]
    BashMode,
}

impl ThemeColor {
    pub fn token(&self) -> ThemeToken {
        match self {
            Self::Accent => ThemeToken::Accent,
            Self::Border => ThemeToken::Border,
            Self::BorderAccent => ThemeToken::BorderAccent,
            Self::BorderMuted => ThemeToken::BorderMuted,
            Self::Success => ThemeToken::Success,
            Self::Error => ThemeToken::Error,
            Self::Warning => ThemeToken::Warning,
            Self::Muted => ThemeToken::Muted,
            Self::Dim => ThemeToken::Dim,
            Self::Text => ThemeToken::Text,
            Self::ThinkingText => ThemeToken::ThinkingText,
            Self::ScrollbarTrack => ThemeToken::ScrollbarTrack,
            Self::ScrollbarThumb => ThemeToken::ScrollbarThumb,
            Self::SearchMatchText => ThemeToken::SearchMatchText,
            Self::UserMessageText => ThemeToken::UserMessageText,
            Self::CustomMessageText => ThemeToken::CustomMessageText,
            Self::CustomMessageLabel => ThemeToken::CustomMessageLabel,
            Self::ToolTitle => ThemeToken::ToolTitle,
            Self::ToolOutput => ThemeToken::ToolOutput,
            Self::MdHeading => ThemeToken::MdHeading,
            Self::MdLink => ThemeToken::MdLink,
            Self::MdLinkUrl => ThemeToken::MdLinkUrl,
            Self::MdCode => ThemeToken::MdCode,
            Self::MdCodeBlock => ThemeToken::MdCodeBlock,
            Self::MdCodeBlockBorder => ThemeToken::MdCodeBlockBorder,
            Self::MdQuote => ThemeToken::MdQuote,
            Self::MdQuoteBorder => ThemeToken::MdQuoteBorder,
            Self::MdHr => ThemeToken::MdHr,
            Self::MdListBullet => ThemeToken::MdListBullet,
            Self::ToolDiffAdded => ThemeToken::ToolDiffAdded,
            Self::ToolDiffRemoved => ThemeToken::ToolDiffRemoved,
            Self::ToolDiffContext => ThemeToken::ToolDiffContext,
            Self::SyntaxComment => ThemeToken::SyntaxComment,
            Self::SyntaxKeyword => ThemeToken::SyntaxKeyword,
            Self::SyntaxFunction => ThemeToken::SyntaxFunction,
            Self::SyntaxVariable => ThemeToken::SyntaxVariable,
            Self::SyntaxString => ThemeToken::SyntaxString,
            Self::SyntaxNumber => ThemeToken::SyntaxNumber,
            Self::SyntaxType => ThemeToken::SyntaxType,
            Self::SyntaxOperator => ThemeToken::SyntaxOperator,
            Self::SyntaxPunctuation => ThemeToken::SyntaxPunctuation,
            Self::ThinkingOff => ThemeToken::ThinkingOff,
            Self::ThinkingMinimal => ThemeToken::ThinkingMinimal,
            Self::ThinkingLow => ThemeToken::ThinkingLow,
            Self::ThinkingMedium => ThemeToken::ThinkingMedium,
            Self::ThinkingHigh => ThemeToken::ThinkingHigh,
            Self::ThinkingXhigh => ThemeToken::ThinkingXhigh,
            Self::ThinkingMax => ThemeToken::ThinkingMax,
            Self::BashMode => ThemeToken::BashMode,
        }
    }

    pub fn as_str(&self) -> &'static str {
        self.token().as_str()
    }
}

impl From<ThemeColor> for ThemeToken {
    fn from(value: ThemeColor) -> Self {
        value.token()
    }
}

/// Background tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ThemeBg {
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

impl ThemeBg {
    pub fn token(&self) -> ThemeToken {
        match self {
            Self::SelectedBg => ThemeToken::SelectedBg,
            Self::SearchMatchBg => ThemeToken::SearchMatchBg,
            Self::UserMessageBg => ThemeToken::UserMessageBg,
            Self::CustomMessageBg => ThemeToken::CustomMessageBg,
            Self::ToolPendingBg => ThemeToken::ToolPendingBg,
            Self::ToolSuccessBg => ThemeToken::ToolSuccessBg,
            Self::ToolErrorBg => ThemeToken::ToolErrorBg,
        }
    }

    pub fn as_str(&self) -> &'static str {
        self.token().as_str()
    }
}

impl From<ThemeBg> for ThemeToken {
    fn from(value: ThemeBg) -> Self {
        value.token()
    }
}

/// `ThemeColorValues<Value>`: required tokens plus optional ones, as the JSON `colors` object.
pub type ThemeColorValues<V> = IndexMap<String, V>;

/// `ResolvedThemeColorValues<Value>`: a value for every token.
pub type ResolvedThemeColorValues<V> = IndexMap<ThemeToken, V>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallbacks_reference_tokens_of_the_same_slot() {
        for (token, descriptor) in THEME_TOKENS {
            if let Some(fallback) = descriptor.fallback {
                assert_eq!(fallback.slot(), descriptor.slot, "{token:?}");
            }
            assert_eq!(ThemeToken::parse(token.as_str()), Some(*token));
        }
    }
}
