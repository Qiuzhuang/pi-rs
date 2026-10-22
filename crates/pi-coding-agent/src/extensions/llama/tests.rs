//! Port of `test/llama-extension.test.ts`.

use super::client::*;
use super::huggingface::*;
use super::provider::*;
use crate::core::event_bus::create_event_bus;
use crate::core::extensions::loader::{create_extension_runtime, load_extension_from_factory};
use crate::core::extensions::types::{ExtensionAPIRef, ExtensionFactory};
use async_trait::async_trait;
use parking_lot::Mutex;
use pi_ai::auth::types::{ApiKeyCredential, ApiKeyResolveInput, AuthContext, AuthEvent, AuthInteraction, AuthPrompt, Credential, ProviderAuthInteraction};
use pi_ai::models::{ModelsPublication, PublishFn, RefreshModelsContext};
use pi_ai::models_store::ModelsStoreEntry;
use pi_ai::types::{AnyModel, ClassifierAnswer, ClassifierContext, StreamOptions};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// A tiny HTTP server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Request {
    method: String,
    /// Path with query, like `request.url` in Node.
    url: String,
    headers: HashMap<String, String>,
    body: String,
}

impl Request {
    fn path(&self) -> &str {
        self.url.split('?').next().unwrap_or("")
    }

    fn query(&self, name: &str) -> Option<String> {
        let query = self.url.split_once('?')?.1;
        url::form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == name).map(|(_, v)| v.into_owned())
    }
}

enum Reply {
    Json(Value),
    NotFound,
    Sse(mpsc::UnboundedReceiver<String>),
}

type Handler = Arc<dyn Fn(Request) -> Reply + Send + Sync>;

async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_string();
    let url = first.next()?.to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .collect();
    let length: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = buffer[header_end..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some(Request { method, url, headers, body: String::from_utf8_lossy(&body).to_string() })
}

async fn listen(handler: Handler) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { break };
            let handler = handler.clone();
            tokio::spawn(async move {
                let Some(request) = read_request(&mut stream).await else { return };
                match handler(request) {
                    Reply::Json(value) => {
                        let body = serde_json::to_vec(&value).unwrap();
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(head.as_bytes()).await;
                        let _ = stream.write_all(&body).await;
                    }
                    Reply::NotFound => {
                        let _ = stream
                            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                    }
                    Reply::Sse(mut events) => {
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                            .await;
                        while let Some(event) = events.recv().await {
                            if stream.write_all(event.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                let _ = stream.shutdown().await;
            });
        }
    });
    url
}

fn handler(f: impl Fn(Request) -> Reply + Send + Sync + 'static) -> Handler {
    Arc::new(f)
}

/// SSE subscribers of a test server.
#[derive(Clone, Default)]
struct Streams(Arc<Mutex<Vec<mpsc::UnboundedSender<String>>>>);

impl Streams {
    fn subscribe(&self) -> Reply {
        let (tx, rx) = mpsc::unbounded_channel();
        self.0.lock().push(tx);
        Reply::Sse(rx)
    }

    fn send(&self, event: Value) {
        for stream in self.0.lock().iter() {
            let _ = stream.send(format!("data: {event}\n\n"));
        }
    }
}

// ---------------------------------------------------------------------------
// Refresh helpers
// ---------------------------------------------------------------------------

type Cache = Arc<Mutex<Option<ModelsStoreEntry>>>;

fn caching_publish(cache: Cache) -> PublishFn {
    Arc::new(move |publication: ModelsPublication| {
        match publication.persist {
            Some(None) => *cache.lock() = None,
            Some(Some(entry)) => *cache.lock() = Some(entry),
            None => {}
        }
        if let Some(update) = publication.update {
            update();
        }
        Box::pin(async { Ok(true) })
    })
}

fn refresh_context(url: &str, stored: Option<ModelsStoreEntry>, publish: PublishFn, allow_network: bool) -> RefreshModelsContext {
    RefreshModelsContext {
        credential: Some(Credential::ApiKey(ApiKeyCredential {
            key: Some("local".into()),
            env: Some(HashMap::from([("LLAMA_BASE_URL".to_string(), url.to_string())])),
        })),
        stored,
        publish,
        allow_network,
        force: None,
        signal: CancellationToken::new(),
        base_url: None,
    }
}

