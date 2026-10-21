//! Port of `packages/coding-agent/src/core/auth-storage.ts`.

use crate::compat::AuthOperationOptions;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_ai::auth::types::Credential;
use pi_ai::auth::types::CredentialInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub type AuthStorageData = IndexMap<String, Credential>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockResult {
    pub result: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Clone)]
pub struct AuthFileReload {
    pub controller: CancellationToken,
    pub promise:
        futures::future::Shared<BoxFuture<'static, Result<AuthStorageData, Arc<anyhow::Error>>>>,
    pub readers: f64,
}

#[derive(Clone)]
pub struct AuthFileReadState {
    pub data: AuthStorageData,
    pub revision: Option<String>,
    pub reload: Option<AuthFileReload>,
}

#[async_trait::async_trait]
pub trait AuthStorageBackend: Send + Sync {
    fn with_lock(&self, r#fn: Arc<dyn Fn(Option<String>) -> LockResult + Send + Sync>) -> Value;
    async fn with_lock_async(
        &self,
        r#fn: Arc<
            dyn Fn(Option<String>) -> BoxFuture<'static, anyhow::Result<LockResult>> + Send + Sync,
        >,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Value>;
}

/// TS: `implements AuthStorageBackend`
pub struct FileAuthStorageBackend {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl FileAuthStorageBackend {
    pub fn new(auth_path: Option<&str>) -> Self {
        todo!("port core/auth-storage.ts:FileAuthStorageBackend.constructor")
    }
}

#[async_trait::async_trait]
impl AuthStorageBackend for FileAuthStorageBackend {
    fn with_lock(&self, r#fn: Arc<dyn Fn(Option<String>) -> LockResult + Send + Sync>) -> Value {
        todo!("port core/auth-storage.ts:FileAuthStorageBackend.with_lock")
    }
    async fn with_lock_async(
        &self,
        r#fn: Arc<
            dyn Fn(Option<String>) -> BoxFuture<'static, anyhow::Result<LockResult>> + Send + Sync,
        >,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Value> {
        todo!("port core/auth-storage.ts:FileAuthStorageBackend.with_lock_async")
    }
}

/// TS: `implements CredentialStore`
pub struct ReadOnlyAuthStorage {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ReadOnlyAuthStorage {
    pub fn new(auth_path: Option<&str>) -> Self {
        todo!("port core/auth-storage.ts:ReadOnlyAuthStorage.constructor")
    }

    pub async fn read(
        &self,
        provider_id: &str,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Option<Credential>> {
        todo!("port core/auth-storage.ts:ReadOnlyAuthStorage.read")
    }

    pub async fn list(
        &self,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Vec<CredentialInfo>> {
        todo!("port core/auth-storage.ts:ReadOnlyAuthStorage.list")
    }

    pub async fn modify(
        &self,
        provider_id: &str,
        r#fn: Arc<
            dyn Fn(Option<Credential>) -> BoxFuture<'static, anyhow::Result<Option<Credential>>>
                + Send
                + Sync,
        >,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Option<Credential>> {
        todo!("port core/auth-storage.ts:ReadOnlyAuthStorage.modify")
    }

    pub async fn delete(
        &self,
        provider_id: &str,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/auth-storage.ts:ReadOnlyAuthStorage.delete")
    }
}

// TODO(port): impl pi-ai::CredentialStore for ReadOnlyAuthStorage

/// TS: `implements AuthStorageBackend`
pub struct InMemoryAuthStorageBackend {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl InMemoryAuthStorageBackend {
    pub fn new() -> Self {
        todo!("port core/auth-storage.ts:InMemoryAuthStorageBackend.constructor")
    }
}

#[async_trait::async_trait]
impl AuthStorageBackend for InMemoryAuthStorageBackend {
    fn with_lock(&self, r#fn: Arc<dyn Fn(Option<String>) -> LockResult + Send + Sync>) -> Value {
        todo!("port core/auth-storage.ts:InMemoryAuthStorageBackend.with_lock")
    }
    async fn with_lock_async(
        &self,
        r#fn: Arc<
            dyn Fn(Option<String>) -> BoxFuture<'static, anyhow::Result<LockResult>> + Send + Sync,
        >,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Value> {
        todo!("port core/auth-storage.ts:InMemoryAuthStorageBackend.with_lock_async")
    }
}

/// Credential storage backed by a JSON file.
/// TS: `implements CredentialStore`
pub struct AuthStorage {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl AuthStorage {
    pub fn new(storage: Arc<dyn AuthStorageBackend>, auth_path: Option<&str>) -> Self {
        todo!("port core/auth-storage.ts:AuthStorage.constructor")
    }

    pub fn create(auth_path: Option<&str>) -> Arc<AuthStorage> {
        todo!("port core/auth-storage.ts:AuthStorage.create")
    }

    pub fn from_storage(storage: Arc<dyn AuthStorageBackend>) -> Arc<AuthStorage> {
        todo!("port core/auth-storage.ts:AuthStorage.fromStorage")
    }

    pub fn in_memory(data: Option<&AuthStorageData>) -> Arc<AuthStorage> {
        todo!("port core/auth-storage.ts:AuthStorage.inMemory")
    }

    /// Reload credentials from storage.
    pub fn reload(&self) {
        todo!("port core/auth-storage.ts:AuthStorage.reload")
    }

    pub async fn read(
        &self,
        provider: &str,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Option<Credential>> {
        todo!("port core/auth-storage.ts:AuthStorage.read")
    }

    pub async fn modify(
        &self,
        provider: &str,
        r#fn: Arc<
            dyn Fn(Option<Credential>) -> BoxFuture<'static, anyhow::Result<Option<Credential>>>
                + Send
                + Sync,
        >,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Option<Credential>> {
        todo!("port core/auth-storage.ts:AuthStorage.modify")
    }

    pub async fn delete(
        &self,
        provider: &str,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/auth-storage.ts:AuthStorage.delete")
    }

    /// List credential metadata without resolving configured key values.
    pub async fn list(
        &self,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Vec<CredentialInfo>> {
        todo!("port core/auth-storage.ts:AuthStorage.list")
    }
}

// TODO(port): impl pi-ai::CredentialStore for AuthStorage

/// One-off synchronous read of a stored credential from an auth.json file,
/// without instantiating a store or resolving configured key values.
pub fn read_stored_credential(provider_id: &str, auth_path: Option<&str>) -> Option<Credential> {
    todo!("port core/auth-storage.ts:readStoredCredential")
}
