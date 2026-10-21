//! Port of `packages/coding-agent/src/cli/list-models.ts`.

use crate::core::model_runtime::ModelRuntime;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// List available models, optionally filtered by search pattern
pub async fn list_models(
    model_runtime: Arc<ModelRuntime>,
    search_pattern: Option<&str>,
    signal: Option<CancellationToken>,
) -> anyhow::Result<()> {
    todo!("port cli/list-models.ts:listModels")
}
