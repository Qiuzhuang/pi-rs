//! OAuth cases not covered by `tests/oauth_flows.rs`: `anthropic-oauth.test.ts`,
//! `openai-codex-oauth.test.ts`, `github-copilot-oauth.test.ts`, `kimi-coding-oauth.test.ts`,
//! `meta-oauth.test.ts`, `xai-oauth.test.ts` and `openrouter-oauth.test.ts`.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_ai::auth::oauth::*;
use pi_ai::auth::{AuthEvent, AuthType, Credential, CredentialStore, InMemoryCredentialStore, OAuthAuth, OAuthCredential};
use pi_ai::providers::builtin::github_copilot_provider;
use pi_ai::providers::openrouter::openrouter_provider;
use pi_ai::*;
use serde_json::{Map, Value, json};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::oauth_support::*;
use crate::support::{ENV_LOCK, EnvGuard};

fn cred(access: &str, refresh: &str, extra: Value) -> OAuthCredential {
    OAuthCredential { access: access.into(), refresh: refresh.into(), expires: 0, extra: extra.as_object().cloned().unwrap_or_default() }
}

fn never_prompt() -> PromptHandler {
    Arc::new(|prompt, _| Box::pin(async move { anyhow::bail!("Unexpected prompt: {:?}", prompt.kind) }))
}

fn elapsed_ms(start: Instant) -> u128 {
    start.elapsed().as_millis()
}

// ---------------------------------------------------------------------------
// Port of anthropic-oauth.test.ts ("resolves through the manual_code prompt and aborts it after settling")
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_login_accepts_a_bare_manual_code_and_aborts_the_prompt() {
    let _guard = PORT_LOCK.lock().await;
    let stub = stub_fetch(|request, _| {
        assert!(request.url.contains("/oauth/token"));
        json_response(200, json!({"access_token": "access", "refresh_token": "refresh", "expires_in": 3600}))
    });
    let (login, recorder) = interaction(answering("browser", |_, _| Ok("the-code".into())), None, None);
    let credential = with_oauth_fetch(stub.fetch.clone(), anthropic::AnthropicOAuth.login(login, None)).await.unwrap();
    assert_eq!(credential.access, "access");
    assert!(recorder.events.lock().iter().any(|e| matches!(e, AuthEvent::AuthUrl { .. })));
    assert!(recorder.prompts.lock().iter().any(|p| p.kind == pi_ai::auth::AuthPromptKind::ManualCode));
    assert!(recorder.manual_signal().unwrap().is_cancelled());
    assert_eq!(stub.requests.lock()[0].json_body().unwrap()["code"], "the-code");
}

// ---------------------------------------------------------------------------
// Port of openai-codex-oauth.test.ts ("cancels when OpenAI Codex login method selection is cancelled")
// ---------------------------------------------------------------------------

#[tokio::test]
async fn codex_login_cancels_when_method_selection_is_cancelled() {
    let (login, _) = interaction(Arc::new(|_, _| Box::pin(async { anyhow::bail!("Login cancelled") })), None, None);
    let error = openai_codex::OpenAICodexOAuth.login(login, None).await.unwrap_err();
    assert_eq!(error.to_string(), "Login cancelled");
}

// ---------------------------------------------------------------------------
// Port of github-copilot-oauth.test.ts
// ---------------------------------------------------------------------------

const COPILOT_TOKEN: &str = "tid=test;exp=9999999999;proxy-ep=proxy.individual.githubcopilot.com;";
const COPILOT_MODELS: &str = "https://api.individual.githubcopilot.com/models";

fn provider_model_ids() -> Vec<String> {
    github_copilot_provider().get_models().into_iter().map(|m| m.id).collect()
}

async fn refresh_copilot_with(data: Value, proxy_host: &str) -> OAuthCredential {
    let access = format!("tid=test;exp=9999999999;proxy-ep={proxy_host};");
    let models_url = format!("https://{}/models", proxy_host.replacen("proxy.", "api.", 1));
    let expected_auth = format!("Bearer {access}");
    let stub = stub_fetch(move |request, _| {
        if request.url.contains("/copilot_internal/v2/token") {
            return json_response(200, json!({"token": access, "expires_at": 9999999999u64}));
        }
        if request.url == models_url {
            assert_eq!(request.header_value("Authorization"), Some(expected_auth.as_str()));
            return json_response(200, json!({"data": data}));
        }
        anyhow::bail!("Unexpected fetch URL: {}", request.url)
    });
    with_oauth_fetch(stub.fetch.clone(), github_copilot::GitHubCopilotOAuth.refresh(cred("old-access-token", "ghu_refresh_token", json!({})), CancellationToken::new()))
        .await
        .unwrap()
}

async fn available_with(credential: OAuthCredential) -> Vec<String> {
    let store = Arc::new(InMemoryCredentialStore::new());
    store
        .modify("github-copilot", Box::new(move |_| Box::pin(async move { Ok(Some(Credential::OAuth(credential))) })), None)
        .await
        .unwrap();
    let models = create_models(CreateModelsOptions { credentials: Some(store), ..Default::default() });
    models.set_provider(github_copilot_provider());
    models.get_available(Some("github-copilot"), None).await.unwrap().into_iter().map(|m| m.id).collect()
}

fn tools(enabled: bool) -> Value {
    json!({"supports": {"tool_calls": enabled}})
}

