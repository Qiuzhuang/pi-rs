//! Port of `packages/coding-agent/src/core/models-store.ts`.

use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_ai::models_store::ModelsStoreEntry;
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub type StoredModels = IndexMap<String, ModelsStoreEntry>;

#[derive(Clone)]
pub struct ModelsFileReload {
    pub controller: CancellationToken,
    pub promise:
        futures::future::Shared<BoxFuture<'static, Result<StoredModels, Arc<anyhow::Error>>>>,
    pub readers: f64,
}

#[derive(Clone)]
pub struct ModelsFileReadState {
    pub data: StoredModels,
    pub revision: Option<String>,
    pub reload: Option<ModelsFileReload>,
}

/// TS: `implements ModelsStore`
pub struct InMemoryCodingAgentModelsStore {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl InMemoryCodingAgentModelsStore {
    pub fn new() -> Self {
        todo!("port core/models-store.ts:InMemoryCodingAgentModelsStore.constructor")
    }

    pub async fn read(
        &self,
        provider_id: &str,
        options: Option<Value /* TS: pi-ai::ModelsStoreOperationOptions */>,
    ) -> anyhow::Result<Option<ModelsStoreEntry>> {
        todo!("port core/models-store.ts:InMemoryCodingAgentModelsStore.read")
    }

    pub async fn write(
        &self,
        provider_id: &str,
        entry: ModelsStoreEntry,
        options: Option<Value /* TS: pi-ai::ModelsStoreOperationOptions */>,
    ) -> anyhow::Result<()> {
        todo!("port core/models-store.ts:InMemoryCodingAgentModelsStore.write")
    }

    pub async fn delete(
        &self,
        provider_id: &str,
        options: Option<Value /* TS: pi-ai::ModelsStoreOperationOptions */>,
    ) -> anyhow::Result<()> {
        todo!("port core/models-store.ts:InMemoryCodingAgentModelsStore.delete")
    }
}

// TODO(port): impl pi-ai::ModelsStore for InMemoryCodingAgentModelsStore

/// Locked JSON-backed storage for dynamically refreshed provider catalogs.
/// TS: `implements ModelsStore`
pub struct FileModelsStore {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl FileModelsStore {
    pub fn new(path: Option<&str>) -> Self {
        todo!("port core/models-store.ts:FileModelsStore.constructor")
    }

    pub async fn read(
        &self,
        provider_id: &str,
        options: Option<Value /* TS: pi-ai::ModelsStoreOperationOptions */>,
    ) -> anyhow::Result<Option<ModelsStoreEntry>> {
        todo!("port core/models-store.ts:FileModelsStore.read")
    }

    pub async fn write(
        &self,
        provider_id: &str,
        entry: ModelsStoreEntry,
        options: Option<Value /* TS: pi-ai::ModelsStoreOperationOptions */>,
    ) -> anyhow::Result<()> {
        todo!("port core/models-store.ts:FileModelsStore.write")
    }

    pub async fn delete(
        &self,
        provider_id: &str,
        options: Option<Value /* TS: pi-ai::ModelsStoreOperationOptions */>,
    ) -> anyhow::Result<()> {
        todo!("port core/models-store.ts:FileModelsStore.delete")
    }
}

// TODO(port): impl pi-ai::ModelsStore for FileModelsStore
