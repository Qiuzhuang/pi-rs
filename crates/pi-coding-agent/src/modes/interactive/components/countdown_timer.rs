//! Port of `packages/coding-agent/src/modes/interactive/components/countdown-timer.ts`.

use pi_tui::tui::Tui;
use std::sync::Arc;

pub struct CountdownTimer {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl CountdownTimer {
    pub fn new(
        timeout_ms: i64,
        tui: Option<Arc<Tui>>,
        on_tick: Arc<dyn Fn(i64) + Send + Sync>,
        on_expire: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        todo!("port modes/interactive/components/countdown-timer.ts:CountdownTimer.constructor")
    }

    pub fn dispose(&self) {
        todo!("port modes/interactive/components/countdown-timer.ts:CountdownTimer.dispose")
    }
}
