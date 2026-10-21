//! Helpers for the OAuth flow tests: a scripted login interaction and a stubbed transport.
#![allow(dead_code)]

use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use parking_lot::Mutex;
use pi_ai::auth::oauth::{OAuthFetch, OAuthHttpRequest, OAuthHttpResponse};
use pi_ai::auth::{AuthEvent, AuthInteraction, AuthPrompt, AuthPromptKind, ProviderAuthInteraction};
use tokio_util::sync::CancellationToken;

pub type PromptHandler = Arc<dyn Fn(AuthPrompt, &Recorder) -> BoxFuture<'static, anyhow::Result<String>> + Send + Sync>;
pub type NotifyHook = Arc<dyn Fn(&AuthEvent) + Send + Sync>;

/// Records prompts and events.
#[derive(Default)]
pub struct Recorder {
    pub prompts: Mutex<Vec<AuthPrompt>>,
    pub events: Mutex<Vec<AuthEvent>>,
}

impl Recorder {
    pub fn auth_url(&self) -> Option<String> {
        self.events.lock().iter().find_map(|e| match e {
            AuthEvent::AuthUrl { url, .. } => Some(url.clone()),
            _ => None,
        })
    }
    pub fn device_codes(&self) -> Vec<AuthEvent> {
        self.events.lock().iter().filter(|e| matches!(e, AuthEvent::DeviceCode { .. })).cloned().collect()
    }
    pub fn manual_signal(&self) -> Option<CancellationToken> {
        self.prompts.lock().iter().find(|p| p.kind == AuthPromptKind::ManualCode).and_then(|p| p.signal.clone())
    }
}

pub struct TestInteraction {
    pub recorder: Arc<Recorder>,
    handler: PromptHandler,
    notify_hook: Option<NotifyHook>,
}

#[async_trait]
impl AuthInteraction for TestInteraction {
    async fn prompt(&self, prompt: AuthPrompt) -> anyhow::Result<String> {
        self.recorder.prompts.lock().push(prompt.clone());
        (self.handler)(prompt, &self.recorder).await
    }
    fn notify(&self, event: AuthEvent) {
        self.recorder.events.lock().push(event.clone());
        if let Some(hook) = &self.notify_hook {
            hook(&event);
        }
    }
}

pub fn interaction(handler: PromptHandler, notify: Option<NotifyHook>, signal: Option<CancellationToken>) -> (ProviderAuthInteraction, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let interaction = TestInteraction { recorder: recorder.clone(), handler, notify_hook: notify };
    (ProviderAuthInteraction { interaction: Arc::new(interaction), signal: signal.unwrap_or_default() }, recorder)
}

/// Answer select prompts with `select` and other prompts with `answer(prompt, recorder)`.
pub fn answering(
    select: &'static str,
    answer: impl Fn(&AuthPrompt, &Recorder) -> anyhow::Result<String> + Send + Sync + 'static,
) -> PromptHandler {
    Arc::new(move |prompt, recorder| {
        let result = if prompt.kind == AuthPromptKind::Select { Ok(select.to_string()) } else { answer(&prompt, recorder) };
        Box::pin(async move { result })
    })
}

/// Select prompts get `select`; other prompts stay open until their signal aborts.
pub fn pending_after_select(select: &'static str) -> PromptHandler {
    Arc::new(move |prompt, _| {
        Box::pin(async move {
            if prompt.kind == AuthPromptKind::Select {
                return Ok(select.to_string());
            }
            match prompt.signal {
                Some(signal) => {
                    signal.cancelled().await;
                    anyhow::bail!("prompt aborted")
                }
                None => std::future::pending().await,
            }
        })
    })
}

/// A stubbed transport that records requests.
pub struct FetchStub {
    pub requests: Arc<Mutex<Vec<OAuthHttpRequest>>>,
    pub fetch: OAuthFetch,
}

pub fn stub_fetch(handler: impl Fn(&OAuthHttpRequest, usize) -> anyhow::Result<OAuthHttpResponse> + Send + Sync + 'static) -> FetchStub {
    let requests: Arc<Mutex<Vec<OAuthHttpRequest>>> = Arc::default();
    let sink = requests.clone();
    let handler = Arc::new(handler);
    let fetch: OAuthFetch = Arc::new(move |request: OAuthHttpRequest| {
        let index = {
            let mut list = sink.lock();
            list.push(request.clone());
            list.len() - 1
        };
        let result = handler(&request, index);
        Box::pin(async move { result })
    });
    FetchStub { requests, fetch }
}

pub fn json_response(status: u16, body: serde_json::Value) -> anyhow::Result<OAuthHttpResponse> {
    Ok(OAuthHttpResponse::json_response(status, &body))
}

/// Plain HTTP GET that bypasses proxies (for loopback callback servers).
pub async fn http_get(url: &str) -> (u16, String) {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client.get(url).send().await.unwrap();
    let status = response.status().as_u16();
    (status, response.text().await.unwrap_or_default())
}

pub fn query_param(url: &str, name: &str) -> Option<String> {
    url::Url::parse(url).ok()?.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned())
}

/// A JWT-shaped token whose payload carries a ChatGPT account id.
pub fn access_token_for(account: &str) -> String {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    let header = engine.encode(r#"{"alg":"none"}"#);
    let payload =
        engine.encode(serde_json::json!({ "https://api.openai.com/auth": { "chatgpt_account_id": account } }).to_string());
    format!("{header}.{payload}.signature")
}

/// Serializes tests that bind fixed callback ports.
pub static PORT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
