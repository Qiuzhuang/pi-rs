//! Port of `test/model-catalog-protocol.test.ts` (regression test for #9099): runs the current
//! client against the catalog selection pi.dev performs.
//!
//! The pi.dev side (`scripts/model-catalog-protocol.ts`, shared with pi.dev) is reproduced below
//! for the parts the test server uses. The final provider request is captured by pointing the
//! negotiated model at the local server instead of stubbing `fetch`.

#[path = "../../pi-ai/tests/mock_http/mod.rs"]
mod mock_http;

use std::cmp::Ordering;
use std::sync::Arc;

use mock_http::{MockServer, Response};
use pi_ai::auth::credential_store::InMemoryCredentialStore;
use pi_ai::auth::types::{Credential, CredentialStore, OAuthCredentials};
use pi_ai::*;
use pi_coding_agent::config::VERSION;
use pi_coding_agent::core::model_runtime::{CreateModelRuntimeOptions, ModelRuntime};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// scripts/model-catalog-protocol.ts (server side)
// ---------------------------------------------------------------------------

const MODEL_CATALOG_PREFIX: &str = "models/v1";

#[derive(Clone, Copy, PartialEq)]
enum Representation {
    Legacy,
    Typed,
}

struct IndexEntry {
    minimum_pi_version: &'static str,
    revision: String,
}

struct CatalogIndex {
    default_revision: String,
    catalogs: Vec<IndexEntry>,
}

enum CatalogRequest {
    Catalog { pi_version: Option<String>, representation: Representation },
    Redirect { location: String },
    Invalid,
}

fn provider_key(revision: &str, provider: &str, representation: Representation) -> String {
    let suffix = if representation == Representation::Typed { ".all.json" } else { ".json" };
    format!("{MODEL_CATALOG_PREFIX}/revisions/{revision}/providers/{provider}{suffix}")
}

type PiVersion = (u64, u64, u64, String);

fn parse_pi_version(version: &str) -> Option<PiVersion> {
    let re = regex::Regex::new(r"^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$").unwrap();
    let captures = re.captures(version.trim())?;
    Some((
        captures[1].parse().ok()?,
        captures[2].parse().ok()?,
        captures[3].parse().ok()?,
        captures.get(4).map(|m| m.as_str().to_string()).unwrap_or_default(),
    ))
}

fn compare_pi_versions(left: &PiVersion, right: &PiVersion) -> Ordering {
    let core = (left.0, left.1, left.2).cmp(&(right.0, right.1, right.2));
    if core != Ordering::Equal {
        return core;
    }
    match (left.3.is_empty(), right.3.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        // The test index has no prerelease versions; plain comparison suffices.
        (false, false) => left.3.cmp(&right.3),
    }
}

fn parse_representation(types: Option<&str>) -> Option<Representation> {
    let Some(types) = types else { return Some(Representation::Legacy) };
    let re = regex::Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]*$").unwrap();
    if types.is_empty() || types.split(',').any(|t| !re.is_match(t)) {
        return None;
    }
    Some(Representation::Typed)
}

fn parse_catalog_request(url: &url::Url, user_agent: Option<&str>) -> CatalogRequest {
    let query = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
    let Some(representation) = parse_representation(query("types").as_deref()) else {
        return CatalogRequest::Invalid;
    };
    match query("pi-version") {
        None => {
            let re = regex::RegexBuilder::new(r"^pi/([^\s()]+)(?: \([^;()]+(?:;\s*[^;()]+(?:;\s*[^()]+)?)?\))?$")
                .case_insensitive(true)
                .build()
                .unwrap();
            let version = re.captures(user_agent.unwrap_or_default()).map(|c| c[1].to_string());
            match version {
                Some(version) if parse_pi_version(&version).is_some() => {
                    let mut location = url.clone();
                    location.query_pairs_mut().append_pair("pi-version", &version);
                    CatalogRequest::Redirect { location: location.to_string() }
                }
                _ => CatalogRequest::Catalog { pi_version: None, representation },
            }
        }
        Some(version) if parse_pi_version(&version).is_none() => CatalogRequest::Invalid,
        Some(version) => CatalogRequest::Catalog { pi_version: Some(version), representation },
    }
}

