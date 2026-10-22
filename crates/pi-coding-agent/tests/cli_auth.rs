//! Ports of `test/auth-check.test.ts` and `test/credential-print.test.ts` (the cases that do
//! not mock OAuth refresh functions).

use std::sync::Arc;

use indexmap::IndexMap;
use pi_ai::auth::types::{ApiKeyCredential, Credential, CredentialStore, OAuthCredentials};
use pi_coding_agent::cli::args::parse_args;
use pi_coding_agent::cli::auth_check::{
    AuthCheckReason, AuthCheckResult, AuthCheckResultAuthType, AuthCheckStatus, GetProviderCredentialOptions,
    check_provider_auth, create_auth_check_model_runtime, get_provider_credential,
};
use pi_coding_agent::cli::credential_print::{CredentialPrintKindValue, resolve_credential_for_print};
use pi_coding_agent::core::auth_storage::{AuthStorage, ReadOnlyAuthStorage};
use pi_coding_agent::core::model_runtime::{CreateModelRuntimeOptions, ModelRuntime};
use pi_coding_agent::core::models_store::InMemoryCodingAgentModelsStore;

fn args(list: &[&str]) -> pi_coding_agent::cli::args::Args {
    parse_args(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

fn api_key(key: &str) -> Credential {
    Credential::ApiKey(ApiKeyCredential { key: Some(key.into()), env: None })
}

fn oauth(access: &str, expires: i64) -> Credential {
    Credential::OAuth(OAuthCredentials {
        refresh: "test-refresh-token".into(),
        access: access.into(),
        expires,
        extra: Default::default(),
    })
}

fn storage(entries: &[(&str, Credential)]) -> Arc<AuthStorage> {
    let data: IndexMap<String, Credential> = entries.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    AuthStorage::in_memory(Some(&data))
}

async fn runtime(credentials: Arc<dyn CredentialStore>) -> Arc<ModelRuntime> {
    ModelRuntime::create(Some(&CreateModelRuntimeOptions {
        credentials: Some(credentials),
        models_store: Some(Arc::new(InMemoryCodingAgentModelsStore::new())),
        allow_model_network: Some(false),
        refresh_on_create: Some(false),
        ..Default::default()
    }))
    .await
    .unwrap()
}

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pi-test-auth-check-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn reports_a_configured_provider_as_ready() {
    let runtime = runtime(storage(&[("openai", api_key("test-key"))])).await;
    assert_eq!(
        check_provider_auth(&args(&["--provider", "openai"]), runtime, None).await.unwrap(),
        AuthCheckResult {
            status: AuthCheckStatus::Ready,
            provider: "openai".into(),
            reason: None,
            auth_type: Some(AuthCheckResultAuthType::ApiKey),
        }
    );
}

#[tokio::test]
async fn resolves_the_provider_from_model() {
    let runtime = runtime(storage(&[("openai", api_key("test-key"))])).await;
    let result = check_provider_auth(&args(&["--model", "openai/gpt-5.5"]), runtime.clone(), None).await.unwrap();
    assert_eq!((result.status, result.provider.as_str()), (AuthCheckStatus::Ready, "openai"));
    let result =
        check_provider_auth(&args(&["--provider", "openai", "--model", "gpt-5.5"]), runtime, None).await.unwrap();
    assert_eq!((result.status, result.provider.as_str()), (AuthCheckStatus::Ready, "openai"));
}

#[tokio::test]
async fn reads_credentials_without_refreshing_oauth_when_requested() {
    let api_credentials = storage(&[("openai", api_key("test-key"))]);
    let api_runtime = runtime(api_credentials.clone()).await;
    assert_eq!(
        get_provider_credential("openai", api_runtime, api_credentials, &GetProviderCredentialOptions { refresh: false })
            .await
            .unwrap()
            .as_deref(),
        Some("test-key")
    );
    let credentials = storage(&[("openai-codex", oauth("old-token", 0))]);
    let oauth_runtime = runtime(credentials.clone()).await;
    assert_eq!(
        get_provider_credential("openai-codex", oauth_runtime, credentials, &GetProviderCredentialOptions { refresh: false })
            .await
            .unwrap()
            .as_deref(),
        Some("old-token")
    );
}

#[tokio::test]
async fn reports_an_unknown_provider_as_not_ready() {
    let runtime = runtime(storage(&[])).await;
    assert_eq!(
        check_provider_auth(&args(&["--provider", "not-installed"]), runtime, None).await.unwrap(),
        AuthCheckResult {
            status: AuthCheckStatus::NotReady,
            provider: "not-installed".into(),
            reason: Some(AuthCheckReason::ProviderNotFound),
            auth_type: None,
        }
    );
}

#[tokio::test]
async fn does_not_treat_an_unresolved_stored_environment_reference_as_configured() {
    let dir = temp_dir();
    let auth_path = dir.join("auth.json");
    std::fs::write(&auth_path, r#"{"openai":{"type":"api_key","key":"$MISSING_AUTH_CHECK_KEY"}}"#).unwrap();
    let runtime = runtime(Arc::new(ReadOnlyAuthStorage::new(Some(auth_path.to_str().unwrap())))).await;
    assert_eq!(
        check_provider_auth(&args(&["--provider", "openai"]), runtime, None).await.unwrap(),
        AuthCheckResult {
            status: AuthCheckStatus::NotReady,
            provider: "openai".into(),
            reason: Some(AuthCheckReason::CredentialsNotConfigured),
            auth_type: None,
        }
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn reports_malformed_auth_state_as_invalid() {
    let dir = temp_dir();
    let auth_path = dir.join("auth.json");
    std::fs::write(&auth_path, "{invalid-json").unwrap();
    let runtime = runtime(Arc::new(ReadOnlyAuthStorage::new(Some(auth_path.to_str().unwrap())))).await;
    assert_eq!(
        check_provider_auth(&args(&["--provider", "openai"]), runtime, None).await.unwrap(),
        AuthCheckResult {
            status: AuthCheckStatus::Invalid,
            provider: "openai".into(),
            reason: Some(AuthCheckReason::InvalidState),
            auth_type: None,
        }
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn does_not_create_an_auth_file_or_its_parent_directory() {
    let dir = temp_dir();
    let auth_path = dir.join("agent").join("auth.json");
    let runtime = runtime(Arc::new(ReadOnlyAuthStorage::new(Some(auth_path.to_str().unwrap())))).await;
    let result = check_provider_auth(&args(&["--provider", "openai"]), runtime, None).await.unwrap();
    assert_eq!(result.status, AuthCheckStatus::NotReady);
    assert_eq!(result.reason, Some(AuthCheckReason::CredentialsNotConfigured));
    assert!(!auth_path.exists());
    assert!(!dir.join("agent").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn creates_an_auth_check_runtime_without_catalog_storage() {
    let runtime = create_auth_check_model_runtime(storage(&[])).await.unwrap();
    assert!(runtime.get_provider("openai").is_some());
}

#[tokio::test]
async fn prints_a_resolved_api_key() {
    let runtime = runtime(storage(&[("openai", api_key("test-api-key"))])).await;
    let value = resolve_credential_for_print(
        &args(&["--provider", "openai"]),
        runtime,
        &CredentialPrintKindValue::ApiKey,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(value, "test-api-key");
}

#[tokio::test]
async fn resolves_model_without_provider_only_among_configured_providers() {
    let runtime =
        runtime(storage(&[("openai", api_key("openai-test-key")), ("anthropic", api_key("anthropic-test-key"))])).await;
    let value = resolve_credential_for_print(
        &args(&["--model", "gpt-5.5"]),
        runtime,
        &CredentialPrintKindValue::ApiKey,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(value, "openai-test-key");
}

#[tokio::test]
async fn rejects_invalid_arguments_or_credential_types() {
    let far_future = chrono::Utc::now().timestamp_millis() + 60 * 60 * 1000;
    let runtime = runtime(storage(&[("openai-codex", oauth("test-token-not-to-be-printed", far_future))])).await;
    let err = resolve_credential_for_print(&args(&[]), runtime.clone(), &CredentialPrintKindValue::ApiKey, None, None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("requires --provider <provider> or --model <model>"), "{err}");
    let err = resolve_credential_for_print(
        &args(&["--provider", "openai-codex"]),
        runtime,
        &CredentialPrintKindValue::ApiKey,
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("configured with OAuth"), "{err}");
}
