//! Port of `packages/coding-agent/src/modes/interactive/components/auth-url.ts`.

use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use std::sync::Arc;

/// A sign-in URL with a click hint and a copy hint. Hosts call `copy()` when `app.message.copy` is
/// pressed, since a long URL wraps and often cannot be selected or clicked as a whole (SSH, tmux).
/// TS: `extends Container`
pub struct AuthUrlComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl AuthUrlComponent {
    pub fn new(tui: Arc<Tui>, url: &str) -> Self {
        todo!("port modes/interactive/components/auth-url.ts:AuthUrlComponent.constructor")
    }

    pub fn url(&self) -> String {
        todo!("port modes/interactive/components/auth-url.ts:AuthUrlComponent.url")
    }

    pub async fn copy(&self) -> anyhow::Result<()> {
        todo!("port modes/interactive/components/auth-url.ts:AuthUrlComponent.copy")
    }
}

// TODO(port): impl pi-tui::Container for AuthUrlComponent

impl Component for AuthUrlComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/auth-url.ts:AuthUrlComponent.render")
    }
}
