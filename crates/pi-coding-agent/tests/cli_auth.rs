//! Ports of `test/auth-check.test.ts` and `test/credential-print.test.ts`. OAuth refresh spies
//! are a native provider wrapping the built-in one (see `spy_on_refresh`).

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

// ---------------------------------------------------------------------------
// OAuth refresh cases (TS replaces `provider.auth.oauth.refresh` with a spy; here the built-in
// provider is replaced by a native provider whose OAuth auth wraps the built-in one).
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use pi_ai::auth::types::{
    LoginOptions, ModelAuth, OAuthAuth, OAuthCredential, ProviderAuth, ProviderAuthInteraction,
};
use pi_ai::models::Provider;
use pi_ai::types::{AssistantMessageEventStream, Model, StreamOptions, TranscriptContext};
use pi_coding_agent::cli::auth_command::{AuthCommand, AuthCommandKind, is_auth_command_help, parse_auth_command};
use tokio_util::sync::CancellationToken;

struct CountingOAuth {
    inner: Arc<dyn OAuthAuth>,
    calls: Arc<AtomicUsize>,
    /// `None`: delegate to the built-in refresh.
    refreshed: Option<OAuthCredential>,
}

#[async_trait]
impl OAuthAuth for CountingOAuth {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn is_subscription(&self) -> Option<bool> {
        self.inner.is_subscription()
    }
    async fn login(&self, interaction: ProviderAuthInteraction, options: Option<LoginOptions>) -> anyhow::Result<OAuthCredential> {
        self.inner.login(interaction, options).await
    }
    async fn refresh(&self, credential: OAuthCredential, signal: CancellationToken) -> anyhow::Result<OAuthCredential> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.refreshed {
            Some(refreshed) => Ok(refreshed.clone()),
            None => self.inner.refresh(credential, signal).await,
        }
    }
    async fn to_auth(&self, credential: &OAuthCredential) -> anyhow::Result<ModelAuth> {
        self.inner.to_auth(credential).await
    }
}

struct SpyProvider {
    inner: Arc<dyn Provider>,
    auth: ProviderAuth,
}

#[async_trait]
impl Provider for SpyProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn auth(&self) -> &ProviderAuth {
        &self.auth
    }
    fn get_models(&self) -> Vec<Model> {
        self.inner.get_models()
    }
    fn stream(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        self.inner.stream(model, context, options)
    }
    fn stream_simple(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        self.inner.stream_simple(model, context, options)
    }
}

/// Replace `provider_id`'s OAuth refresh with a counting test double.
fn spy_on_refresh(runtime: &ModelRuntime, provider_id: &str, refreshed: Option<OAuthCredential>) -> Arc<AtomicUsize> {
    let inner = runtime.get_provider(provider_id).expect("provider is registered");
    let oauth = inner.auth().oauth.clone().expect("provider has OAuth");
    let calls = Arc::new(AtomicUsize::new(0));
    let auth = ProviderAuth {
        api_key: inner.auth().api_key.clone(),
        oauth: Some(Arc::new(CountingOAuth { inner: oauth, calls: calls.clone(), refreshed })),
    };
    runtime.register_native_provider(Arc::new(SpyProvider { inner, auth })).unwrap();
    calls
}

fn fresh_oauth(access: &str) -> OAuthCredential {
    OAuthCredentials {
        access: access.into(),
        refresh: "refresh-token".into(),
        expires: chrono::Utc::now().timestamp_millis() + 60 * 60 * 1000,
        extra: Default::default(),
    }
}

