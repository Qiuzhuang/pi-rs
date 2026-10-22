//! Port of `packages/coding-agent/src/modes/interactive/components/skill-invocation-message.ts`.

use crate::compat::MarkdownTheme;
use crate::core::agent_session::ParsedSkillBlock;
use crate::modes::interactive::components::collapsible_box::{CollapsibleBox, collapsible_component};
use crate::modes::interactive::components::keybinding_hints::key_text;
use crate::modes::interactive::theme::theme::{ThemeColor, get_markdown_theme, theme};
use pi_tui::components::markdown::{DefaultTextStyle, Markdown};
use pi_tui::components::text::Text;
use pi_tui::tui::Container;
use std::sync::Arc;

/// Component that renders a skill invocation message with collapsed/expanded state.
/// Uses same background color as custom messages for visual consistency.
/// Only renders the skill block itself - user message is rendered separately.
/// TS: `extends Box`
pub struct SkillInvocationMessageComponent {
    inner: Arc<CollapsibleBox>,
}

impl SkillInvocationMessageComponent {
    pub fn new(
        skill_block: &ParsedSkillBlock,
        markdown_theme: Option<&MarkdownTheme>,
        output_pad: Option<i64>,
    ) -> Self {
        let skill_block = skill_block.clone();
        let markdown_theme = markdown_theme.cloned().unwrap_or_else(get_markdown_theme);
        let build = move |content: &Container, expanded: bool| {
            let t = theme();
            if expanded {
                // Expanded: label + skill name header + full content
                let label = t.fg(&ThemeColor::CustomMessageLabel, "\x1b[1m[skill]\x1b[22m");
                content.add_child(Arc::new(Text::new(label, 0, 0, None)));
                let header = format!("**{}**\n\n", skill_block.name);
                content.add_child(Arc::new(Markdown::new(
                    format!("{header}{}", skill_block.content),
                    0,
                    0,
                    markdown_theme.clone(),
                    Some(DefaultTextStyle {
                        color: Some(Arc::new(|text: &str| theme().fg(&ThemeColor::CustomMessageText, text))),
                        ..Default::default()
                    }),
                    None,
                )));
            } else {
                // Collapsed: single line - [skill] name (hint to expand)
                let line = t.fg(&ThemeColor::CustomMessageLabel, "\x1b[1m[skill]\x1b[22m ")
                    + &t.fg(&ThemeColor::CustomMessageText, &skill_block.name)
                    + &t.fg(&ThemeColor::Dim, &format!(" ({} to expand)", key_text("app.tools.expand".to_string())));
                content.add_child(Arc::new(Text::new(line, 0, 0, None)));
            }
        };
        Self { inner: CollapsibleBox::new(output_pad, Box::new(build)) }
    }

    pub fn set_expanded(&self, expanded: bool) {
        self.inner.set_expanded(expanded);
    }

    pub fn set_output_pad(&self, output_pad: i64) {
        self.inner.set_output_pad(output_pad);
    }
}

collapsible_component!(SkillInvocationMessageComponent);