#[tokio::test]
async fn copilot_refresh_filters_models_to_the_account_picker_catalog() {
    let ids = provider_model_ids();
    let credential = refresh_copilot_with(
        json!([
            {"id": ids[0], "model_picker_enabled": true, "capabilities": tools(true)},
            {"id": ids[1], "model_picker_enabled": true, "policy": {"state": "disabled"}, "capabilities": tools(true)},
            {"id": ids[2], "model_picker_enabled": false, "policy": {"state": "enabled"}, "capabilities": tools(true)},
        ]),
        "proxy.individual.githubcopilot.com",
    )
    .await;
    assert_eq!(credential.extra.get("availableModelIds"), Some(&json!([ids[0]])));
    assert_eq!(available_with(credential).await, vec![ids[0].clone()]);
}

#[tokio::test]
async fn copilot_refresh_falls_back_to_explicitly_enabled_policy_models() {
    let ids = provider_model_ids();
    let credential = refresh_copilot_with(
        json!([
            {"id": ids[0], "model_picker_enabled": false, "policy": {"state": "enabled"}, "capabilities": tools(true)},
            {"id": "policy-disabled-model", "model_picker_enabled": false, "policy": {"state": "disabled"}, "capabilities": tools(true)},
            {"id": "unconfigured-model", "model_picker_enabled": false, "capabilities": tools(true)},
            {"id": "tool-incapable-model", "model_picker_enabled": false, "policy": {"state": "enabled"}, "capabilities": tools(false)},
        ]),
        "proxy.individual.githubcopilot.com",
    )
    .await;
    assert_eq!(credential.extra.get("availableModelIds"), Some(&json!([ids[0]])));
    assert_eq!(available_with(credential).await, vec![ids[0].clone()]);

    // Non-Individual accounts get no policy fallback.
    let credential = refresh_copilot_with(
        json!([{"id": "gpt-4.1", "model_picker_enabled": false, "policy": {"state": "enabled"}, "capabilities": tools(true)}]),
        "proxy.business.githubcopilot.com",
    )
    .await;
    assert_eq!(credential.extra.get("availableModelIds"), Some(&json!([])));
}

type PolicyHandler = Arc<dyn Fn(&str, usize) -> anyhow::Result<OAuthHttpResponse> + Send + Sync>;

/// A login transport with device code interval 1 and the given catalog and policy handlers.
fn copilot_login_stub(models: Value, policy: PolicyHandler) -> (FetchStub, Arc<Mutex<Vec<String>>>) {
    let policies = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = policies.clone();
    let stub = stub_fetch(move |request, _| {
        let url = request.url.as_str();
        if url.ends_with("/login/device/code") {
            return json_response(200, json!({"device_code": "device-code", "user_code": "ABCD-EFGH", "verification_uri": "https://github.com/login/device", "interval": 1, "expires_in": 900}));
        }
        if url.ends_with("/login/oauth/access_token") {
            return json_response(200, json!({"access_token": "ghu_refresh_token"}));
        }
        if url.contains("/copilot_internal/v2/token") {
            return json_response(200, json!({"token": COPILOT_TOKEN, "expires_at": 9999999999u64}));
        }
        if url == COPILOT_MODELS {
            return json_response(200, models.clone());
        }
        if let Some(id) = url.strip_prefix(&format!("{COPILOT_MODELS}/")).and_then(|r| r.strip_suffix("/policy")) {
            let count = {
                let mut list = sink.lock();
                list.push(id.to_string());
                list.len()
            };
            return policy(id, count);
        }
        anyhow::bail!("Unexpected fetch URL: {url}")
    });
    (stub, policies)
}

fn empty_interaction() -> (pi_ai::auth::ProviderAuthInteraction, Arc<Recorder>) {
    interaction(answering("unused", |_, _| Ok(String::new())), None, None)
}

#[tokio::test(start_paused = true)]
async fn copilot_login_updates_only_known_tool_capable_unconfigured_policies() {
    let ids = provider_model_ids();
    let models = json!({"data": [
        {"id": ids[0], "model_picker_enabled": true, "policy": {"state": "enabled"}, "capabilities": tools(true)},
        {"id": ids[1], "model_picker_enabled": true, "policy": {"state": "unconfigured"}, "capabilities": tools(true)},
        {"id": "remote-only-model", "model_picker_enabled": true, "policy": {"state": "unconfigured"}, "capabilities": tools(true)},
        {"id": ids[2], "model_picker_enabled": true, "policy": {"state": "unconfigured"}, "capabilities": tools(false)},
    ]});
    let (stub, policies) = copilot_login_stub(models, Arc::new(|_, _| Ok(OAuthHttpResponse::new(200, ""))));
    let (login, _) = empty_interaction();
    with_oauth_fetch(stub.fetch.clone(), github_copilot::GitHubCopilotOAuth.login(login, None)).await.unwrap();
    assert_eq!(stub.requests.lock().iter().filter(|r| r.url == COPILOT_MODELS).count(), 1);
    assert_eq!(*policies.lock(), vec![ids[1].clone()]);
}

