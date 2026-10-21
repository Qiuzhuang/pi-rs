//! Port of `packages/coding-agent/src/modes/interactive/components/skill-invocation-message.ts`.

use crate::compat::MarkdownTheme;
use crate::core::agent_session::ParsedSkillBlock;
use pi_tui::tui::Component;

/// Component that renders a skill invocation message with collapsed/expanded state.
/// Uses same background color as custom messages for visual consistency.
/// Only renders the skill block itself - user message is rendered separately.
/// TS: `extends Box`
pub struct SkillInvocationMessageComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl SkillInvocationMessageComponent {
    pub fn new(
        skill_block: &ParsedSkillBlock,
        markdown_theme: Option<&MarkdownTheme>,
        output_pad: Option<i64>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/skill-invocation-message.ts:SkillInvocationMessageComponent.constructor"
        )
    }

    pub fn set_expanded(&self, expanded: bool) {
        todo!(
            "port modes/interactive/components/skill-invocation-message.ts:SkillInvocationMessageComponent.setExpanded"
        )
    }

    pub fn set_output_pad(&self, output_pad: i64) {
        todo!(
            "port modes/interactive/components/skill-invocation-message.ts:SkillInvocationMessageComponent.setOutputPad"
        )
    }
}

// TODO(port): impl pi-tui::Box for SkillInvocationMessageComponent

impl Component for SkillInvocationMessageComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/skill-invocation-message.ts:SkillInvocationMessageComponent.render"
        )
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/skill-invocation-message.ts:SkillInvocationMessageComponent.invalidate"
        )
    }
}
