//! Port of `packages/coding-agent/src/cli/credential-print.ts`.

use crate::cli::args::Args;
use crate::core::model_runtime::ModelRuntime;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CredentialPrintKindValue {
    #[serde(rename = "api_key")]
    ApiKey,
    #[serde(rename = "bearer_token")]
    BearerToken,
}

impl CredentialPrintKindValue {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::BearerToken => "bearer_token",
        }
    }
}

pub type CredentialPrintKind = CredentialPrintKindValue;

/// Resolve one configured provider credential.
///
/// This intentionally calls ModelRuntime.getAuth(), which refreshes and persists
/// OAuth credentials with less than five minutes remaining through the normal request-auth path.
pub async fn resolve_credential_for_print(
    args: &Args,
    model_runtime: Arc<ModelRuntime>,
    kind: &CredentialPrintKind,
    min_expiry_ms: Option<i64>,
    signal: Option<CancellationToken>,
) -> anyhow::Result<String> {
    todo!("port cli/credential-print.ts:resolveCredentialForPrint")
}