#[tokio::test(start_paused = true)]
async fn copilot_retries_a_throttled_policy_update_after_retry_after() {
    let ids = provider_model_ids();
    let models = json!({"data": [{"id": ids[0], "model_picker_enabled": true, "policy": {"state": "unconfigured"}}]});
    let times = Arc::new(Mutex::new(Vec::<u128>::new()));
    let start = Instant::now();
    let sink = times.clone();
    let (stub, policies) = copilot_login_stub(
        models,
        Arc::new(move |_, count| {
            sink.lock().push(elapsed_ms(start));
            if count == 1 {
                Ok(OAuthHttpResponse::json_response(429, &json!({"error": "too many requests"})).with_header("Retry-After", "1"))
            } else {
                Ok(OAuthHttpResponse::new(200, ""))
            }
        }),
    );
    let (login, _) = empty_interaction();
    with_oauth_fetch(stub.fetch.clone(), github_copilot::GitHubCopilotOAuth.login(login, None)).await.unwrap();
    assert_eq!(policies.lock().len(), 2);
    let times = times.lock().clone();
    assert_eq!(times[1] - times[0], 1000, "{times:?}");
}

#[tokio::test(start_paused = true)]
async fn copilot_continues_policy_updates_after_a_transport_failure() {
    let ids = provider_model_ids();
    let models = json!({"data": [
        {"id": ids[0], "model_picker_enabled": true, "policy": {"state": "unconfigured"}},
        {"id": ids[1], "model_picker_enabled": true, "policy": {"state": "unconfigured"}},
    ]});
    let (stub, policies) = copilot_login_stub(
        models,
        Arc::new(|_, count| if count == 1 { anyhow::bail!("fetch failed") } else { Ok(OAuthHttpResponse::new(200, "")) }),
    );
    let (login, _) = empty_interaction();
    with_oauth_fetch(stub.fetch.clone(), github_copilot::GitHubCopilotOAuth.login(login, None)).await.unwrap();
    assert_eq!(*policies.lock(), vec![ids[0].clone(), ids[1].clone()]);
}

#[tokio::test(start_paused = true)]
async fn copilot_stops_policy_updates_past_the_login_budget_and_persists_the_credential() {
    let ids = provider_model_ids();
    let models = json!({"data": [
        {"id": ids[0], "model_picker_enabled": true, "policy": {"state": "unconfigured"}},
        {"id": ids[1], "model_picker_enabled": true, "policy": {"state": "unconfigured"}},
    ]});
    let (stub, policies) = copilot_login_stub(
        models,
        Arc::new(|_, _| Ok(OAuthHttpResponse::json_response(429, &json!({"error": "too many requests"})).with_header("Retry-After", "5"))),
    );
    let store = Arc::new(InMemoryCredentialStore::new());
    let models = create_models(CreateModelsOptions { credentials: Some(store.clone()), ..Default::default() });
    models.set_provider(github_copilot_provider());
    let (login, _) = empty_interaction();
    let credential = with_oauth_fetch(stub.fetch.clone(), models.login("github-copilot", AuthType::OAuth, login.interaction.clone(), None, None))
        .await
        .unwrap();
    let Credential::OAuth(oauth) = &credential else { panic!("expected oauth credential") };
    assert_eq!(oauth.access, COPILOT_TOKEN);
    assert_eq!(*policies.lock(), vec![ids[0].clone()]);
    assert_eq!(store.read("github-copilot", None).await.unwrap(), Some(credential));
}

#[tokio::test(start_paused = true)]
async fn copilot_verification_uris_are_validated_and_normalized() {
    let stub = stub_fetch(|_, _| json_response(200, json!({"device_code": "dc", "user_code": "ABCD-EFGH", "verification_uri": "$(id>/tmp/pwned)", "interval": 1, "expires_in": 900})));
    let (login, recorder) = empty_interaction();
    let error = with_oauth_fetch(stub.fetch.clone(), github_copilot::GitHubCopilotOAuth.login(login, None)).await.unwrap_err();
    assert!(error.to_string().contains("Untrusted verification_uri"), "{error}");
    assert!(recorder.device_codes().is_empty());

    let raw = "https://github.com/login/\u{1b}]8;;evil";
    let normalized = url::Url::parse(raw).unwrap().to_string();
    assert_ne!(normalized, raw);
    let stub = stub_fetch(move |request, _| {
        let url = request.url.as_str();
        if url.ends_with("/login/device/code") {
            return json_response(200, json!({"device_code": "dc", "user_code": "ABCD-EFGH", "verification_uri": raw, "interval": 1, "expires_in": 900}));
        }
        if url.ends_with("/login/oauth/access_token") {
            return json_response(200, json!({"access_token": "ghu_refresh_token"}));
        }
        if url.contains("/copilot_internal/v2/token") {
            return json_response(200, json!({"token": COPILOT_TOKEN, "expires_at": 9999999999u64}));
        }
        if url.ends_with("/models") {
            return json_response(200, json!({"data": []}));
        }
        Ok(OAuthHttpResponse::new(200, ""))
    });
    let (login, recorder) = empty_interaction();
    with_oauth_fetch(stub.fetch.clone(), github_copilot::GitHubCopilotOAuth.login(login, None)).await.unwrap();
    assert_eq!(
        recorder.device_codes(),
        vec![AuthEvent::DeviceCode { user_code: "ABCD-EFGH".into(), verification_uri: normalized, interval_seconds: Some(1), expires_in_seconds: Some(900) }]
    );
}

