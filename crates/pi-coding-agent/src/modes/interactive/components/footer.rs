//! Port of `packages/coding-agent/src/modes/interactive/components/footer.ts`.

use crate::core::agent_session::AgentSession;
use crate::core::extensions::types::ContextUsage;
use crate::core::footer_data_provider::ReadonlyFooterDataProvider;
use crate::core::usage_totals::UsageTotals;
use pi_tui::tui::Component;
use serde_json::Value;
use std::sync::Arc;

/// Format token counts for compact footer display.
pub fn format_tokens(count: i64) -> String {
    todo!("port modes/interactive/components/footer.ts:formatTokens")
}

pub fn format_cwd_for_footer(cwd: &str, home: Option<&str>) -> String {
    todo!("port modes/interactive/components/footer.ts:formatCwdForFooter")
}

#[derive(Clone)]
pub struct SessionStats {
    pub session: Arc<AgentSession>,
    pub session_id: String,
    pub leaf_id: Option<String>,
    pub entry_count: f64,
    pub limits_model: Value,
    pub usage_totals: UsageTotals,
    pub latest_cache_hit_rate: Option<f64>,
    pub context_usage: Option<ContextUsage>,
}

/// Footer component that shows pwd, token stats, and context usage.
/// Computes token/context stats from session, gets git branch and extension statuses from provider.
/// TS: `implements Component`
pub struct FooterComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl FooterComponent {
    pub fn new(session: Arc<AgentSession>, footer_data: &ReadonlyFooterDataProvider) -> Self {
        todo!("port modes/interactive/components/footer.ts:FooterComponent.constructor")
    }

    pub fn set_session(&self, session: Arc<AgentSession>) {
        todo!("port modes/interactive/components/footer.ts:FooterComponent.setSession")
    }

    pub fn set_auto_compact_enabled(&self, enabled: bool) {
        todo!("port modes/interactive/components/footer.ts:FooterComponent.setAutoCompactEnabled")
    }

    /// Clean up resources.
    /// Git watcher cleanup now handled by provider.
    pub fn dispose(&self) {
        todo!("port modes/interactive/components/footer.ts:FooterComponent.dispose")
    }
}

impl Component for FooterComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/footer.ts:FooterComponent.render")
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/footer.ts:FooterComponent.invalidate")
    }
}
