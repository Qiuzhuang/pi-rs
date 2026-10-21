//! Port of `packages/coding-agent/src/core/runtime-credentials.ts`.

use crate::compat::AuthOperationOptions;
use futures::future::BoxFuture;
use pi_ai::auth::types::Credential;
use pi_ai::auth::types::CredentialInfo;
use pi_ai::auth::types::CredentialStore;
use std::sync::Arc;

/// Async credential store overlay for non-persistent runtime API keys.
/// TS: `implements CredentialStore`
pub struct RuntimeCredentials {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl RuntimeCredentials {
    pub fn new(store: Arc<dyn CredentialStore>) -> Self {
        todo!("port core/runtime-credentials.ts:RuntimeCredentials.constructor")
    }

    pub fn set_runtime_api_key(&self, provider_id: &str, api_key: &str) {
        todo!("port core/runtime-credentials.ts:RuntimeCredentials.setRuntimeApiKey")
    }

    pub fn remove_runtime_api_key(&self, provider_id: &str) {
        todo!("port core/runtime-credentials.ts:RuntimeCredentials.removeRuntimeApiKey")
    }

    pub fn has_runtime_api_key(&self, provider_id: &str) -> bool {
        todo!("port core/runtime-credentials.ts:RuntimeCredentials.hasRuntimeApiKey")
    }

    pub async fn read(
        &self,
        provider_id: &str,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Option<Credential>> {
        todo!("port core/runtime-credentials.ts:RuntimeCredentials.read")
    }

    pub async fn list(
        &self,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Vec<CredentialInfo>> {
        todo!("port core/runtime-credentials.ts:RuntimeCredentials.list")
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
        todo!("port core/runtime-credentials.ts:RuntimeCredentials.modify")
    }

    pub async fn delete(
        &self,
        provider_id: &str,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/runtime-credentials.ts:RuntimeCredentials.delete")
    }
}

// TODO(port): impl pi-ai::CredentialStore for RuntimeCredentials
