//! Live (credentialed) ports of the E2E files in `packages/ai/test`.
//!
//! Every test here is `#[ignore]`d and runs the same provider/model matrix as its TS file. A
//! target runs only when the same credentials gate its TS `describe.skipIf` (environment variables,
//! or the `~/.pi/agent/auth.json` entry the TS `resolveApiKey` helper reads); the other targets
//! are reported as skipped. Run them with, for example:
//!
//! ```sh
//! ANTHROPIC_API_KEY=... cargo test -p pi-ai --test ts_ported live:: -- --ignored --nocapture
//! ```

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;

use futures::FutureExt;
use pi_ai::providers::all::{builtin_providers, get_builtin_model};
use pi_ai::*;
use serde_json::{Map, Value, json};

// mod basic;
// mod cache;
// mod handoff;
mod matrix;
// mod overflow;
// mod providers;
// mod replay;
// mod tool_results;
// mod unicode;

/// Credentials that gate a target, mirroring the TS `skipIf` conditions.
#[derive(Clone, Debug)]
pub enum Gate {
    /// Every variable is set and non-empty.
    Env(&'static [&'static str]),
    /// `hasAzureOpenAICredentials()`.
    Azure,
    /// `hasBedrockCredentials()`.
    Bedrock,
    /// `hasCloudflareWorkersAICredentials()`.
    CloudflareWorkers,
    /// `hasCloudflareAiGatewayCredentials()`.
    CloudflareGateway,
    /// `resolveApiKey(provider)` from `~/.pi/agent/auth.json`; the token becomes the api key.
    OAuth(&'static str),
    /// `which ollama` succeeds (and `PI_NO_LOCAL_LLM` is unset); `ollama serve` is started if needed.
    Ollama,
    /// An LM Studio server answers on localhost:1234 (and `PI_NO_LOCAL_LLM` is unset).
    LmStudio,
    /// A llama.cpp server answers on localhost:8081 with a completions endpoint.
    LlamaCpp,
    /// Never skipped (the TS block has no gate).
    None,
}

#[derive(Clone, Debug)]
pub struct Target {
    pub label: String,
    pub gate: Gate,
    pub provider: &'static str,
    pub model: &'static str,
    pub extra: Map<String, Value>,
    pub api: Option<&'static str>,
    pub strip_compat: bool,
    /// A model literal used instead of the catalog entry.
    pub custom_model: Option<Value>,
    pub api_key: Option<String>,
}

pub fn target(label: &str, gate: Gate, provider: &'static str, model: &'static str) -> Target {
    Target {
        label: label.to_string(),
        gate,
        provider,
        model,
        extra: Map::new(),
        api: None,
        strip_compat: false,
        custom_model: None,
        api_key: None,
    }
}

impl Target {
    pub fn extra(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.to_string(), value);
        self
    }

    /// `{ ...getModel(...), api }` without `compat` (the TS `{ compat: _, ...base }` spread).
    pub fn as_api(mut self, api: &'static str) -> Self {
        self.api = Some(api);
        self.strip_compat = true;
        self
    }

    /// `{ ...getModel(...), api }` keeping `compat`.
    pub fn with_api(mut self, api: &'static str) -> Self {
        self.api = Some(api);
        self
    }

    pub fn custom(mut self, model: Value) -> Self {
        self.custom_model = Some(model);
        self
    }

    pub fn api_key(mut self, key: &str) -> Self {
        self.api_key = Some(key.to_string());
        self
    }
}

pub fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn auth_path() -> std::path::PathBuf {
    dirs_home().join(".pi").join("agent").join("auth.json")
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default()
}

/// Port of `test/oauth.ts` `resolveApiKey`: api keys verbatim, OAuth access tokens refreshed when
/// expired (and written back), then converted with the provider's `toAuth`.
pub async fn resolve_api_key(provider: &str) -> Option<String> {
    let path = auth_path();
    let mut storage: Map<String, Value> = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok())?;
    let entry = storage.get(provider)?.clone();
    match entry["type"].as_str()? {
        "api_key" => entry["key"].as_str().map(str::to_string),
        "oauth" => {
            let oauth = builtin_providers().into_iter().find(|p| p.id() == provider)?.auth().oauth.clone()?;
            let mut credential: pi_ai::auth::OAuthCredential = serde_json::from_value(entry).ok()?;
            if chrono_now_ms() >= credential.expires {
                credential = match oauth.refresh(credential, Default::default()).await {
                    Ok(credential) => credential,
                    Err(error) => {
                        println!("{error:#}");
                        return None;
                    }
                };
            }
            let mut stored = serde_json::to_value(&credential).ok()?;
            stored["type"] = json!("oauth");
            storage.insert(provider.to_string(), stored);
            let _ = std::fs::write(&path, serde_json::to_string_pretty(&storage).unwrap_or_default());
            oauth.to_auth(&credential).await.ok()?.api_key
        }
        _ => None,
    }
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default()
}