fn copilot_polling_stub(expires_in: u64, replies: Vec<Value>) -> (FetchStub, Arc<Mutex<Vec<u128>>>) {
    let start = Instant::now();
    let times = Arc::new(Mutex::new(Vec::<u128>::new()));
    let sink = times.clone();
    let replies = Arc::new(Mutex::new(replies));
    let stub = stub_fetch(move |request, _| {
        let url = request.url.as_str();
        if url.ends_with("/login/device/code") {
            assert_eq!(request.header_value("Accept"), Some("application/json"));
            assert_eq!(request.header_value("Content-Type"), Some("application/x-www-form-urlencoded"));
            assert!(request.form_field("client_id").is_some());
            assert_eq!(request.form_field("scope").as_deref(), Some("read:user"));
            return json_response(200, json!({"device_code": "device-code", "user_code": "ABCD-EFGH", "verification_uri": "https://github.com/login/device", "interval": 5, "expires_in": expires_in}));
        }
        if url.ends_with("/login/oauth/access_token") {
            sink.lock().push(elapsed_ms(start));
            assert_eq!(request.form_field("device_code").as_deref(), Some("device-code"));
            assert_eq!(request.form_field("grant_type").as_deref(), Some("urn:ietf:params:oauth:grant-type:device_code"));
            let mut replies = replies.lock();
            if replies.is_empty() {
                anyhow::bail!("Unexpected extra access token poll");
            }
            return json_response(200, replies.remove(0));
        }
        if url.contains("/copilot_internal/v2/token") {
            return json_response(200, json!({"token": COPILOT_TOKEN, "expires_at": 9999999999u64}));
        }
        if url.ends_with("/models") {
            return json_response(200, json!({"data": []}));
        }
        Ok(OAuthHttpResponse::new(200, ""))
    });
    (stub, times)
}

#[tokio::test(start_paused = true)]
async fn copilot_waits_before_polling_and_honors_slow_down_intervals() {
    let (stub, times) = copilot_polling_stub(900, vec![
        json!({"error": "authorization_pending", "error_description": "pending"}),
        json!({"error": "slow_down", "error_description": "slow down", "interval": 7}),
        json!({"access_token": "ghu_refresh_token"}),
    ]);
    let (login, _) = empty_interaction();
    with_oauth_fetch(stub.fetch.clone(), github_copilot::GitHubCopilotOAuth.login(login, None)).await.unwrap();
    assert_eq!(*times.lock(), vec![5000, 10000, 17000]);

    let (stub, times) = copilot_polling_stub(25, vec![
        json!({"error": "slow_down", "error_description": "slow down"}),
        json!({"error": "slow_down", "error_description": "still too fast"}),
        json!({"error": "authorization_pending", "error_description": "pending"}),
    ]);
    let (login, _) = empty_interaction();
    let error = with_oauth_fetch(stub.fetch.clone(), github_copilot::GitHubCopilotOAuth.login(login, None)).await.unwrap_err();
    assert!(error.to_string().contains("Device flow timed out after one or more slow_down responses"), "{error}");
    assert_eq!(*times.lock(), vec![5000, 15000]);
}

// ---------------------------------------------------------------------------
// Port of kimi-coding-oauth.test.ts (poll timing, request bodies, host override)
// ---------------------------------------------------------------------------

const KIMI_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";

fn kimi_device_authorization(interval: u64) -> Value {
    json!({"user_code": "ABCD-1234", "device_code": "device-code-123", "verification_uri": "https://www.kimi.com/code",
        "verification_uri_complete": "https://www.kimi.com/code?user_code=ABCD-1234", "interval": interval, "expires_in": 600})
}

#[tokio::test(start_paused = true)]
async fn kimi_device_login_waits_before_the_first_poll() {
    let _lock = ENV_LOCK.lock().await;
    let start = Instant::now();
    let times = Arc::new(Mutex::new(Vec::<u128>::new()));
    let sink = times.clone();
    let polls = Arc::new(Mutex::new(vec![
        (400u16, json!({"error": "authorization_pending"})),
        (200, json!({"access_token": "access-token", "refresh_token": "refresh-token", "expires_in": 3600})),
    ]));
    let stub = stub_fetch(move |request, _| match request.url.as_str() {
        "https://auth.kimi.com/api/oauth/device_authorization" => {
            assert_eq!(request.header_value("Content-Type"), Some("application/x-www-form-urlencoded"));
            assert_eq!(request.header_value("Accept"), Some("application/json"));
            assert_eq!(request.form_field("client_id").as_deref(), Some(KIMI_CLIENT_ID));
            json_response(200, kimi_device_authorization(5))
        }
        "https://auth.kimi.com/api/oauth/token" => {
            sink.lock().push(elapsed_ms(start));
            assert_eq!(request.form_field("grant_type").as_deref(), Some("urn:ietf:params:oauth:grant-type:device_code"));
            assert_eq!(request.form_field("client_id").as_deref(), Some(KIMI_CLIENT_ID));
            assert_eq!(request.form_field("device_code").as_deref(), Some("device-code-123"));
            let (status, body) = polls.lock().remove(0);
            json_response(status, body)
        }
        other => anyhow::bail!("Unexpected fetch URL: {other}"),
    });
    let (login, _) = interaction(never_prompt(), None, None);
    let before = pi_ai::utils::now_ms();
    let credential = with_oauth_fetch(stub.fetch.clone(), kimi_coding::KimiCodingOAuth.login(login, None)).await.unwrap();
    assert_eq!(*times.lock(), vec![5000, 10000]);
    assert_eq!((credential.access.as_str(), credential.refresh.as_str()), ("access-token", "refresh-token"));
    assert!(credential.expires >= before + 3600 * 1000);
    assert!(credential.extra.is_empty());

    // A refresh sends the stored refresh token and client id.
    let stub = stub_fetch(|request, _| {
        assert_eq!(request.url, "https://auth.kimi.com/api/oauth/token");
        assert_eq!(request.form_field("grant_type").as_deref(), Some("refresh_token"));
        assert_eq!(request.form_field("refresh_token").as_deref(), Some("old-refresh"));
        assert_eq!(request.form_field("client_id").as_deref(), Some(KIMI_CLIENT_ID));
        json_response(200, json!({"access_token": "new-access", "refresh_token": "new-refresh", "expires_in": 3600}))
    });
    let refreshed = with_oauth_fetch(stub.fetch.clone(), kimi_coding::KimiCodingOAuth.refresh(cred("old-access", "old-refresh", json!({})), CancellationToken::new()))
        .await
        .unwrap();
    assert_eq!((refreshed.access.as_str(), refreshed.refresh.as_str()), ("new-access", "new-refresh"));
}