fn model_info(value: Value) -> LlamaModelInfo {
    serde_json::from_value(value).unwrap()
}

fn chat_ids(controller: &Arc<dyn LlamaProviderController>) -> Vec<String> {
    controller.provider().get_models().into_iter().map(|m| m.id).collect()
}

fn classifier_apis(controller: &Arc<dyn LlamaProviderController>) -> Vec<(String, String)> {
    controller
        .provider()
        .get_all_models()
        .into_iter()
        .filter_map(|m| match m {
            AnyModel::Classifier(c) => Some((c.id, c.api)),
            _ => None,
        })
        .collect()
}

fn cached_models(cache: &Cache) -> Vec<(String, String)> {
    cache
        .lock()
        .as_ref()
        .map(|entry| entry.models.iter().map(|m| (m.id().to_string(), api_of(m))).collect())
        .unwrap_or_default()
}

fn api_of(model: &AnyModel) -> String {
    match model {
        AnyModel::Chat(m) => m.api.clone(),
        AnyModel::Classifier(m) => m.api.clone(),
        AnyModel::Image(m) => m.api.clone(),
    }
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn registers_a_native_provider_and_llama_command() {
    let runtime = create_extension_runtime();
    let factory: ExtensionFactory =
        Arc::new(|pi: ExtensionAPIRef| Box::pin(async move { super::llama_extension(pi) }));
    let extension = load_extension_from_factory(
        &factory,
        &std::env::current_dir().unwrap().to_string_lossy(),
        create_event_bus(),
        runtime.clone(),
        Some("builtin:llama.cpp"),
    )
    .await
    .unwrap();
    let description = extension.read().commands.get("llama").and_then(|c| c.description.clone());
    assert_eq!(description.as_deref(), Some("Manage llama.cpp router models"));
    let ids: Vec<String> =
        runtime.pending_native_provider_registrations.lock().iter().map(|e| e.provider.id().to_string()).collect();
    assert_eq!(ids, vec![LLAMA_PROVIDER_ID]);
}

#[test]
fn normalizes_management_and_inference_urls() {
    assert_eq!(normalize_llama_server_url("http://127.0.0.1:8080/v1/").unwrap(), "http://127.0.0.1:8080");
    assert_eq!(normalize_llama_server_url("https://example.com/prefix/v1").unwrap(), "https://example.com/prefix");
    assert!(normalize_llama_server_url("file:///tmp/llama").unwrap_err().to_string().contains("http or https"));
    assert_eq!(llama_inference_url("http://localhost:8080"), "http://localhost:8080/v1");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1024), "1.00 KiB");
    assert_eq!(format_bytes(15 * 1024 * 1024), "15.0 MiB");
}

#[test]
fn exposes_loaded_and_sleeping_models_with_router_metadata() {
    let controller = create_llama_provider();
    controller.set_catalog(
        &[
            model_info(json!({
                "id": "loaded",
                "status": { "value": "loaded", "args": ["llama-server", "--n-gpu-layers", "999"] },
                "architecture": { "input_modalities": ["text", "image"] },
                "meta": { "n_ctx": 65536, "n_ctx_train": 131072 }
            })),
            model_info(json!({ "id": "sleeping", "status": { "value": "sleeping" } })),
            model_info(json!({ "id": "unloaded", "status": { "value": "unloaded" } })),
            model_info(json!({ "id": "loading", "status": { "value": "loading" } })),
        ],
        "http://localhost:8080",
        None,
    );
    let models = controller.provider().get_models();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "loaded");
    assert_eq!(models[0].base_url, "http://localhost:8080/v1");
    assert_eq!(models[0].context_window, 65536);
    assert_eq!(models[0].max_tokens, 65536);
    assert_eq!(serde_json::to_value(&models[0].input).unwrap(), json!(["text", "image"]));
    assert_eq!(models[1].id, "sleeping");
    assert_eq!(models[1].base_url, "http://localhost:8080/v1");
}

