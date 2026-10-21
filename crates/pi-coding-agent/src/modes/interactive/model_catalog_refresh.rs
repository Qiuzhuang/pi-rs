//! Port of `packages/coding-agent/src/modes/interactive/model-catalog-refresh.ts`.

use crate::core::model_runtime::ModelRuntime;
use futures::future::BoxFuture;
use pi_ai::models::ModelsRefreshResult;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// TS: Pick<ModelRuntime, "refresh">
pub type ModelCatalogRuntime = Arc<ModelRuntime>;

#[derive(Clone)]
pub struct ActiveModelCatalogRefresh {
    pub controller: CancellationToken,
    pub promise: futures::future::Shared<
        BoxFuture<'static, Result<ModelsRefreshResult, Arc<anyhow::Error>>>,
    >,
    pub waiters: f64,
}

pub struct ModelCatalogRefreshCoordinator {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ModelCatalogRefreshCoordinator {
    pub fn new() -> Self {
        todo!(
            "port modes/interactive/model-catalog-refresh.ts:ModelCatalogRefreshCoordinator.constructor"
        )
    }

    pub async fn refresh(
        &self,
        model_runtime: &ModelCatalogRuntime,
        signal: CancellationToken,
    ) -> anyhow::Result<ModelsRefreshResult> {
        todo!(
            "port modes/interactive/model-catalog-refresh.ts:ModelCatalogRefreshCoordinator.refresh"
        )
    }
}

/// Share concurrent interactive all-catalog refreshes while keeping each caller's cancellation independent.
pub async fn refresh_model_catalogs(
    model_runtime: &ModelCatalogRuntime,
    signal: CancellationToken,
) -> anyhow::Result<ModelsRefreshResult> {
    todo!("port modes/interactive/model-catalog-refresh.ts:refreshModelCatalogs")
}
