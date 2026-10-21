//! Legacy extension OAuth callback types (port of `compat/extension-oauth-types.ts`).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

pub use super::types::OAuthCredentials;

/// Legacy extension OAuth prompt.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthPrompt {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_empty: Option<bool>,
}

/// Legacy extension OAuth authorization link.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OAuthAuthInfo {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Legacy extension OAuth device-code notification.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthDeviceCodeInfo {
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OAuthSelectOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OAuthSelectPrompt {
    pub message: String,
    pub options: Vec<OAuthSelectOption>,
}

/// Callback surface retained for coding-agent extension compatibility.
#[async_trait]
pub trait OAuthLoginCallbacks: Send + Sync {
    fn on_auth(&self, info: OAuthAuthInfo);
    fn on_device_code(&self, info: OAuthDeviceCodeInfo);
    async fn on_prompt(&self, prompt: OAuthPrompt) -> anyhow::Result<String>;
    fn on_progress(&self, _message: &str) {}
    /// `None` when manual code input is not supported.
    async fn on_manual_code_input(&self) -> Option<anyhow::Result<String>> {
        None
    }
    async fn on_select(&self, prompt: OAuthSelectPrompt) -> anyhow::Result<Option<String>>;
    fn signal(&self) -> Option<CancellationToken> {
        None
    }
}