/// `resolveAzureDeploymentName(modelId)` from `AZURE_OPENAI_DEPLOYMENT_NAME_MAP`.
pub fn azure_deployment_name(model_id: &str) -> Option<String> {
    env("AZURE_OPENAI_DEPLOYMENT_NAME_MAP")?.split(',').find_map(|entry| {
        let (id, name) = entry.trim().split_once('=')?;
        (id.trim() == model_id && !name.trim().is_empty()).then(|| name.trim().to_string())
    })
}

pub fn has_bedrock_credentials() -> bool {
    env("AWS_PROFILE").is_some()
        || (env("AWS_ACCESS_KEY_ID").is_some() && env("AWS_SECRET_ACCESS_KEY").is_some())
        || env("AWS_BEARER_TOKEN_BEDROCK").is_some()
}

/// Resolve the gate; `None` means the TS test is skipped.
pub async fn open_gate(gate: &Gate) -> Option<Option<String>> {
    let ok = |b: bool| b.then_some(None);
    match gate {
        Gate::Env(names) => ok(names.iter().all(|n| env(n).is_some())),
        Gate::Azure => ok(env("AZURE_OPENAI_API_KEY").is_some()
            && (env("AZURE_OPENAI_BASE_URL").is_some() || env("AZURE_OPENAI_RESOURCE_NAME").is_some())),
        Gate::Bedrock => ok(has_bedrock_credentials()),
        Gate::CloudflareWorkers => ok(env("CLOUDFLARE_API_KEY").is_some() && env("CLOUDFLARE_ACCOUNT_ID").is_some()),
        Gate::CloudflareGateway => ok(env("CLOUDFLARE_API_KEY").is_some()
            && env("CLOUDFLARE_ACCOUNT_ID").is_some()
            && env("CLOUDFLARE_GATEWAY_ID").is_some()),
        Gate::OAuth(provider) => resolve_api_key(provider).await.map(Some),
        Gate::Ollama => ok(local_llm_allowed() && ollama_ready().await),
        Gate::LmStudio => ok(local_llm_allowed() && http_status("http://localhost:1234/v1/models", None).await.is_some()),
        Gate::LlamaCpp => ok(local_llm_allowed()
            && http_status("http://localhost:8081/health", None).await.is_some()
            && http_status("http://localhost:8081/v1/completions", Some(json!({"model": "local-model", "prompt": "ping", "max_tokens": 1})))
                .await
                .is_some_and(|status| status != 404 && status != 405)),
        Gate::None => Some(None),
    }
}

fn local_llm_allowed() -> bool {
    env("PI_NO_LOCAL_LLM").is_none()
}

/// The status of a GET (or a JSON POST) with a one-second timeout; `None` when unreachable.
async fn http_status(url: &str, post: Option<Value>) -> Option<u16> {
    let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(1)).build().ok()?;
    let request = match post {
        Some(body) => client.post(url).json(&body),
        None => client.get(url),
    };
    request.send().await.ok().map(|r| r.status().as_u16())
}