// Regression test for #9528.
#[tokio::test]
async fn discovers_chat_template_thinking_support_for_loaded_models() {
    let props_requests = Arc::new(Mutex::new(Vec::<(Option<String>, Option<String>)>::new()));
    let seen = props_requests.clone();
    let url = listen(handler(move |request| {
        if request.url == "/models" {
            return Reply::Json(json!({ "data": [{ "id": "qwen", "status": { "value": "loaded" }, "meta": { "n_ctx": 32768 } }] }));
        }
        if request.path() == "/props" {
            seen.lock().push((request.query("model"), request.query("autoload")));
            return Reply::Json(json!({ "chat_template": "{% if enable_thinking %}think{% endif %}" }));
        }
        Reply::NotFound
    }))
    .await;
    let controller = create_llama_provider();
    let cache: Cache = Arc::default();
    controller.provider().refresh_models(refresh_context(&url, None, caching_publish(cache), true)).await.unwrap();
    assert_eq!(*props_requests.lock(), vec![(Some("qwen".to_string()), Some("false".to_string()))]);
    let models = controller.provider().get_models();
    assert_eq!(models.len(), 1);
    assert!(models[0].reasoning);
    assert_eq!(
        serde_json::to_value(&models[0].thinking_level_map).unwrap(),
        json!({ "off": "off", "minimal": null, "low": null, "medium": "medium", "high": null, "xhigh": null })
    );
    assert_eq!(serde_json::to_value(&models[0].compat).unwrap()["thinkingFormat"], json!("qwen-chat-template"));
}

