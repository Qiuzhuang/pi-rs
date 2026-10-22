//! Port of `packages/coding-agent/src/modes/interactive/components/mermaid.ts`.
//!
//! The diagram renderer is a port of the `grok-mermaid` npm package (`mermaid/grok_mermaid`).

mod grok_mermaid;

use crate::core::extensions::types::{MarkdownMessageType, MarkdownTransformContext, MarkdownTransformer};
use crate::core::settings_schema::MermaidRenderingMode;
use crate::modes::interactive::theme::theme::{Theme, ThemeColor};
use grok_mermaid::{Cls, MermaidArt, Span, render};
use pi_tui::marked::{MarkedOptions, Token, TokenType, lexer};
use std::sync::Arc;

#[derive(Clone)]
pub struct MermaidTransformerOptions {
    pub get_mode: Arc<dyn Fn() -> MermaidRenderingMode + Send + Sync>,
    pub theme: Option<Arc<Theme>>,
}

fn is_mermaid(token: &Token) -> bool {
    token.kind == TokenType::Code
        && token
            .lang
            .as_deref()
            .and_then(|lang| pi_tui::utils::js_trim(lang).split(pi_tui::utils::is_whitespace_ch).next())
            .is_some_and(|first| first.to_lowercase() == "mermaid")
}

fn code_span(line: &str) -> String {
    // Encode each diagram row as inline code (` ... `) so Markdown preserves its spacing and
    // box-drawing characters. Use a non-breaking space for blank rows because an
    // empty code span has no visible height.
    let content = if line.is_empty() { "\u{a0}" } else { line };
    // CommonMark code spans use matching backtick delimiters, so choose one
    // longer than any backtick run in the content. If the content starts or ends
    // with a backtick, a space keeps that backtick as content.
    let mut longest_backtick_run = 0;
    let mut run = 0;
    for c in content.chars() {
        if c == '`' {
            run += 1;
            longest_backtick_run = longest_backtick_run.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat(longest_backtick_run + 1);
    let padding = if content.starts_with('`') || content.ends_with('`') { " " } else { "" };
    format!("{fence}{padding}{content}{padding}{fence}")
}

fn style_span(span: &Span, theme: &Theme) -> String {
    match span.cls {
        Cls::Border => theme.fg(&ThemeColor::BorderMuted, &span.text),
        Cls::Text => theme.fg(&ThemeColor::Text, &span.text),
        Cls::Edge => theme.fg(&ThemeColor::Accent, &span.text),
        Cls::EdgeLabel => theme.fg(&ThemeColor::Muted, &span.text),
        Cls::Title => theme.fg(&ThemeColor::Accent, &theme.bold(&span.text)),
        Cls::None => span.text.clone(),
    }
}

fn themed_lines(art: &MermaidArt, theme: &Theme) -> Vec<String> {
    art.styled.iter().map(|row| row.iter().map(|span| style_span(span, theme)).collect()).collect()
}

/// Create a transformer that replaces top-level Mermaid code blocks with Unicode terminal diagrams.
pub fn create_mermaid_markdown_transformer(
    options: &MermaidTransformerOptions,
) -> MarkdownTransformer {
    let options = options.clone();
    Arc::new(move |markdown: &str, context: &MarkdownTransformContext| {
        let mode = (options.get_mode)();
        if mode == MermaidRenderingMode::Off
            || context.message_type == MarkdownMessageType::AssistantThinking
            || (context.is_streaming && mode != MermaidRenderingMode::Streaming)
        {
            return markdown.to_string();
        }

        lexer(markdown, MarkedOptions::default())
            .into_iter()
            .map(|token| {
                if !is_mermaid(&token) {
                    return token.raw;
                }
                let Some(art) = render(&token.text) else { return token.raw };
                if art.width > context.available_width {
                    return token.raw;
                }
                if !context.is_streaming && !art.warnings.is_empty() {
                    let suffix = if art.warnings.len() > 1 {
                        format!(" (+{} more)", art.warnings.len() - 1)
                    } else {
                        String::new()
                    };
                    let warning = format!("Mermaid diagram not rendered: {}{suffix}", art.warnings[0]);
                    let styled_warning = match &options.theme {
                        Some(theme) => theme.fg(&ThemeColor::Warning, &warning),
                        None => warning,
                    };
                    return format!("{}\n{}  \n", token.raw, code_span(&styled_warning));
                }
                let lines = match &options.theme {
                    Some(theme) => themed_lines(&art, theme),
                    None => art.plain.clone(),
                };
                // Markdown hard breaks keep every diagram row on its own line.
                format!("{}\n", lines.iter().map(|l| code_span(l)).collect::<Vec<_>>().join("  \n"))
            })
            .collect::<String>()
    })
}