#[tokio::test(start_paused = true)]
async fn kimi_honors_the_oauth_host_override() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::set(&[("KIMI_CODE_OAUTH_HOST", Some("https://auth.example.com/"))]);
    let stub = stub_fetch(|request, _| match request.url.as_str() {
        "https://auth.example.com/api/oauth/device_authorization" => json_response(200, kimi_device_authorization(1)),
        "https://auth.example.com/api/oauth/token" => json_response(200, json!({"access_token": "a", "refresh_token": "r", "expires_in": 60})),
        other => anyhow::bail!("Unexpected fetch URL: {other}"),
    });
    let (login, _) = interaction(never_prompt(), None, None);
    let credential = with_oauth_fetch(stub.fetch.clone(), kimi_coding::KimiCodingOAuth.login(login, None)).await.unwrap();
    assert_eq!((credential.access.as_str(), credential.refresh.as_str()), ("a", "r"));
    let urls: Vec<String> = stub.requests.lock().iter().map(|r| r.url.clone()).collect();
    assert_eq!(urls, vec!["https://auth.example.com/api/oauth/device_authorization", "https://auth.example.com/api/oauth/token"]);
}

// ---------------------------------------------------------------------------
// Port of meta-oauth.test.ts (poll timing and key lifetime, refresh re-mint)
// ---------------------------------------------------------------------------

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

#[tokio::test(start_paused = true)]
async fn meta_device_login_and_refresh_re_mint_the_key() {
    let start = Instant::now();
    let times = Arc::new(Mutex::new(Vec::<u128>::new()));
    let sink = times.clone();
    let polls = Arc::new(Mutex::new(vec![(400u16, json!({"error": "authorization_pending"})), (200, json!({"access_token": "identity-token", "token_type": "Bearer"}))]));
    let stub = stub_fetch(move |request, _| match request.url.as_str() {
        "https://auth.meta.com/oidc/device/authorization/" => {
            assert!(request.form_field("client_id").is_some());
            json_response(200, json!({"device_code": "device-code-123", "user_code": "ABCD-1234", "verification_uri": "https://auth.meta.com/oauth/device/",
                "verification_uri_complete": "https://auth.meta.com/oauth/device/?code=ABCD-1234", "interval": 5, "expires_in": 600}))
        }
        "https://auth.meta.com/oidc/device/token/" => {
            sink.lock().push(elapsed_ms(start));
            assert_eq!(request.form_field("grant_type").as_deref(), Some("urn:ietf:params:oauth:grant-type:device_code"));
            assert_eq!(request.form_field("device_code").as_deref(), Some("device-code-123"));
            let (status, body) = polls.lock().remove(0);
            json_response(status, body)
        }
        "https://api.meta.ai/muse-code/key" => {
            assert_eq!(request.header_value("Authorization"), Some("Bearer identity-token"));
            json_response(200, json!({"api_key": "LLM|minted-key"}))
        }
        other => anyhow::bail!("Unexpected fetch URL: {other}"),
    });
    let (login, recorder) = interaction(never_prompt(), None, None);
    let before = pi_ai::utils::now_ms();
    let credential = with_oauth_fetch(stub.fetch.clone(), meta::MetaOAuth.login(login, None)).await.unwrap();
    assert_eq!(recorder.device_codes()[0], AuthEvent::DeviceCode {
        user_code: "ABCD-1234".into(),
        verification_uri: "https://auth.meta.com/oauth/device/?code=ABCD-1234".into(),
        interval_seconds: Some(5),
        expires_in_seconds: Some(600),
    });
    assert_eq!(times.lock().len(), 2);
    assert_eq!((credential.refresh.as_str(), credential.access.as_str()), ("identity-token", "LLM|minted-key"));
    assert!(credential.expires >= before + DAY_MS && credential.expires <= pi_ai::utils::now_ms() + DAY_MS);

    let stub = stub_fetch(|request, _| {
        assert_eq!(request.url, "https://api.meta.ai/muse-code/key");
        assert_eq!(request.header_value("Authorization"), Some("Bearer identity-token"));
        json_response(200, json!({"api_key": "LLM|fresh-key"}))
    });
    let before = pi_ai::utils::now_ms();
    let refreshed = with_oauth_fetch(stub.fetch.clone(), meta::MetaOAuth.refresh(cred("LLM|old-key", "identity-token", json!({})), CancellationToken::new()))
        .await
        .unwrap();
    assert_eq!((refreshed.refresh.as_str(), refreshed.access.as_str()), ("identity-token", "LLM|fresh-key"));
    assert!(refreshed.expires >= before + DAY_MS && refreshed.expires <= pi_ai::utils::now_ms() + DAY_MS);
}

