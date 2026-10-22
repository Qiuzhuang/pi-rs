//! Port of `packages/coding-agent/src/modes/interactive/components/branch-summary-message.ts`.

use crate::compat::MarkdownTheme;
use crate::core::messages::BranchSummaryMessage;
use crate::modes::interactive::components::collapsible_box::{CollapsibleBox, collapsible_component};
use crate::modes::interactive::components::keybinding_hints::key_text;
use crate::modes::interactive::theme::theme::{ThemeColor, get_markdown_theme, theme};
use pi_tui::components::markdown::{DefaultTextStyle, Markdown};
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::tui::Container;
use std::sync::Arc;

/// Component that renders a branch summary message with collapsed/expanded state.
/// Uses same background color as custom messages for visual consistency.
/// TS: `extends Box`
pub struct BranchSummaryMessageComponent {
    inner: Arc<CollapsibleBox>,
}

impl BranchSummaryMessageComponent {
    pub fn new(
        message: &BranchSummaryMessage,
        markdown_theme: Option<&MarkdownTheme>,
        output_pad: Option<i64>,
    ) -> Self {
        let message = message.clone();
        let markdown_theme = markdown_theme.cloned().unwrap_or_else(get_markdown_theme);
        let build = move |content: &Container, expanded: bool| {
            let t = theme();
            let label = t.fg(&ThemeColor::CustomMessageLabel, "\x1b[1m[branch]\x1b[22m");
            content.add_child(Arc::new(Text::new(label, 0, 0, None)));
            content.add_child(Arc::new(Spacer::new(1)));

            if expanded {
                let header = "**Branch Summary**\n\n";
                content.add_child(Arc::new(Markdown::new(
                    format!("{header}{}", message.summary),
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
                content.add_child(Arc::new(Text::new(
                    t.fg(&ThemeColor::CustomMessageText, "Branch summary (")
                        + &t.fg(&ThemeColor::Dim, &key_text("app.tools.expand".to_string()))
                        + &t.fg(&ThemeColor::CustomMessageText, " to expand)"),
                    0,
                    0,
                    None,
                )));
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

collapsible_component!(BranchSummaryMessageComponent);
