//! Port of `packages/coding-agent/src/modes/interactive/components/bordered-loader.ts`.

use crate::modes::interactive::theme::theme::Theme;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BorderedLoaderOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellable: Option<bool>,
}

/// Loader wrapped with borders for extension UI
/// TS: `extends Container`
pub struct BorderedLoader {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl BorderedLoader {
    pub fn new(
        tui: Arc<Tui>,
        theme: Arc<Theme>,
        message: &str,
        options: Option<&BorderedLoaderOptions>,
    ) -> Self {
        todo!("port modes/interactive/components/bordered-loader.ts:BorderedLoader.constructor")
    }

    pub fn signal(&self) -> CancellationToken {
        todo!("port modes/interactive/components/bordered-loader.ts:BorderedLoader.signal")
    }

    pub fn set_on_abort(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/bordered-loader.ts:BorderedLoader.onAbort")
    }

    pub fn dispose(&self) {
        todo!("port modes/interactive/components/bordered-loader.ts:BorderedLoader.dispose")
    }
}

// TODO(port): impl pi-tui::Container for BorderedLoader

impl Component for BorderedLoader {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/bordered-loader.ts:BorderedLoader.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port modes/interactive/components/bordered-loader.ts:BorderedLoader.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }
}