#[tokio::test]
async fn reads_credentials_without_calling_oauth_refresh_when_requested() {
    let credentials = storage(&[("openai-codex", oauth("old-token", 0))]);
    let oauth_runtime = runtime(credentials.clone()).await;
    let calls = spy_on_refresh(&oauth_runtime, "openai-codex", None);
    assert_eq!(
        get_provider_credential("openai-codex", oauth_runtime, credentials, &GetProviderCredentialOptions { refresh: false })
            .await
            .unwrap()
            .as_deref(),
        Some("old-token")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn refreshes_oauth_by_default() {
    let credentials = storage(&[("openai-codex", oauth("old-token", 0))]);
    let runtime = runtime(credentials).await;
    let calls = spy_on_refresh(&runtime, "openai-codex", Some(fresh_oauth("fresh-token")));
    let result = check_provider_auth(
        &args(&["--provider", "openai-codex"]),
        runtime,
        Some(&pi_coding_agent::cli::auth_check::CheckProviderAuthOptions { refresh: true }),
    )
    .await
    .unwrap();
    assert_eq!(result.status, AuthCheckStatus::Ready);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn prints_bearer_tokens_resolved_from_an_authorization_header() {
    let far_future = chrono::Utc::now().timestamp_millis() + 60 * 60 * 1000;
    let runtime = runtime(storage(&[("kimi-coding", oauth("header-test-token", far_future))])).await;
    let value = resolve_credential_for_print(
        &args(&["--provider", "kimi-coding"]),
        runtime,
        &CredentialPrintKindValue::BearerToken,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(value, "header-test-token");
}

#[tokio::test]
async fn refreshes_an_expired_oauth_token_before_printing_it() {
    let credentials = storage(&[("openai-codex", oauth("old-test-token", 0))]);
    let runtime = runtime(credentials.clone()).await;
    let calls = spy_on_refresh(&runtime, "openai-codex", Some(fresh_oauth("fresh-test-token")));
    let value = resolve_credential_for_print(
        &args(&["--provider", "openai-codex"]),
        runtime,
        &CredentialPrintKindValue::BearerToken,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(value, "fresh-test-token");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    match credentials.read("openai-codex", None).await.unwrap() {
        Some(Credential::OAuth(stored)) => assert_eq!(stored.access, "fresh-test-token"),
        other => panic!("expected stored OAuth credential, got {other:?}"),
    }
}

// TS calls `main()` in-process and spies on console.error; here the `pi` binary runs with an
// isolated agent directory.
#[test]
fn reports_unknown_auth_options_like_package_commands() {
    let dir = temp_dir();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pi"))
        .args(["auth", "check", "--provider", "openai-codex", "--credentails"])
        .env("PI_CODING_AGENT_DIR", dir.join("agent"))
        .env("HOME", &dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown option --credentails for \"auth check\"."), "{stderr}");
    assert!(
        stderr.contains("Use \"pi --help\" or \"pi auth check --provider <provider> [--json] [--credentials] [--no-refresh]\"."),
        "{stderr}"
    );
    assert_eq!(output.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(dir);
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn accepts_optional_json_output_credential_output_and_no_refresh() {
    assert_eq!(
        parse_auth_command(&strings(&["auth", "check", "--provider", "openai"])).unwrap(),
        Some(AuthCommand {
            kind: AuthCommandKind::Check,
            args: strings(&["--provider", "openai"]),
            json: false,
            credentials: false,
            no_refresh: false,
            min_expiry_ms: None,
        })
    );
    assert_eq!(
        parse_auth_command(&strings(&["auth", "check", "--json", "--credentials", "--no-refresh", "--provider", "openai"]))
            .unwrap(),
        Some(AuthCommand {
            kind: AuthCommandKind::Check,
            args: strings(&["--provider", "openai"]),
            json: true,
            credentials: true,
            no_refresh: true,
            min_expiry_ms: None,
        })
    );
}

#[test]
fn parses_credential_commands() {
    assert_eq!(
        parse_auth_command(&strings(&["auth", "print-api-key", "--provider", "openai"])).unwrap(),
        Some(AuthCommand {
            kind: AuthCommandKind::ApiKey,
            args: strings(&["--provider", "openai"]),
            json: false,
            credentials: false,
            no_refresh: false,
            min_expiry_ms: None,
        })
    );
    assert_eq!(
        parse_auth_command(&strings(&["auth", "print-bearer-token"])).unwrap().map(|c| c.kind),
        Some(AuthCommandKind::BearerToken)
    );
    assert_eq!(
        parse_auth_command(&strings(&["auth", "print-bearer-token", "--min-expiry", "30m"])).unwrap(),
        Some(AuthCommand {
            kind: AuthCommandKind::BearerToken,
            args: vec![],
            json: false,
            credentials: false,
            no_refresh: false,
            min_expiry_ms: Some(30 * 60_000),
        })
    );
    let error = parse_auth_command(&strings(&["auth", "print-api-key", "--min-expiry", "30m"])).unwrap_err();
    assert!(error.to_string().contains("only supported by print-bearer-token"));
    assert!(is_auth_command_help(&strings(&["auth", "--help"])));
    assert!(is_auth_command_help(&strings(&["auth", "print-api-key", "--help"])));
    assert!(is_auth_command_help(&strings(&["auth", "print-bearer-token", "-h"])));
    assert!(is_auth_command_help(&strings(&["auth", "check", "--help"])));
    let error = parse_auth_command(&strings(&["auth", "unknown"])).unwrap_err();
    assert!(error.downcast_ref::<pi_coding_agent::cli::auth_command::AuthCommandError>().is_some());
}
