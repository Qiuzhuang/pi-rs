//! Port of `packages/coding-agent/src/cli/session-picker.ts`.

use crate::core::session_manager::SessionInfo;
use crate::core::session_manager::SessionListProgress;
use crate::core::settings_manager::SettingsManager;
use futures::future::BoxFuture;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub type SessionsLoader = Arc<
    dyn Fn(
            Option<SessionListProgress>,
            Option<CancellationToken>,
        ) -> BoxFuture<'static, anyhow::Result<Vec<SessionInfo>>>
        + Send
        + Sync,
>;

/// Show TUI session selector and return selected session path or null if cancelled
pub async fn select_session(
    current_sessions_loader: &SessionsLoader,
    all_sessions_loader: &SessionsLoader,
    settings_manager: Arc<SettingsManager>,
) -> anyhow::Result<Option<String>> {
    todo!("port cli/session-picker.ts:selectSession")
}
