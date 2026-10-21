//! Port of `packages/coding-agent/src/modes/interactive/components/earendil-announcement.ts`.

use pi_tui::tui::Component;

/// TS: `extends Container`
pub struct EarendilAnnouncementComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl EarendilAnnouncementComponent {
    pub fn new() -> Self {
        todo!(
            "port modes/interactive/components/earendil-announcement.ts:EarendilAnnouncementComponent.constructor"
        )
    }
}

// TODO(port): impl pi-tui::Container for EarendilAnnouncementComponent

impl Component for EarendilAnnouncementComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/earendil-announcement.ts:EarendilAnnouncementComponent.render"
        )
    }
}
