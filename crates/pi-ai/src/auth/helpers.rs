//! Auth helpers (port of `auth/helpers.ts`).

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

use super::types::{
    ApiKeyAuth, ApiKeyCredential, ApiKeyResolveInput, AuthPrompt, AuthResult, LoginOptions, ModelAuth, OAuthAuth,
    OAuthCredential, ProviderAuthInteraction,
};

/// Standard api-key auth: a stored credential key wins, otherwise the first set env var.
pub struct EnvApiKeyAuth {
    name: String,
    env_vars: Vec<String>,
}

pub fn env_api_key_auth(name: &str, env_vars: &[&str]) -> Arc<dyn ApiKeyAuth> {
    Arc::new(EnvApiKeyAuth { name: name.to_string(), env_vars: env_vars.iter().map(|s| s.to_string()).collect() })
}

#[async_trait]
impl ApiKeyAuth for EnvApiKeyAuth {
    fn name(&self) -> &str {
        &self.name
    }

    fn supports_login(&self) -> bool {
        true
    }

    async fn login(&self, interaction: ProviderAuthInteraction) -> anyhow::Result<ApiKeyCredential> {
        interaction.throw_if_aborted()?;
        let key = interaction.interaction.prompt(AuthPrompt::secret(format!("Enter {}", self.name))).await?;
        interaction.throw_if_aborted()?;
        Ok(ApiKeyCredential { key: Some(key), env: None })
    }

    async fn resolve(&self, input: ApiKeyResolveInput) -> anyhow::Result<Option<AuthResult>> {
        input.throw_if_aborted()?;
        if let Some(credential) = &input.credential
            && let Some(key) = credential.key.as_ref().filter(|k| !k.is_empty()) {
                return Ok(Some(AuthResult {
                    auth: ModelAuth { api_key: Some(key.clone()), ..Default::default() },
                    env: credential.env.clone(),
                    source: Some("stored credential".to_string()),
                }));
            }
        for env_var in &self.env_vars {
            let value = input.ctx.env(env_var).await;
            input.throw_if_aborted()?;
            if let Some(value) = value.filter(|v| !v.is_empty()) {
                return Ok(Some(AuthResult {
                    auth: ModelAuth { api_key: Some(value), ..Default::default() },
                    env: None,
                    source: Some(env_var.clone()),
                }));
            }
        }
        Ok(None)
    }
}

type OAuthLoader = Arc<dyn Fn() -> futures::future::BoxFuture<'static, anyhow::Result<Arc<dyn OAuthAuth>>> + Send + Sync>;

/// Wraps a lazily loaded `OAuthAuth` so provider definitions can advertise OAuth.
pub struct LazyOAuth {
    name: String,
    is_subscription: Option<bool>,
    login_label: Option<String>,
    load: OAuthLoader,
    loaded: OnceCell<Arc<dyn OAuthAuth>>,
}

impl LazyOAuth {
    async fn loaded(&self) -> anyhow::Result<Arc<dyn OAuthAuth>> {
        self.loaded.get_or_try_init(|| (self.load)()).await.cloned()
    }
}

pub fn lazy_oauth(
    name: &str,
    is_subscription: Option<bool>,
    login_label: Option<&str>,
    load: OAuthLoader,
) -> Arc<dyn OAuthAuth> {
    Arc::new(LazyOAuth {
        name: name.to_string(),
        is_subscription,
        login_label: login_label.map(str::to_string),
        load,
        loaded: OnceCell::new(),
    })
}

#[async_trait]
impl OAuthAuth for LazyOAuth {
    fn name(&self) -> &str {
        &self.name
    }
    fn is_subscription(&self) -> Option<bool> {
        self.is_subscription
    }
    fn login_label(&self) -> Option<&str> {
        self.login_label.as_deref()
    }
    async fn login(&self, interaction: ProviderAuthInteraction, options: Option<LoginOptions>) -> anyhow::Result<OAuthCredential> {
        self.loaded().await?.login(interaction, options).await
    }
    async fn refresh(&self, credential: OAuthCredential, signal: CancellationToken) -> anyhow::Result<OAuthCredential> {
        self.loaded().await?.refresh(credential, signal).await
    }
    async fn to_auth(&self, credential: &OAuthCredential) -> anyhow::Result<ModelAuth> {
        self.loaded().await?.to_auth(credential).await
    }
}

/// An OAuth method whose flow is not ported yet; every operation fails with a clear message.
pub fn unavailable_oauth(name: &str, is_subscription: Option<bool>, login_label: Option<&str>) -> Arc<dyn OAuthAuth> {
    let provider_name = name.to_string();
    lazy_oauth(
        name,
        is_subscription,
        login_label,
        Arc::new(move || {
            let provider_name = provider_name.clone();
            Box::pin(async move { Err(anyhow::anyhow!("OAuth flow for {provider_name} is not available in this build")) })
        }),
    )
}