// ---------------------------------------------------------------------------
// Port of xai-oauth.test.ts
// ---------------------------------------------------------------------------

const XAI_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";

fn xai_device(overrides: Value) -> Value {
    let mut value = json!({"device_code": "device-code", "user_code": "ABCD-1234", "verification_uri": "https://accounts.x.ai/oauth2/device", "expires_in": 900, "interval": 5});
    for (k, v) in overrides.as_object().unwrap() {
        value[k] = v.clone();
    }
    value
}

fn xai_token(overrides: Value) -> Value {
    let mut value = json!({"access_token": "access-token", "refresh_token": "refresh-token", "expires_in": 21600, "token_type": "Bearer"});
    for (k, v) in overrides.as_object().unwrap() {
        if v.is_null() {
            value.as_object_mut().unwrap().remove(k);
        } else {
            value[k] = v.clone();
        }
    }
    value
}

#[tokio::test(start_paused = true)]
async fn xai_device_grant_delays_polling_and_handles_pending_and_slow_down() {
    let start = Instant::now();
    let times = Arc::new(Mutex::new(Vec::<u128>::new()));
    let sink = times.clone();
    let replies = Arc::new(Mutex::new(vec![
        (400u16, json!({"error": "authorization_pending"})),
        (400, json!({"error": "slow_down", "interval": 10})),
        (200, xai_token(json!({}))),
    ]));
    let stub = stub_fetch(move |request, _| match request.url.as_str() {
        "https://auth.x.ai/oauth2/device/code" => {
            assert_eq!(request.form_field("client_id").as_deref(), Some(XAI_CLIENT_ID));
            assert_eq!(request.form_field("scope").as_deref(), Some("openid profile email offline_access grok-cli:access api:access"));
            assert_eq!(request.form_field("referrer").as_deref(), Some("pi"));
            json_response(200, xai_device(json!({})))
        }
        "https://auth.x.ai/oauth2/token" => {
            sink.lock().push(elapsed_ms(start));
            assert_eq!(request.form_field("grant_type").as_deref(), Some("urn:ietf:params:oauth:grant-type:device_code"));
            assert_eq!(request.form_field("client_id").as_deref(), Some(XAI_CLIENT_ID));
            assert_eq!(request.form_field("device_code").as_deref(), Some("device-code"));
            let (status, body) = replies.lock().remove(0);
            json_response(status, body)
        }
        other => anyhow::bail!("Unexpected request: {other}"),
    });
    let (login, recorder) = interaction(never_prompt(), None, None);
    let before = pi_ai::utils::now_ms();
    let credential = with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.login(login, None)).await.unwrap();
    assert_eq!(recorder.device_codes(), vec![AuthEvent::DeviceCode {
        user_code: "ABCD-1234".into(),
        verification_uri: "https://accounts.x.ai/oauth2/device".into(),
        interval_seconds: Some(5),
        expires_in_seconds: Some(900),
    }]);
    assert_eq!(*times.lock(), vec![5000, 10000, 20000]);
    assert_eq!((credential.access.as_str(), credential.refresh.as_str()), ("access-token", "refresh-token"));
    assert!(credential.expires >= before + 21_600_000 - 300_000 && credential.expires <= pi_ai::utils::now_ms() + 21_600_000 - 300_000);
}

#[tokio::test(start_paused = true)]
async fn xai_defaults_the_poll_interval_and_prefers_the_complete_uri() {
    let start = Instant::now();
    let times = Arc::new(Mutex::new(Vec::<u128>::new()));
    let sink = times.clone();
    let stub = stub_fetch(move |request, _| {
        if request.url == "https://auth.x.ai/oauth2/device/code" {
            return json_response(200, xai_device(json!({"interval": 0})));
        }
        sink.lock().push(elapsed_ms(start));
        json_response(200, xai_token(json!({})))
    });
    let (login, _) = interaction(never_prompt(), None, None);
    with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.login(login, None)).await.unwrap();
    // RFC 8628 default interval is 5 seconds when the server does not require a wait.
    assert_eq!(*times.lock(), vec![5000]);

    let stub = stub_fetch(|request, _| {
        if request.url == "https://auth.x.ai/oauth2/device/code" {
            return json_response(200, xai_device(json!({"verification_uri_complete": "https://accounts.x.ai/oauth2/device?user_code=ABCD-1234"})));
        }
        json_response(200, xai_token(json!({})))
    });
    let (login, recorder) = interaction(never_prompt(), None, None);
    with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.login(login, None)).await.unwrap();
    assert_eq!(recorder.device_codes(), vec![AuthEvent::DeviceCode {
        user_code: "ABCD-1234".into(),
        verification_uri: "https://accounts.x.ai/oauth2/device?user_code=ABCD-1234".into(),
        interval_seconds: Some(5),
        expires_in_seconds: Some(900),
    }]);
}