fn select_catalog<'a>(index: &'a CatalogIndex, pi_version: Option<&str>) -> Option<&'a IndexEntry> {
    let Some(pi_version) = pi_version else {
        return index.catalogs.iter().find(|c| c.revision == index.default_revision);
    };
    let requested = parse_pi_version(pi_version)?;
    let mut selected: Option<(&IndexEntry, PiVersion)> = None;
    for catalog in &index.catalogs {
        let minimum = parse_pi_version(catalog.minimum_pi_version).unwrap();
        if compare_pi_versions(&minimum, &requested) != Ordering::Greater
            && selected.as_ref().is_none_or(|(_, version)| compare_pi_versions(&minimum, version) == Ordering::Greater)
        {
            selected = Some((catalog, minimum));
        }
    }
    selected.map(|(entry, _)| entry)
}

// ---------------------------------------------------------------------------
// test
// ---------------------------------------------------------------------------

const MODEL_ID: &str = "anthropic/claude-sonnet-5";

fn common_model() -> Value {
    json!({
        "id": MODEL_ID, "name": "Claude Sonnet 5", "provider": "openrouter", "reasoning": true, "input": ["text"],
        "cost": { "input": 3, "output": 15, "cacheRead": 0.3, "cacheWrite": 3.75 },
        "contextWindow": 200_000, "maxTokens": 64_000
    })
}