#[tokio::test]
async fn persists_and_restores_selectable_models_for_cache_only_startup_refreshes() {
    let url = listen(handler(|request| {
        if request.url == "/models" {
            return Reply::Json(json!({ "data": [
                { "id": "loaded", "status": { "value": "loaded" }, "meta": { "n_ctx": 32768 } },
                { "id": "sleeping", "status": { "value": "sleeping" }, "meta": { "n_ctx": 32768 } },
                { "id": "unloaded", "status": { "value": "unloaded" } }
            ] }));
        }
        if request.url == "/props?model=loaded&autoload=false" {
            return Reply::Json(json!({}));
        }
        Reply::NotFound
    }))
    .await;
    let cache: Cache = Arc::default();
    let first = create_llama_provider();
    first.provider().refresh_models(refresh_context(&url, None, caching_publish(cache.clone()), true)).await.unwrap();
    assert_eq!(chat_ids(&first), vec!["loaded", "sleeping"]);
    assert_eq!(
        cached_models(&cache),
        pairs(&[
            ("loaded", "openai-completions"),
            ("sleeping", "openai-completions"),
            ("loaded", "llama-cpp-classify"),
            ("sleeping", "llama-cpp-classify"),
        ])
    );

    let second = create_llama_provider();
    let stored = cache.lock().clone();
    second.provider().refresh_models(refresh_context(&url, stored, caching_publish(cache.clone()), false)).await.unwrap();
    let models = second.provider().get_models();
    assert_eq!(models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["loaded", "sleeping"]);
    for model in &models {
        assert_eq!(model.base_url, format!("{url}/v1"));
        assert_eq!(model.context_window, 32768);
    }
    let classifiers: Vec<_> = second
        .provider()
        .get_all_models()
        .into_iter()
        .filter_map(|m| match m {
            AnyModel::Classifier(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(classifiers.len(), 2);
    for classifier in &classifiers {
        assert_eq!(classifier.api, "llama-cpp-classify");
        assert_eq!(classifier.base_url, url);
        assert_eq!(classifier.context_window, 32768);
    }
}

#[tokio::test]
async fn preserves_cached_context_for_unloaded_autoload_presets() {
    let loaded = Arc::new(Mutex::new(true));
    let unloaded_args: Arc<Mutex<Option<Vec<String>>>> = Arc::default();
    let (state, args) = (loaded.clone(), unloaded_args.clone());
    let url = listen(handler(move |request| {
        if request.url == "/models" {
            let entry = if *state.lock() {
                json!({ "id": "qwen", "status": { "value": "loaded" }, "source": "preset", "meta": { "n_ctx": 65536, "n_ctx_train": 128000 } })
            } else {
                let mut status = json!({ "value": "unloaded" });
                if let Some(args) = args.lock().clone() {
                    status["args"] = json!(args);
                }
                json!({ "id": "qwen", "status": status, "source": "preset", "meta": { "n_ctx_train": 128000 } })
            };
            return Reply::Json(json!({ "data": [entry] }));
        }
        if request.url == "/props?model=qwen&autoload=false" {
            return Reply::Json(json!({}));
        }
        if request.url == "/props" {
            return Reply::Json(json!({ "role": "router", "models_autoload": true }));
        }
        Reply::NotFound
    }))
    .await;
    let cache: Cache = Arc::default();
    let windows = |cache: &Cache| -> Vec<u64> {
        cache
            .lock()
            .as_ref()
            .map(|e| {
                e.models
                    .iter()
                    .map(|m| match m {
                        AnyModel::Chat(m) => m.context_window,
                        AnyModel::Classifier(m) => m.context_window,
                        AnyModel::Image(_) => 0,
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let first = create_llama_provider();
    first.provider().refresh_models(refresh_context(&url, None, caching_publish(cache.clone()), true)).await.unwrap();
    assert_eq!(windows(&cache), vec![65536, 65536]);

    *loaded.lock() = false;
    let second = create_llama_provider();
    let stored = cache.lock().clone();
    second.provider().refresh_models(refresh_context(&url, stored, caching_publish(cache.clone()), true)).await.unwrap();
    let models = second.provider().get_models();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].context_window, 65536);
    assert_eq!(windows(&cache), vec![65536, 65536]);

    *unloaded_args.lock() = Some(vec!["llama-server".into(), "--ctx-size".into(), "32768".into()]);
    let stored = cache.lock().clone();
    second.provider().refresh_models(refresh_context(&url, stored, caching_publish(cache.clone()), true)).await.unwrap();
    assert_eq!(windows(&cache), vec![32768, 32768]);
}

#[tokio::test]
async fn exposes_unloaded_presets_only_when_router_autoload_is_enabled() {
    let props_requests = Arc::new(Mutex::new(0));
    let auth_headers = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let (count, auth) = (props_requests.clone(), auth_headers.clone());
    let url = listen(handler(move |request| {
        auth.lock().push(request.headers.get("authorization").cloned());
        if request.url == "/models" {
            return Reply::Json(json!({ "data": [
                { "id": "preset", "status": { "value": "unloaded" }, "source": "preset", "meta": { "n_ctx": 65536 } },
                { "id": "failed-preset", "status": { "value": "unloaded", "failed": true }, "source": "preset" },
                { "id": "cache", "status": { "value": "unloaded" }, "source": "cache" },
                { "id": "models-dir", "status": { "value": "unloaded" }, "source": "models_dir" }
            ] }));
        }
        if request.url == "/props" {
            *count.lock() += 1;
            return Reply::Json(json!({ "role": "router", "models_autoload": true }));
        }
        Reply::NotFound
    }))
    .await;
    let cache: Cache = Arc::default();
    let controller = create_llama_provider();
    controller.provider().refresh_models(refresh_context(&url, None, caching_publish(cache.clone()), true)).await.unwrap();
    assert_eq!(*props_requests.lock(), 1);
    assert!(auth_headers.lock().iter().all(|h| h.as_deref() == Some("Bearer local")));
    assert_eq!(chat_ids(&controller), vec!["preset"]);
    assert_eq!(cached_models(&cache), pairs(&[("preset", "openai-completions"), ("preset", "llama-cpp-classify")]));
}

#[tokio::test]
async fn hides_unloaded_presets_when_router_autoload_is_disabled() {
    let url = listen(handler(|request| {
        if request.url == "/models" {
            return Reply::Json(json!({ "data": [{ "id": "preset", "status": { "value": "unloaded" }, "source": "preset" }] }));
        }
        if request.url == "/props" {
            return Reply::Json(json!({ "role": "router", "models_autoload": false }));
        }
        Reply::NotFound
    }))
    .await;
    let controller = create_llama_provider();
    controller.provider().refresh_models(refresh_context(&url, None, caching_publish(Arc::default()), true)).await.unwrap();
    assert!(controller.provider().get_models().is_empty());
}

#[tokio::test]
async fn classifies_with_selectable_models_through_llama_server() {
    let paths = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = paths.clone();
    let url = listen(handler(move |request| {
        seen.lock().push(request.url.clone());
        let payload: Value = serde_json::from_str(&request.body).unwrap_or(Value::Null);
        assert_eq!(payload["model"], json!("qwen"));
        match request.url.as_str() {
            "/tokenize" => {
                let content = payload["content"].as_str().unwrap_or_default();
                Reply::Json(json!({ "tokens": content.chars().map(|c| c as u32).collect::<Vec<_>>() }))
            }
            "/apply-template" => Reply::Json(json!({ "prompt": "<|im_start|>assistant\n" })),
            "/completion" => Reply::Json(json!({ "completion_probabilities": [{ "top_logprobs": [
                { "id": 66, "token": "B", "logprob": -0.1 },
                { "id": 65, "token": "A", "logprob": -2.4 }
            ] }] })),
            _ => Reply::NotFound,
        }
    }))
    .await;
    let controller = create_llama_provider();
    controller.set_catalog(&[model_info(json!({ "id": "qwen", "status": { "value": "loaded" } }))], &url, None);
    let Some(AnyModel::Classifier(mut classifier)) =
        controller.provider().get_all_models().into_iter().find(|m| matches!(m, AnyModel::Classifier(_)))
    else {
        panic!("missing classifier model");
    };
    // Provider auth resolves the OpenAI-compatible /v1 URL, which replaces the model's base URL.
    classifier.base_url = format!("{url}/v1");
    let context: ClassifierContext = serde_json::from_value(json!({
        "state": { "message": "The build is red again." },
        "questions": { "kind": { "type": "choice", "instructions": "What is this about?", "criteria": { "billing": "", "ci": "" } } }
    }))
    .unwrap();
    let result = controller
        .provider()
        .classify(&classifier, &context, StreamOptions { api_key: Some("local".into()), ..Default::default() })
        .await;
    assert_eq!(result.error_message, None);
    match &result.answers["kind"] {
        ClassifierAnswer::Choice { choice, .. } => assert_eq!(choice, "ci"),
        other => panic!("unexpected answer {other:?}"),
    }
    assert!(paths.lock().iter().any(|p| p == "/completion"));
}

#[tokio::test]
async fn exposes_decision_models_as_native_system_one_classifiers() {
    let props_models = Arc::new(Mutex::new(Vec::<String>::new()));
    let system_one_requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let (props_seen, requests_seen) = (props_models.clone(), system_one_requests.clone());
    let url = listen(handler(move |request| {
        match request.path() {
            "/models" => Reply::Json(json!({ "data": [
                { "id": "qwen", "status": { "value": "loaded" }, "architecture": { "input_modalities": ["text"], "output_modalities": ["text"] }, "meta": { "n_ctx": 32768 } },
                { "id": "kev", "status": { "value": "loaded" }, "architecture": { "input_modalities": ["text"], "output_modalities": ["decisions"] }, "meta": { "n_ctx": 8192 } },
                { "id": "laya", "status": { "value": "sleeping" }, "architecture": { "input_modalities": ["text"], "output_modalities": ["decisions"] } },
                { "id": "legacy", "status": { "value": "loaded" } }
            ] })),
            "/props" => {
                props_seen.lock().push(request.query("model").unwrap_or_default());
                Reply::Json(json!({}))
            }
            "/v1/systemone" => {
                let payload: Value = serde_json::from_str(&request.body).unwrap();
                requests_seen.lock().push(payload.clone());
                let answers: serde_json::Map<String, Value> = payload["questions"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(|id| (id.clone(), json!({ "type": "noul", "noul": 0.82 })))
                    .collect();
                Reply::Json(json!({ "model": payload["model"], "answers": answers, "usage": { "input_tokens": 42, "output_tokens": 0 } }))
            }
            _ => Reply::NotFound,
        }
    }))
    .await;
    let cache: Cache = Arc::default();
    let controller = create_llama_provider();
    controller.provider().refresh_models(refresh_context(&url, None, caching_publish(cache.clone()), true)).await.unwrap();

    // The catalog identifies decision models, so a refresh neither probes them nor reads their chat template.
    assert!(system_one_requests.lock().is_empty());
    let mut props = props_models.lock().clone();
    props.sort();
    assert_eq!(props, vec!["legacy", "qwen"]);
    assert_eq!(chat_ids(&controller), vec!["qwen", "legacy"]);
    let cached: Vec<(String, String, String)> = cache
        .lock()
        .as_ref()
        .unwrap()
        .models
        .iter()
        .map(|m| {
            let base_url = match m {
                AnyModel::Chat(m) => m.base_url.clone(),
                AnyModel::Classifier(m) => m.base_url.clone(),
                AnyModel::Image(m) => m.base_url.clone(),
            };
            (m.id().to_string(), api_of(m), base_url)
        })
        .collect();
    let v1 = format!("{url}/v1");
    assert_eq!(
        cached,
        vec![
            ("qwen".into(), "openai-completions".into(), v1.clone()),
            ("legacy".into(), "openai-completions".into(), v1.clone()),
            ("qwen".into(), "llama-cpp-classify".into(), url.clone()),
            ("kev".into(), "typesafe-system-one".into(), v1.clone()),
            ("laya".into(), "typesafe-system-one".into(), v1.clone()),
            ("legacy".into(), "llama-cpp-classify".into(), url.clone()),
        ]
    );

    let Some(AnyModel::Classifier(kev)) = controller.provider().get_all_models().into_iter().find(|m| m.id() == "kev") else {
        panic!("missing decision classifier model");
    };
    assert_eq!(kev.context_window, 8192);
    let context: ClassifierContext = serde_json::from_value(json!({
        "state": { "message": "I was charged twice." },
        "questions": { "angry": { "type": "bool", "instructions": "Is the customer angry?", "criteria": { "true": "angry", "false": "calm" } } }
    }))
    .unwrap();
    let result = controller
        .provider()
        .classify(&kev, &context, StreamOptions { api_key: Some("local".into()), ..Default::default() })
        .await;
    assert_eq!(result.error_message, None);
    assert_eq!(result.answers["angry"], ClassifierAnswer::Bool { probability: 0.82 });
    let usage = result.usage.unwrap();
    assert_eq!((usage.input, usage.output), (42, 0));
    assert_eq!(
        *system_one_requests.lock(),
        vec![json!({
            "model": "kev",
            "state": { "message": "I was charged twice." },
            "questions": { "angry": { "type": "noul", "instructions": "Is the customer angry?", "criteria": { "true": "angry", "false": "calm" } } }
        })]
    );

    // A cache-only startup restores decision models with their native API.
    let restored = create_llama_provider();
    let stored = cache.lock().clone();
    restored.provider().refresh_models(refresh_context(&url, stored, caching_publish(cache.clone()), false)).await.unwrap();
    assert_eq!(chat_ids(&restored), vec!["qwen", "legacy"]);
    assert_eq!(
        classifier_apis(&restored),
        pairs(&[
            ("qwen", "llama-cpp-classify"),
            ("kev", "typesafe-system-one"),
            ("laya", "typesafe-system-one"),
            ("legacy", "llama-cpp-classify"),
        ])
    );
}

#[test]
fn lists_decision_models_that_also_output_text_for_chat() {
    let controller = create_llama_provider();
    controller.set_catalog(
        &[
            model_info(json!({ "id": "decide", "status": { "value": "sleeping" }, "architecture": { "output_modalities": ["decisions"] } })),
            model_info(json!({ "id": "hybrid", "status": { "value": "loaded" }, "architecture": { "output_modalities": ["text", "decisions"] } })),
        ],
        "http://localhost:8080",
        None,
    );
    assert_eq!(chat_ids(&controller), vec!["hybrid"]);
    assert_eq!(classifier_apis(&controller), pairs(&[("decide", "typesafe-system-one"), ("hybrid", "typesafe-system-one")]));
}

struct EmptyContext;

#[async_trait]
impl AuthContext for EmptyContext {
    async fn env(&self, _name: &str) -> Option<String> {
        None
    }
    async fn file_exists(&self, _path: &str) -> bool {
        false
    }
}

struct Answers(Mutex<Vec<String>>);

#[async_trait]
impl AuthInteraction for Answers {
    async fn prompt(&self, _prompt: AuthPrompt) -> anyhow::Result<String> {
        Ok(self.0.lock().remove(0))
    }
    fn notify(&self, _event: AuthEvent) {}
}

#[tokio::test]
async fn stays_dormant_until_configured_and_stores_url_plus_optional_key() {
    let controller = create_llama_provider();
    let provider = controller.provider();
    let auth = provider.auth().api_key.clone().unwrap();
    let input = |credential: Option<ApiKeyCredential>| ApiKeyResolveInput {
        ctx: Arc::new(EmptyContext),
        credential,
        signal: CancellationToken::new(),
    };
    assert!(auth.check(input(None)).await.unwrap().is_none());
    assert!(auth.resolve(input(None)).await.unwrap().is_none());

    let headers = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let seen = headers.clone();
    let url = listen(handler(move |request| {
        seen.lock().push(request.headers.get("authorization").cloned());
        Reply::Json(json!({ "data": [] }))
    }))
    .await;
    let credential = auth
        .login(ProviderAuthInteraction {
            interaction: Arc::new(Answers(Mutex::new(vec![url.clone(), "secret".into()]))),
            signal: CancellationToken::new(),
        })
        .await
        .unwrap();
    assert_eq!(*headers.lock(), vec![Some("Bearer secret".to_string())]);
    assert_eq!(
        credential,
        ApiKeyCredential {
            key: Some("secret".into()),
            env: Some(HashMap::from([("LLAMA_BASE_URL".to_string(), url.clone())])),
        }
    );
    let resolved = auth.resolve(input(Some(credential))).await.unwrap().unwrap();
    assert_eq!(resolved.auth.api_key.as_deref(), Some("secret"));
    assert_eq!(resolved.auth.base_url, Some(format!("{url}/v1")));
    assert_eq!(resolved.env, Some(HashMap::from([("LLAMA_BASE_URL".to_string(), url.clone())])));
    assert_eq!(resolved.source.as_deref(), Some("stored credential"));
}

#[tokio::test]
async fn searches_hugging_face_and_reads_quantizations_plus_access_requirements() {
    let url = listen(handler(|request| {
        assert_eq!(request.headers.get("authorization").map(String::as_str), Some("Bearer hf-secret"));
        if request.url.starts_with("/api/models?") {
            assert_eq!(request.query("search").as_deref(), Some("qwen coder"));
            assert_eq!(request.query("filter").as_deref(), Some("gguf"));
            assert_eq!(request.query("sort").as_deref(), Some("downloads"));
            return Reply::Json(json!([{ "id": "owner/model-GGUF", "downloads": 1200 }]));
        }
        if request.url == "/api/models/owner/model-GGUF?blobs=true" {
            return Reply::Json(json!({
                "id": "owner/model-GGUF",
                "gated": "manual",
                "siblings": [
                    { "rfilename": "model-Q5_K_M.gguf", "size": 6000 },
                    { "rfilename": "model-Q4_K_M-00001-of-00002.gguf", "size": 2000 },
                    { "rfilename": "model-Q4_K_M-00002-of-00002.gguf", "size": 3000 },
                    { "rfilename": "mmproj-F16.gguf", "size": 1000 }
                ]
            }));
        }
        Reply::NotFound
    }))
    .await;
    let client = HuggingFaceClient::new(Some("hf-secret"), Some(&url));
    assert_eq!(
        client.search("qwen coder", None).await.unwrap(),
        vec![HuggingFaceModel { id: "owner/model-GGUF".into(), downloads: 1200.0 }]
    );
    assert_eq!(
        client.details("owner/model-GGUF", None).await.unwrap(),
        HuggingFaceModelDetails {
            id: "owner/model-GGUF".into(),
            gated: HuggingFaceModelDetailsGated::String("manual".into()),
            quantizations: vec![
                HuggingFaceQuantization { name: "Q4_K_M".into(), size: Some(5000) },
                HuggingFaceQuantization { name: "Q5_K_M".into(), size: Some(6000) },
            ],
        }
    );
    assert_eq!(
        find_hugging_face_token(Some(json!({ "HF_TOKEN": " hf-secret " }))).await.unwrap().as_deref(),
        Some("hf-secret")
    );
}

#[tokio::test]
async fn loads_with_sse_progress_and_waits_for_the_loaded_catalog_state() {
    let status = Arc::new(Mutex::new("unloaded"));
    let streams = Streams::default();
    let (state, sse) = (status.clone(), streams.clone());
    let url = listen(handler(move |request| {
        if request.url == "/models/sse" {
            return sse.subscribe();
        }
        if request.url == "/models/load" && request.method == "POST" {
            *state.lock() = "loading";
            let (state, sse) = (state.clone(), sse.clone());
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                sse.send(json!({ "model": "test-model", "event": "status_change", "data": {
                    "status": "loading",
                    "progress": { "stages": ["text_model", "mmproj_model"], "current": "text_model", "value": 0.5 }
                } }));
                *state.lock() = "loaded";
                sse.send(json!({ "model": "test-model", "event": "status_change", "data": { "status": "loaded" } }));
            });
            return Reply::Json(json!({ "success": true }));
        }
        if request.url == "/models" {
            return Reply::Json(json!({ "data": [{ "id": "test-model", "status": { "value": *state.lock() } }] }));
        }
        Reply::NotFound
    }))
    .await;
    let progress = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = progress.clone();
    let model = LlamaClient::new(&url, None)
        .load_and_wait("test-model", Arc::new(move |entry: LlamaProgress| seen.lock().push(entry.message)), None)
        .await
        .unwrap();
    assert_eq!(model.status.value, LlamaModelStatus::Loaded);
    let messages = progress.lock().clone();
    assert!(messages.contains(&"Loading text model".to_string()), "{messages:?}");
}

#[tokio::test]
async fn downloads_with_byte_progress_and_returns_the_refreshed_catalog() {
    let status = Arc::new(Mutex::new("missing"));
    let streams = Streams::default();
    let (state, sse) = (status.clone(), streams.clone());
    let url = listen(handler(move |request| {
        if request.url == "/models/sse" {
            return sse.subscribe();
        }
        if request.url == "/models" && request.method == "POST" {
            *state.lock() = "downloading";
            let (state, sse) = (state.clone(), sse.clone());
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                sse.send(json!({ "model": "owner/repo:Q4_K_M", "event": "download_progress", "data": {
                    "progress": { "https://example/model.gguf": { "done": 512, "total": 1024 } }
                } }));
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                *state.lock() = "unloaded";
                sse.send(json!({ "model": "owner/repo:Q4_K_M", "event": "download_finished", "data": {} }));
            });
            return Reply::Json(json!({ "success": true }));
        }
        if request.url.starts_with("/models") {
            let current = *state.lock();
            return Reply::Json(json!({ "data": if current == "missing" { json!([]) } else {
                json!([{ "id": "owner/repo:Q4_K_M", "status": { "value": current } }])
            } }));
        }
        Reply::NotFound
    }))
    .await;
    let progress = Arc::new(Mutex::new(Vec::<LlamaProgress>::new()));
    let seen = progress.clone();
    let models = LlamaClient::new(&url, None)
        .download_and_wait("owner/repo:Q4_K_M", Arc::new(move |entry| seen.lock().push(entry)), None)
        .await
        .unwrap();
    assert_eq!(models, vec![model_info(json!({ "id": "owner/repo:Q4_K_M", "status": { "value": "unloaded" } }))]);
    assert!(progress.lock().contains(&LlamaProgress {
        message: "Downloading model".into(),
        ratio: Some(0.5),
        detail: Some("512 B / 1.00 KiB".into()),
    }));
}