#[tokio::test(start_paused = true)]
async fn xai_rejects_untrusted_verification_uris_and_denials() {
    for overrides in [
        json!({"verification_uri_complete": "http://accounts.x.ai/oauth2/device?user_code=ABCD-1234"}),
        json!({"verification_uri": "http://accounts.x.ai/oauth2/device"}),
        json!({"verification_uri": "file:///etc/passwd"}),
        json!({"verification_uri": "not a url"}),
    ] {
        let device = xai_device(overrides.clone());
        let stub = stub_fetch(move |_, _| json_response(200, device.clone()));
        let (login, _) = interaction(never_prompt(), None, None);
        let error = with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.login(login, None)).await.unwrap_err();
        assert!(error.to_string().contains("Untrusted verification URI"), "{overrides}: {error}");
    }
    for denial in ["access_denied", "authorization_denied"] {
        let stub = stub_fetch(move |_, index| {
            if index == 0 { json_response(200, xai_device(json!({"interval": 1}))) } else { json_response(400, json!({"error": denial})) }
        });
        let (login, _) = interaction(never_prompt(), None, None);
        let error = with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.login(login, None)).await.unwrap_err();
        assert!(error.to_string().contains("xAI device authorization was denied"), "{denial}: {error}");
    }
}

#[tokio::test(start_paused = true)]
async fn xai_cancels_while_waiting_for_the_first_poll() {
    let signal = CancellationToken::new();
    let cancel = signal.clone();
    let notify: NotifyHook = Arc::new(move |event| {
        if matches!(event, AuthEvent::DeviceCode { .. }) {
            cancel.cancel();
        }
    });
    let stub = stub_fetch(|_, _| json_response(200, xai_device(json!({}))));
    let (login, _) = interaction(never_prompt(), Some(notify), Some(signal));
    let error = with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.login(login, None)).await.unwrap_err();
    assert_eq!(error.to_string(), "Login cancelled");
    assert_eq!(stub.requests.lock().len(), 1);
}

#[tokio::test]
async fn xai_refresh_rotation_lifetimes_and_validation() {
    let stub = stub_fetch(|request, index| {
        assert_eq!(request.url, "https://auth.x.ai/oauth2/token");
        assert_eq!(request.form_field("grant_type").as_deref(), Some("refresh_token"));
        assert_eq!(request.form_field("client_id").as_deref(), Some(XAI_CLIENT_ID));
        if index == 0 {
            assert_eq!(request.form_field("refresh_token").as_deref(), Some("old-refresh"));
            json_response(200, xai_token(json!({"access_token": "new-access", "refresh_token": "new-refresh"})))
        } else {
            assert_eq!(request.form_field("refresh_token").as_deref(), Some("keep-refresh"));
            json_response(200, xai_token(json!({"access_token": "newer-access", "refresh_token": null})))
        }
    });
    let rotated = with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.refresh(cred("old-access", "old-refresh", json!({})), CancellationToken::new())).await.unwrap();
    let preserved = with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.refresh(cred("old-access", "keep-refresh", json!({})), CancellationToken::new())).await.unwrap();
    assert_eq!((rotated.access.as_str(), rotated.refresh.as_str()), ("new-access", "new-refresh"));
    assert_eq!((preserved.access.as_str(), preserved.refresh.as_str()), ("newer-access", "keep-refresh"));
    assert_eq!(xai::XaiOAuth.name(), "xAI (Grok/X subscription)");
    assert_eq!(xai::XaiOAuth.to_auth(&preserved).await.unwrap().api_key.as_deref(), Some("newer-access"));

    let stub = stub_fetch(|_, _| json_response(200, xai_token(json!({"expires_in": null}))));
    let before = pi_ai::utils::now_ms();
    let refreshed = with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.refresh(cred("a", "old-refresh", json!({})), CancellationToken::new())).await.unwrap();
    assert!(refreshed.expires >= before + 3_600_000 - 300_000 && refreshed.expires <= pi_ai::utils::now_ms() + 3_600_000 - 300_000);

    let stub = stub_fetch(|_, _| json_response(200, xai_token(json!({"access_token": null}))));
    let error = with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.refresh(cred("a", "old-refresh", json!({})), CancellationToken::new())).await.unwrap_err();
    assert!(error.to_string().contains("Invalid xAI OAuth response field: access_token"), "{error}");
}

// ---------------------------------------------------------------------------
// Port of openrouter-oauth.test.ts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openrouter_oauth_is_exposed_and_shared_by_chat_and_image_models() {
    let provider = openrouter_provider();
    assert!(provider.auth().api_key.is_some());
    assert_eq!(provider.auth().oauth.as_ref().unwrap().login_label(), Some("Sign in with OpenRouter"));

    let store = Arc::new(InMemoryCredentialStore::new());
    let stored = OAuthCredential { access: "sk-or-stored".into(), refresh: String::new(), expires: 9_007_199_254_740_991, extra: Map::new() };
    store.modify("openrouter", Box::new(move |_| Box::pin(async move { Ok(Some(Credential::OAuth(stored))) })), None).await.unwrap();
    let models = create_models(CreateModelsOptions { credentials: Some(store), ..Default::default() });
    models.set_provider(provider);
    let chat = models.get_all_models(Some("openrouter")).into_iter().find(|m| m.model_type() == ModelType::Chat).unwrap();
    let image = models.get_all_models(Some("openrouter")).into_iter().find(|m| m.model_type() == ModelType::Image).expect("an OpenRouter image model");
    for model in [chat, image] {
        let auth = models.get_auth_for_model(&model, Default::default()).await.unwrap().unwrap();
        assert_eq!(auth.auth.api_key.as_deref(), Some("sk-or-stored"), "{}", model.id());
    }
}