/// The TS `beforeAll`: pull `gpt-oss:20b` when missing, start `ollama serve`, wait for the API.
async fn ollama_ready() -> bool {
    use std::process::{Command, Stdio};
    if !Command::new("sh").args(["-c", "which ollama"]).stdout(Stdio::null()).status().is_ok_and(|s| s.success()) {
        return false;
    }
    let listed = Command::new("sh").args(["-c", "ollama list | grep -q 'gpt-oss:20b'"]).status().is_ok_and(|s| s.success());
    if !listed && !Command::new("ollama").args(["pull", "gpt-oss:20b"]).status().is_ok_and(|s| s.success()) {
        println!("Failed to pull gpt-oss:20b model, tests will be skipped");
        return false;
    }
    if http_status("http://localhost:11434/api/tags", None).await.is_none() {
        // The server keeps running for the rest of the test process, like the TS child process
        // until `afterAll`.
        let _ = Command::new("ollama").arg("serve").stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    for _ in 0..120 {
        if http_status("http://localhost:11434/api/tags", None).await.is_some_and(|s| s < 300) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    false
}

pub fn catalog_model(provider: &str, id: &str) -> Model {
    get_builtin_model(provider, id).unwrap_or_else(|| panic!("{provider}/{id} is not in the catalog"))
}

/// The target's model and options, or `None` when its credentials are missing.
pub async fn resolve(target: &Target) -> Option<(Model, StreamOptions)> {
    let api_key = open_gate(&target.gate).await?;
    let mut model = match &target.custom_model {
        Some(value) => crate::support::model(value.clone()),
        None => catalog_model(target.provider, target.model),
    };
    if let Some(api) = target.api {
        model.api = api.to_string();
    }
    if target.strip_compat {
        model.compat = None;
    }
    let api_key = target.api_key.clone().or(api_key);
    let mut options = StreamOptions { api_key, extra: target.extra.clone(), ..Default::default() };
    if matches!(target.gate, Gate::Azure)
        && let Some(name) = azure_deployment_name(&model.id)
    {
        options.extra.insert("azureDeploymentName".into(), json!(name));
    }
    Some((model, options))
}

pub type ScenarioFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
pub type Scenario = for<'a> fn(&'a Model, StreamOptions) -> ScenarioFuture<'a>;

/// Run one scenario with vitest-style retries; returns the last panic message on failure.
pub async fn attempt(retries: usize, run: impl Fn() -> ScenarioFuture<'static>) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..=retries {
        match AssertUnwindSafe(run()).catch_unwind().await {
            Ok(()) => return Ok(()),
            Err(panic) => {
                last = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into());
            }
        }
    }
    Err(last)
}

/// Run every scenario against every target whose credentials are present; fail with all failures.
pub async fn run_matrix(file: &str, targets: Vec<Target>, scenarios: &[(&str, Scenario)], retries: usize) {
    let mut failures = Vec::new();
    let mut ran = 0;
    for target in &targets {
        let Some((model, options)) = resolve(target).await else {
            println!("[{file}] skip {} ({:?} not configured)", target.label, target.gate);
            continue;
        };
        for (name, scenario) in scenarios {
            ran += 1;
            let model = model.clone();
            let options = options.clone();
            let scenario = *scenario;
            let result = attempt(retries, move || {
                let model = model.clone();
                let options = options.clone();
                Box::pin(async move { scenario(&model, options).await })
            })
            .await;
            match result {
                Ok(()) => println!("[{file}] ok   {} :: {name}", target.label),
                Err(error) => {
                    println!("[{file}] FAIL {} :: {name}: {error}", target.label);
                    failures.push(format!("{} :: {name}: {error}", target.label));
                }
            }
        }
    }
    println!("[{file}] {ran} case(s) ran, {} failed", failures.len());
    assert!(failures.is_empty(), "{file}: {} failure(s):\n{}", failures.len(), failures.join("\n"));
}

// ---------------------------------------------------------------------------
// Shared message helpers
// ---------------------------------------------------------------------------

pub fn now() -> i64 {
    chrono_now_ms()
}

pub fn user(text: &str) -> Message {
    Message::User(UserMessage::new(vec![UserContentBlock::text(text)], now()))
}

pub fn user_string(text: &str) -> Message {
    serde_json::from_value(json!({"role": "user", "content": text, "timestamp": now()})).unwrap()
}

pub fn text_of(message: &AssistantMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|b| match b {
            AssistantContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn has_tool_call(message: &AssistantMessage) -> bool {
    message.content.iter().any(|b| matches!(b, AssistantContent::ToolCall(_)))
}

pub fn has_thinking(message: &AssistantMessage) -> bool {
    message.content.iter().any(|b| matches!(b, AssistantContent::Thinking(_)))
}

pub fn tool_json(name: &str, description: &str, parameters: Value) -> Tool {
    serde_json::from_value(json!({"name": name, "description": description, "parameters": parameters})).unwrap()
}

pub fn context(system_prompt: Option<&str>, messages: Vec<Message>, tools: Vec<Tool>) -> Context {
    Context { system_prompt: system_prompt.map(str::to_string), messages, tools: (!tools.is_empty()).then_some(tools) }
}

#[track_caller]
pub fn expect_not_error(message: &AssistantMessage) {
    assert_ne!(message.stop_reason, StopReason::Error, "{:?}", message.error_message);
}