fn with_fields(mut base: Value, fields: Value) -> Value {
    for (k, v) in fields.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

#[tokio::test]
async fn negotiates_the_catalog_for_its_version_and_reaches_the_openrouter_api() {
    let legacy_revision = format!("sha256-{}", "a".repeat(64));
    let mixed_revision = format!("sha256-{}", "b".repeat(64));
    // Newer than the bundled catalog, so the client applies the remote overlay.
    let last_modified = "Thu, 01 Jan 2099 00:00:00 GMT";
    let legacy_model = with_fields(common_model(), json!({ "api": "openai-completions", "baseUrl": "https://openrouter.ai/api/v1" }));
    let mixed_model = with_fields(common_model(), json!({ "api": "anthropic-messages", "baseUrl": "https://openrouter.ai/api" }));
    let index = Arc::new(CatalogIndex {
        default_revision: mixed_revision.clone(),
        catalogs: vec![
            IndexEntry { minimum_pi_version: "0.80.7", revision: legacy_revision.clone() },
            IndexEntry { minimum_pi_version: "0.85.0", revision: mixed_revision.clone() },
        ],
    });
    // The legacy revision predates typed shards, so typed requests fall back to the chat-only shard.
    let mut objects = std::collections::HashMap::new();
    objects.insert(provider_key(&legacy_revision, "openrouter", Representation::Legacy), json!({ MODEL_ID: legacy_model }));
    objects.insert(provider_key(&mixed_revision, "openrouter", Representation::Legacy), json!({ MODEL_ID: mixed_model }));
    objects.insert(
        provider_key(&mixed_revision, "openrouter", Representation::Typed),
        json!([with_fields(mixed_model.clone(), json!({ "type": "chat" }))]),
    );
    let objects = Arc::new(objects);

    let server_index = index.clone();
    let server = MockServer::start(move |request, _| {
        let host = request.header("host").unwrap_or("127.0.0.1").to_string();
        let url = url::Url::parse(&format!("http://{host}{}", request.path)).unwrap();
        let provider = url.path().strip_prefix("/api/models/providers/").filter(|p| !p.is_empty() && !p.contains('/'));
        // Provider requests captured after negotiation.
        if url.path().starts_with("/api/v1/") {
            return Response::text(500, "Provider request captured");
        }
        let Some(provider) = provider else { return Response::text(404, "") };
        match parse_catalog_request(&url, request.header("user-agent")) {
            CatalogRequest::Redirect { location } => Response::new(307, "text/plain", "")
                .with_header("location", &location)
                .with_header("cache-control", "no-store"),
            CatalogRequest::Invalid => Response::text(400, "invalid"),
            CatalogRequest::Catalog { pi_version, representation } => {
                let catalog = select_catalog(&server_index, pi_version.as_deref());
                let body = catalog.and_then(|c| {
                    objects
                        .get(&provider_key(&c.revision, provider, representation))
                        .or_else(|| objects.get(&provider_key(&c.revision, provider, Representation::Legacy)))
                });
                match (catalog, body) {
                    (Some(catalog), Some(body)) => Response::json(200, body.clone())
                        .with_header("last-modified", last_modified)
                        .with_header("x-pi-model-catalog-revision", &catalog.revision),
                    _ => Response::text(404, ""),
                }
            }
        }
    })
    .await;

    // TS stores an API key, which makes the OpenRouter personalization call openrouter.ai (and fall
    // back to the catalog on 401). A sign-in without an access token keeps the catalog refresh on
    // the network path while the personalization keeps the catalog without leaving the machine.
    let credentials = Arc::new(InMemoryCredentialStore::new());
    credentials
        .modify(
            "openrouter",
            Box::new(|_| {
                Box::pin(async {
                    Ok(Some(Credential::OAuth(OAuthCredentials {
                        access: String::new(),
                        refresh: String::new(),
                        expires: i64::MAX / 2,
                        extra: Default::default(),
                    })))
                })
            }),
            None,
        )
        .await
        .unwrap();
    let runtime = ModelRuntime::create(Some(&CreateModelRuntimeOptions {
        credentials: Some(credentials),
        models_path: Some(String::new()),
        catalog_base_url: Some(server.url.clone()),
        refresh_on_create: Some(false),
        ..Default::default()
    }))
    .await
    .unwrap();
    let refresh = runtime
        .refresh(Some(ModelsRefreshOptions {
            allow_network: Some(true),
            force: Some(true),
            providers: Some(vec!["openrouter".into()]),
            ..Default::default()
        }))
        .await
        .unwrap();
    assert!(refresh.errors.is_empty(), "{:?}", refresh.errors);

    let catalog_url = "/api/models/providers/openrouter?types=chat%2Cimage%2Cclassifier";
    let paths: Vec<String> = server.requests().iter().map(|r| r.path.clone()).collect();
    assert_eq!(paths, vec![catalog_url.to_string(), format!("{catalog_url}&pi-version={VERSION}")]);

    let expected = if select_catalog(&index, Some(VERSION)).map(|c| c.revision.clone()) == Some(legacy_revision) {
        legacy_model
    } else {
        mixed_model
    };
    let model = runtime.get_model("openrouter", MODEL_ID).expect("negotiated model");
    assert_eq!(json!(model.api), expected["api"]);
    assert_eq!(json!(model.base_url), expected["baseUrl"]);

    let local = Model { base_url: model.base_url.replace("https://openrouter.ai", &server.url), ..model.clone() };
    let context: Context = serde_json::from_value(json!({ "messages": [{ "role": "user", "content": "Hello", "timestamp": 0 }] })).unwrap();
    runtime
        .complete_simple(
            local,
            context,
            Some(StreamOptions { api_key: Some("test-key".into()), max_retries: Some(0), ..Default::default() }.into()),
        )
        .await;
    let provider_path = server.requests().last().map(|r| r.path_only().to_string());
    let expected_path = if expected["api"] == "anthropic-messages" { "/api/v1/messages" } else { "/api/v1/chat/completions" };
    assert_eq!(provider_path.as_deref(), Some(expected_path));
}