#[tokio::test]
async fn openrouter_allows_only_one_token_exchange_per_callback() {
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let release = Arc::new(Mutex::new(Some(release_rx)));
    let exchanges = Arc::new(Mutex::new(0usize));
    let counter = exchanges.clone();
    let fetch: OAuthFetch = Arc::new(move |_request| {
        *counter.lock() += 1;
        let release = release.lock().take();
        Box::pin(async move {
            if let Some(release) = release {
                let _ = release.await;
            }
            Ok(OAuthHttpResponse::json_response(200, &json!({"key": "sk-or-test"})))
        })
    });
    let callback = Arc::new(Mutex::new(None::<String>));
    let first = Arc::new(Mutex::new(None::<tokio::task::JoinHandle<(u16, String)>>));
    let (callback_sink, first_sink) = (callback.clone(), first.clone());
    let notify: NotifyHook = Arc::new(move |event| {
        if let AuthEvent::AuthUrl { url, .. } = event {
            let mut target = url::Url::parse(&query_param(url, "callback_url").unwrap()).unwrap();
            target.query_pairs_mut().append_pair("code", "authorization-code");
            let target = target.to_string();
            *callback_sink.lock() = Some(target.clone());
            *first_sink.lock() = Some(tokio::spawn(async move { http_get(&target).await }));
        }
    });
    let (login, _) = interaction(pending_after_select("unused"), Some(notify), None);
    let login_task = tokio::spawn(with_oauth_fetch(fetch, async move { openrouter::OpenRouterOAuth.login(login, None).await }));
    while *exchanges.lock() == 0 {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let target = callback.lock().clone().unwrap();
    assert_eq!(http_get(&target).await.0, 409);
    assert_eq!(*exchanges.lock(), 1);
    release_tx.send(()).unwrap();
    assert_eq!(login_task.await.unwrap().unwrap().access, "sk-or-test");
    let first = first.lock().take().unwrap();
    assert_eq!(first.await.unwrap().0, 200);
}

#[tokio::test]
async fn openrouter_manual_input_and_cancellation() {
    let stub = stub_fetch(|_, _| json_response(200, json!({"key": "sk-or-unexpected"})));
    let (login, _) = interaction(answering("unused", |_, _| Ok("   ".into())), None, None);
    let error = with_oauth_fetch(stub.fetch.clone(), openrouter::OpenRouterOAuth.login(login, None)).await.unwrap_err();
    assert!(error.to_string().contains("Missing authorization code"), "{error}");
    assert!(stub.requests.lock().is_empty());

    let stub = stub_fetch(|_, _| json_response(200, json!({"key": "sk-or-manual"})));
    let (login, _) = interaction(answering("unused", |_, _| Ok("  manual-code  ".into())), None, None);
    let credential = with_oauth_fetch(stub.fetch.clone(), openrouter::OpenRouterOAuth.login(login, None)).await.unwrap();
    assert_eq!(credential.access, "sk-or-manual");
    assert_eq!(stub.requests.lock()[0].json_body().unwrap()["code"], "manual-code");

    // Cancelling closes the pending callback server.
    let signal = CancellationToken::new();
    let cancel = signal.clone();
    let callback = Arc::new(Mutex::new(None::<String>));
    let sink = callback.clone();
    let notify: NotifyHook = Arc::new(move |event| {
        if let AuthEvent::AuthUrl { url, .. } = event {
            *sink.lock() = query_param(url, "callback_url");
            cancel.cancel();
        }
    });
    let (login, _) = interaction(pending_after_select("unused"), Some(notify), Some(signal));
    let error = openrouter::OpenRouterOAuth.login(login, None).await.unwrap_err();
    assert_eq!(error.to_string(), "Login cancelled");
    let target = callback.lock().clone().unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    assert!(client.get(&target).send().await.is_err());

    // An already-cancelled login emits nothing.
    let signal = CancellationToken::new();
    signal.cancel();
    let notify: NotifyHook = Arc::new(|_| panic!("Cancelled login must not emit events"));
    let (login, _) = interaction(answering("unused", |_, _| Ok(String::new())), Some(notify), Some(signal));
    assert_eq!(openrouter::OpenRouterOAuth.login(login, None).await.unwrap_err().to_string(), "Login cancelled");
}

#[tokio::test]
async fn openrouter_uses_the_configured_callback_host() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::set(&[("PI_OAUTH_CALLBACK_HOST", Some("localhost"))]);
    let signal = CancellationToken::new();
    let cancel = signal.clone();
    let callback = Arc::new(Mutex::new(None::<String>));
    let sink = callback.clone();
    let notify: NotifyHook = Arc::new(move |event| {
        if let AuthEvent::AuthUrl { url, .. } = event {
            *sink.lock() = query_param(url, "callback_url");
            cancel.cancel();
        }
    });
    let (login, _) = interaction(pending_after_select("unused"), Some(notify), Some(signal));
    assert_eq!(openrouter::OpenRouterOAuth.login(login, None).await.unwrap_err().to_string(), "Login cancelled");
    let target = url::Url::parse(&callback.lock().clone().unwrap()).unwrap();
    assert_eq!(target.host_str(), Some("localhost"));
}
