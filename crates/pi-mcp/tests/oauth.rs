mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use common::{Request, Response, listen};
use futures::future::BoxFuture;
use parking_lot::Mutex;
use pi_mcp::oauth::{
    AuthorizationServerDiscoveryOptions, AuthorizationServerMetadata, InvalidateCredentials,
    McpOAuthProvider, McpOAuthProviderOptions, McpOAuthStateStore, MemoryOAuthStateStore,
    OAuthCallbackPage, OAuthCallbackServer, OAuthCallbackServerOptions, OAuthClientInformation,
    OAuthClientMetadata, OAuthClientProvider, OAuthDiscoveryState, OAuthFlowOptions,
    OAuthFlowResult, OAuthTokens, RegisterClientOptions, adapt_oauth_provider, authorize_mcp,
    discover_authorization_server_metadata, get_search_param, register_client,
};
use pi_mcp::{
    AbortController, Error, HttpResponseHead, McpClient, McpClientOptions, McpRequestOptions,
    StreamableHttpTransport, StreamableHttpTransportOptions, UnauthorizedContext, default_fetch,
};
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

#[derive(Default)]
struct TestState {
    client: Option<OAuthClientInformation>,
    tokens: Option<OAuthTokens>,
    verifier: Option<String>,
    discovery: Option<OAuthDiscoveryState>,
    authorization_url: Option<Url>,
}

struct TestOAuthProvider {
    redirect_url: String,
    state: Mutex<TestState>,
}

impl TestOAuthProvider {
    fn new(redirect_url: &str) -> Arc<Self> {
        Arc::new(Self {
            redirect_url: redirect_url.to_string(),
            state: Mutex::default(),
        })
    }

    fn authorization_url(&self) -> Option<Url> {
        self.state.lock().authorization_url.clone()
    }
}

#[async_trait]
impl OAuthClientProvider for TestOAuthProvider {
    fn redirect_url(&self) -> String {
        self.redirect_url.clone()
    }

    fn client_metadata(&self) -> OAuthClientMetadata {
        OAuthClientMetadata {
            redirect_uris: vec![self.redirect_url.clone()],
            client_name: Some("pi-mcp-test".into()),
            grant_types: Some(vec!["authorization_code".into(), "refresh_token".into()]),
            response_types: Some(vec!["code".into()]),
            token_endpoint_auth_method: Some("none".into()),
            ..Default::default()
        }
    }

    async fn state(&self) -> pi_mcp::Result<Option<String>> {
        Ok(Some("expected-state".into()))
    }

    async fn client_information(&self) -> pi_mcp::Result<Option<OAuthClientInformation>> {
        Ok(self.state.lock().client.clone())
    }

    async fn save_client_information(
        &self,
        information: OAuthClientInformation,
    ) -> pi_mcp::Result<()> {
        self.state.lock().client = Some(information);
        Ok(())
    }

    async fn tokens(&self) -> pi_mcp::Result<Option<OAuthTokens>> {
        Ok(self.state.lock().tokens.clone())
    }

    async fn save_tokens(&self, tokens: OAuthTokens) -> pi_mcp::Result<()> {
        self.state.lock().tokens = Some(tokens);
        Ok(())
    }

    async fn redirect_to_authorization(&self, url: Url) -> pi_mcp::Result<()> {
        self.state.lock().authorization_url = Some(url);
        Ok(())
    }

    async fn save_code_verifier(&self, verifier: String) -> pi_mcp::Result<()> {
        self.state.lock().verifier = Some(verifier);
        Ok(())
    }

    async fn code_verifier(&self) -> pi_mcp::Result<String> {
        self.state
            .lock()
            .verifier
            .clone()
            .ok_or_else(|| Error::other("Missing code verifier"))
    }

    async fn invalidate_credentials(&self, kind: InvalidateCredentials) -> pi_mcp::Result<()> {
        let mut state = self.state.lock();
        let all = kind == InvalidateCredentials::All;
        if all || kind == InvalidateCredentials::Client {
            state.client = None;
        }
        if all || kind == InvalidateCredentials::Tokens {
            state.tokens = None;
        }
        if all || kind == InvalidateCredentials::Verifier {
            state.verifier = None;
        }
        if all || kind == InvalidateCredentials::Discovery {
            state.discovery = None;
        }
        Ok(())
    }

    async fn save_discovery_state(&self, state: OAuthDiscoveryState) -> pi_mcp::Result<()> {
        self.state.lock().discovery = Some(state);
        Ok(())
    }

    async fn discovery_state(&self) -> pi_mcp::Result<Option<OAuthDiscoveryState>> {
        Ok(self.state.lock().discovery.clone())
    }
}

fn boxed(future: impl std::future::Future<Output = ()> + Send + 'static) -> BoxFuture<'static, ()> {
    Box::pin(future)
}

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

const JSON: (&str, &str) = ("content-type", "application/json");

fn form(request: &Request) -> pi_mcp::oauth::UrlSearchParams {
    pi_mcp::oauth::UrlSearchParams::parse(&request.text())
}

fn as_metadata(origin: &str) -> Value {
    json!({
        "issuer": origin,
        "authorization_endpoint": format!("{origin}/authorize"),
        "token_endpoint": format!("{origin}/token"),
        "response_types_supported": ["code"],
    })
}

fn http_client(redirects: bool) -> reqwest::Client {
    let policy = if redirects {
        reqwest::redirect::Policy::default()
    } else {
        reqwest::redirect::Policy::none()
    };
    reqwest::Client::builder().redirect(policy).build().unwrap()
}

#[tokio::test]
async fn discovers_registers_authorizes_with_pkce_and_refreshes_on_401() {
    let expected_challenge: Arc<Mutex<Option<String>>> = Arc::default();
    let refreshes = Arc::new(AtomicUsize::new(0));
    let (challenge_slot, refresh_count) = (expected_challenge.clone(), refreshes.clone());
    let origin = listen(move |request: Request, response: Response, origin: String| {
        let (challenge_slot, refresh_count) = (challenge_slot.clone(), refresh_count.clone());
        boxed(async move {
            let url = request.url(&origin);
            match url.path() {
                "/.well-known/oauth-protected-resource/mcp" => {
                    let body = json!({
                        "resource": format!("{origin}/mcp"),
                        "authorization_servers": [origin],
                        "scopes_supported": ["org:read"],
                    });
                    return response.send(200, &[JSON], body.to_string()).await;
                }
                "/.well-known/oauth-authorization-server" => {
                    let body = json!({
                        "issuer": origin,
                        "authorization_endpoint": format!("{origin}/authorize"),
                        "token_endpoint": format!("{origin}/token"),
                        "registration_endpoint": format!("{origin}/register"),
                        "response_types_supported": ["code"],
                        "grant_types_supported": ["authorization_code", "refresh_token"],
                        "token_endpoint_auth_methods_supported": ["none"],
                        "code_challenge_methods_supported": ["S256"],
                    });
                    return response.send(200, &[JSON], body.to_string()).await;
                }
                "/register" => {
                    let mut metadata = request.json();
                    // Empty and null optional fields count as absent (#10266).
                    metadata["client_id"] = json!("test-client");
                    metadata["client_secret"] = json!("");
                    return response.send(201, &[JSON], metadata.to_string()).await;
                }
                "/authorize" => {
                    *challenge_slot.lock() = get_search_param(&url, "code_challenge");
                    let mut redirect = Url::parse(&get_search_param(&url, "redirect_uri").unwrap()).unwrap();
                    redirect.query_pairs_mut().append_pair("code", "test-code");
                    redirect.query_pairs_mut().append_pair("state", &get_search_param(&url, "state").unwrap_or_default());
                    return response.send(302, &[("location", redirect.as_str())], "").await;
                }
                "/token" => {
                    let params = form(&request);
                    if params.get("grant_type") == Some("refresh_token") {
                        refresh_count.fetch_add(1, Ordering::SeqCst);
                        let body = json!({
                            "access_token": "refreshed-token",
                            "token_type": "Bearer",
                            "refresh_token": "",
                            "expires_in": null,
                        });
                        return response.send(200, &[JSON], body.to_string()).await;
                    }
                    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .encode(Sha256::digest(params.get("code_verifier").unwrap_or("").as_bytes()));
                    if params.get("code") != Some("test-code") || Some(challenge) != *challenge_slot.lock() {
                        return response.send(400, &[JSON], json!({ "error": "invalid_grant" }).to_string()).await;
                    }
                    let body = json!({
                        "access_token": "first-token",
                        "refresh_token": "refresh-token",
                        "token_type": "Bearer",
                        "scope": "",
                    });
                    return response.send(200, &[JSON], body.to_string()).await;
                }
                "/mcp" => {}
                _ => return response.send(404, &[], "").await,
            }
            match request.method.as_str() {
                "GET" => return response.send(405, &[], "").await,
                "DELETE" => return response.send(200, &[], "").await,
                _ => {}
            }
            let token = request.header("authorization");
            if token != Some("Bearer first-token") && token != Some("Bearer refreshed-token") {
                // An empty scope falls through to the resource metadata's scopes_supported.
                let challenge =
                    format!("Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\", scope=\"\"");
                return response.send(401, &[("www-authenticate", challenge.as_str())], "Unauthorized").await;
            }
            let message = request.json();
            if message.get("id").is_none() {
                return response.send(202, &[], "").await;
            }
            let result = if message["method"] == "initialize" {
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "oauth-test", "version": "1.0.0" },
                })
            } else {
                json!({ "tools": [{ "name": "issues", "inputSchema": { "type": "object" } }] })
            };
            let body = json!({ "jsonrpc": "2.0", "id": message["id"], "result": result });
            response.send(200, &[JSON], body.to_string()).await
        })
    })
    .await;

    let callback = OAuthCallbackServer::listen(OAuthCallbackServerOptions::default())
        .await
        .unwrap();
    let provider = TestOAuthProvider::new(callback.redirect_url());
    let server_url = format!("{origin}/mcp");
    let transport = |provider: &Arc<TestOAuthProvider>| {
        Arc::new(StreamableHttpTransport::new(
            StreamableHttpTransportOptions::parse(&server_url)
                .unwrap()
                .header("Authorization", "Bearer caller-supplied-stale-token")
                .auth_provider(adapt_oauth_provider(provider.clone()))
                .open_get_stream(false),
        ))
    };
    let first = McpClient::new(McpClientOptions::new("oauth-test", "1.0.0"));
    let error = first.connect(transport(&provider)).await.unwrap_err();
    assert!(
        matches!(error, Error::OAuthAuthorizationRequired),
        "{error:?}"
    );
    let authorization_url = provider.authorization_url().unwrap();
    assert_eq!(
        get_search_param(&authorization_url, "scope").as_deref(),
        Some("org:read")
    );
    assert_eq!(
        get_search_param(&authorization_url, "resource"),
        Some(server_url.clone())
    );
    assert_eq!(
        provider.state.lock().client.as_ref().unwrap().client_secret,
        None
    );

    let callback_result = callback.wait_for_callback("expected-state", None).unwrap();
    let authorization = http_client(false)
        .get(authorization_url.as_str())
        .send()
        .await
        .unwrap();
    let location = authorization
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let _ = http_client(true).get(&location).send().await.unwrap();
    let code = callback_result.await.unwrap().code;
    let options = OAuthFlowOptions {
        authorization_code: Some(code),
        fetch: Some(default_fetch()),
        ..OAuthFlowOptions::new(url(&server_url))
    };
    assert_eq!(
        authorize_mcp(provider.as_ref(), &options).await.unwrap(),
        OAuthFlowResult::Authorized
    );

    let client = McpClient::new(McpClientOptions::new("oauth-test", "1.0.0"));
    client.connect(transport(&provider)).await.unwrap();
    assert_eq!(
        serde_json::to_value(
            client
                .list_tools(McpRequestOptions::default())
                .await
                .unwrap()
        )
        .unwrap(),
        json!([{ "name": "issues", "inputSchema": { "type": "object" } }])
    );
    client.close().await.unwrap();

    provider.state.lock().tokens.as_mut().unwrap().access_token = "stale-token".into();
    let refreshed = McpClient::new(McpClientOptions::new("oauth-test", "1.0.0"));
    refreshed.connect(transport(&provider)).await.unwrap();
    // Neither token response names a scope, so the grant has the requested scope.
    assert_eq!(
        serde_json::to_value(provider.state.lock().tokens.clone()).unwrap(),
        json!({
            "access_token": "refreshed-token",
            "refresh_token": "refresh-token",
            "token_type": "Bearer",
            "scope": "org:read",
        })
    );
    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    refreshed.close().await.unwrap();
    callback.close().await.unwrap();
}

fn unauthorized(
    origin: &str,
    headers: &[(&str, &str)],
    status: u16,
    token: &str,
) -> UnauthorizedContext {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        pi_mcp::auth_provider::set_header(&mut map, name, value);
    }
    UnauthorizedContext {
        response: HttpResponseHead {
            status,
            headers: map,
        },
        server_url: url(&format!("{origin}/mcp")),
        fetch: default_fetch(),
        token: Some(token.to_string()),
    }
}

#[tokio::test]
async fn shares_one_refresh_between_concurrent_401s_when_refresh_tokens_rotate() {
    let grants: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = grants.clone();
    let origin = listen(move |request: Request, response: Response, origin: String| {
        let log = log.clone();
        boxed(async move {
            let url = request.url(&origin);
            match url.path() {
                "/.well-known/oauth-protected-resource/mcp" => {
                    // Invalid resource metadata falls back to the server origin instead of failing discovery.
                    let body = json!({ "resource": format!("{origin}/mcp"), "authorization_servers": ["not a url"] });
                    response.send(200, &[JSON], body.to_string()).await
                }
                // Issuer without the trailing slash that URL parsing adds to the fallback server URL.
                "/.well-known/oauth-authorization-server" => response.send(200, &[JSON], as_metadata(&origin).to_string()).await,
                "/token" => {
                    let refresh_token = form(&request).get("refresh_token").unwrap_or("").to_string();
                    log.lock().push(refresh_token.clone());
                    if refresh_token != "r1" {
                        return response.send(400, &[JSON], json!({ "error": "invalid_grant" }).to_string()).await;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    let body = json!({ "access_token": "a2", "refresh_token": "r2", "token_type": "Bearer", "expires_in": 3600 });
                    response.send(200, &[JSON], body.to_string()).await
                }
                _ => response.send(404, &[], "").await,
            }
        })
    })
    .await;
    let store = Arc::new(MemoryOAuthStateStore::new());
    let mut options = McpOAuthProviderOptions::new(
        url(&format!("{origin}/mcp")),
        "http://127.0.0.1/callback",
        |_| Ok(()),
    );
    options.client_metadata.client_name = Some("test".into());
    options.client_id = Some("client".into());
    options.store = Some(store.clone());
    let provider = Arc::new(McpOAuthProvider::new(options));
    provider
        .save_tokens(OAuthTokens {
            refresh_token: Some("r1".into()),
            ..OAuthTokens::bearer("a1")
        })
        .await
        .unwrap();
    let auth = adapt_oauth_provider(provider.clone());
    let headers = [("www-authenticate", "Bearer")];
    let (first, second) = futures::join!(
        auth.on_unauthorized(unauthorized(&origin, &headers, 401, "a1"))
            .unwrap(),
        auth.on_unauthorized(unauthorized(&origin, &headers, 401, "a1"))
            .unwrap(),
    );
    first.unwrap();
    second.unwrap();
    // A late 401 for a request that still carried the old token must not refresh again.
    auth.on_unauthorized(unauthorized(&origin, &headers, 401, "a1"))
        .unwrap()
        .await
        .unwrap();
    assert_eq!(*grants.lock(), vec!["r1".to_string()]);
    assert_eq!(auth.token().await.unwrap().as_deref(), Some("a2"));
    let state = store.load().await.unwrap().unwrap();
    assert_eq!(state.tokens.unwrap().refresh_token.as_deref(), Some("r2"));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64;
    assert!(state.tokens_expire_at.unwrap() > now + 3_500_000.0);
}

#[tokio::test]
async fn asks_for_authorization_instead_of_refreshing_when_the_server_needs_more_scope() {
    let provider = TestOAuthProvider::new("http://127.0.0.1/callback");
    {
        let mut state = provider.state.lock();
        state.client = Some(OAuthClientInformation::new("client"));
        state.tokens = Some(OAuthTokens {
            refresh_token: Some("r1".into()),
            scope: Some("repo read:org".into()),
            ..OAuthTokens::bearer("a1")
        });
    }
    let origin = listen(|request: Request, response: Response, origin: String| {
        boxed(async move {
            let url = request.url(&origin);
            if url.path() == "/.well-known/oauth-authorization-server" {
                return response
                    .send(200, &[JSON], as_metadata(&origin).to_string())
                    .await;
            }
            response
                .send(if url.path() == "/token" { 500 } else { 404 }, &[], "")
                .await
        })
    })
    .await;
    let auth = adapt_oauth_provider(provider.clone());
    let headers = [(
        "www-authenticate",
        "Bearer error=\"insufficient_scope\", scope=\"repo admin\"",
    )];
    let error = auth
        .on_unauthorized(unauthorized(&origin, &headers, 403, "a1"))
        .unwrap()
        .await
        .unwrap_err();
    assert!(
        matches!(error, Error::OAuthAuthorizationRequired),
        "{error:?}"
    );
    // The challenge may list only the missing scopes; the new grant keeps the old ones too.
    assert_eq!(
        get_search_param(&provider.authorization_url().unwrap(), "scope").as_deref(),
        Some("repo read:org admin")
    );
    // The working grant is kept until the user authorizes the new scope.
    assert_eq!(
        provider.state.lock().tokens.as_ref().unwrap().access_token,
        "a1"
    );
}

#[tokio::test]
async fn binds_persisted_credentials_to_the_exact_mcp_server_url() {
    let store: Arc<dyn McpOAuthStateStore> = Arc::new(MemoryOAuthStateStore::new());
    let options = |server: &str| {
        let mut options =
            McpOAuthProviderOptions::new(url(server), "http://127.0.0.1/callback", |_| Ok(()));
        options.client_metadata.client_name = Some("test".into());
        options.store = Some(store.clone());
        options
    };
    let first = McpOAuthProvider::new(options("https://one.example/mcp"));
    first
        .save_tokens(OAuthTokens::bearer("secret"))
        .await
        .unwrap();
    assert_eq!(
        first.tokens().await.unwrap().unwrap().access_token,
        "secret"
    );
    let second = McpOAuthProvider::new(options("https://two.example/mcp"));
    assert_eq!(second.tokens().await.unwrap(), None);
    // The default client metadata fills in the redirect URI and grant settings.
    assert_eq!(
        serde_json::to_value(second.client_metadata()).unwrap(),
        json!({
            "redirect_uris": ["http://127.0.0.1/callback"],
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "client_name": "test",
        })
    );
    // The state parameter is created once and kept.
    let state = first.oauth_state().await.unwrap();
    assert_eq!(state.len(), 64);
    assert_eq!(first.oauth_state().await.unwrap(), state);
}

// #10493
#[tokio::test]
async fn registers_with_an_application_type_derived_from_the_redirect_uris_unless_one_is_set() {
    let bodies: Arc<Mutex<Vec<Value>>> = Arc::default();
    let log = bodies.clone();
    let origin = listen(move |request: Request, response: Response, _| {
        let log = log.clone();
        boxed(async move {
            let mut metadata = request.json();
            log.lock().push(metadata.clone());
            metadata["client_id"] = json!("client");
            response.send(201, &[JSON], metadata.to_string()).await
        })
    })
    .await;
    let register = |redirect_uris: &[&str], application_type: Option<&str>| {
        let origin = origin.clone();
        let client_metadata = OAuthClientMetadata {
            redirect_uris: redirect_uris.iter().map(|uri| uri.to_string()).collect(),
            application_type: application_type.map(str::to_string),
            ..Default::default()
        };
        async move {
            register_client(
                &origin,
                RegisterClientOptions {
                    client_metadata,
                    ..Default::default()
                },
            )
            .await
            .unwrap()
        }
    };
    register(&["http://127.0.0.1:1234/callback"], None).await;
    register(&["http://[::1]/callback"], None).await;
    register(&["com.example.app:/callback"], None).await;
    register(&["https://app.example/callback"], None).await;
    let registered = register(&["http://localhost/callback"], Some("web")).await;
    assert_eq!(
        bodies
            .lock()
            .iter()
            .map(|body| body["application_type"].clone())
            .collect::<Vec<_>>(),
        vec![
            json!("native"),
            json!("native"),
            json!("native"),
            json!("web"),
            json!("web")
        ]
    );
    assert_eq!(registered.client_id, "client");
    assert_eq!(
        registered.redirect_uris(),
        vec!["http://localhost/callback".to_string()]
    );
}

#[tokio::test]
async fn rejects_authorization_metadata_whose_issuer_does_not_match_discovery() {
    let origin = listen(|request: Request, response: Response, origin: String| {
        boxed(async move {
            if request.url(&origin).path() == "/.well-known/oauth-authorization-server" {
                let mut metadata = as_metadata(&origin);
                metadata["issuer"] = json!("https://attacker.example");
                return response.send(200, &[JSON], metadata.to_string()).await;
            }
            response.send(404, &[], "").await
        })
    })
    .await;
    let error = discover_authorization_server_metadata(
        &origin,
        AuthorizationServerDiscoveryOptions::default(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, Error::OAuthIssuerMismatch { .. }),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        format!(
            "OAuth issuer mismatch: expected \"{origin}\", received \"https://attacker.example\""
        )
    );
}

// #10172
#[tokio::test]
async fn uses_a_configured_authorization_server_metadata_document_as_is() {
    let origin = listen(|request: Request, response: Response, origin: String| {
        boxed(async move {
            match request.url(&origin).path() {
                // Names the MCP server itself, which serves no authorization server metadata.
                "/.well-known/oauth-protected-resource/mcp" => {
                    let body = json!({ "resource": format!("{origin}/mcp"), "authorization_servers": [origin] });
                    response.send(200, &[JSON], body.to_string()).await
                }
                "/idp/metadata.json" => {
                    let body = json!({
                        // Not derivable from the document URL; a configured document is not checked.
                        "issuer": "https://idp.example",
                        "authorization_endpoint": format!("{origin}/idp/authorize"),
                        "token_endpoint": format!("{origin}/idp/token"),
                        "response_types_supported": ["code"],
                    });
                    response.send(200, &[JSON], body.to_string()).await
                }
                _ => response.send(404, &[JSON], "").await,
            }
        })
    })
    .await;
    let provider = TestOAuthProvider::new("http://127.0.0.1/callback");
    provider.state.lock().client = Some(OAuthClientInformation::new("client"));
    let options = OAuthFlowOptions {
        authorization_server_metadata_url: Some(url(&format!("{origin}/idp/metadata.json"))),
        ..OAuthFlowOptions::new(url(&format!("{origin}/mcp")))
    };
    assert_eq!(
        authorize_mcp(provider.as_ref(), &options).await.unwrap(),
        OAuthFlowResult::Redirect
    );
    let authorization_url = provider.authorization_url().unwrap();
    assert_eq!(
        format!(
            "{}{}",
            authorization_url.origin().ascii_serialization(),
            authorization_url.path()
        ),
        format!("{origin}/idp/authorize")
    );
    assert_eq!(
        get_search_param(&authorization_url, "resource"),
        Some(format!("{origin}/mcp"))
    );
    // A configured document is not cached as discovery state.
    assert_eq!(provider.state.lock().discovery, None);

    let insecure = OAuthFlowOptions {
        authorization_server_metadata_url: Some(url("http://idp.example/metadata.json")),
        ..options.clone()
    };
    let error = authorize_mcp(provider.as_ref(), &insecure)
        .await
        .unwrap_err();
    assert!(
        matches!(error, Error::OAuthInsecureEndpoint { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn exchanges_a_code_only_when_its_iss_parameter_names_the_authorization_server() {
    let codes: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = codes.clone();
    let origin = listen(move |request: Request, response: Response, _| {
        let log = log.clone();
        boxed(async move {
            if request.method != "POST" {
                return response.send(404, &[], "").await;
            }
            log.lock()
                .push(form(&request).get("code").unwrap_or("").to_string());
            response
                .send(
                    200,
                    &[JSON],
                    json!({ "access_token": "token", "token_type": "Bearer" }).to_string(),
                )
                .await
        })
    })
    .await;
    let exchange = |code: &str, iss: Option<&str>, iss_supported: bool, metadata: bool| {
        let provider = TestOAuthProvider::new("http://127.0.0.1/callback");
        {
            let mut state = provider.state.lock();
            state.client = Some(OAuthClientInformation::new("client"));
            state.verifier = Some("verifier".into());
            state.discovery = Some(OAuthDiscoveryState {
                authorization_server_url: origin.clone(),
                authorization_server_metadata: metadata.then(|| AuthorizationServerMetadata {
                    authorization_response_iss_parameter_supported: Some(iss_supported),
                    ..AuthorizationServerMetadata::new(
                        &origin,
                        format!("{origin}/authorize"),
                        format!("{origin}/token"),
                    )
                }),
                resource_metadata: None,
                resource_metadata_url: None,
            });
        }
        let options = OAuthFlowOptions {
            authorization_code: Some(code.to_string()),
            iss: iss.map(str::to_string),
            ..OAuthFlowOptions::new(url(&format!("{origin}/mcp")))
        };
        async move { authorize_mcp(provider.as_ref(), &options).await }
    };
    let mismatch = |result: pi_mcp::Result<OAuthFlowResult>| {
        matches!(result, Err(Error::OAuthIssuerMismatch { .. }))
    };
    assert!(mismatch(
        exchange("other", Some("https://attacker.example"), false, true).await
    ));
    assert!(mismatch(exchange("missing", None, true, true).await));
    assert_eq!(
        exchange("matching", Some(&origin), true, true)
            .await
            .unwrap(),
        OAuthFlowResult::Authorized
    );
    // Servers that do not promise the parameter may omit it.
    assert_eq!(
        exchange("omitted", None, false, true).await.unwrap(),
        OAuthFlowResult::Authorized
    );
    // Without metadata there is no issuer to compare with, so a server cannot skip the check by hiding it.
    assert!(mismatch(
        exchange(
            "unverifiable",
            Some("https://attacker.example"),
            false,
            false
        )
        .await
    ));
    assert_eq!(
        exchange("no-metadata", None, false, false).await.unwrap(),
        OAuthFlowResult::Authorized
    );
    assert_eq!(
        *codes.lock(),
        vec![
            "matching".to_string(),
            "omitted".into(),
            "no-metadata".into()
        ]
    );
}

// #10565
#[tokio::test]
async fn stops_when_its_signal_aborts_without_falling_back_to_a_redirect() {
    let stalled: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = stalled.clone();
    // Accepts every request and never answers.
    let origin = listen(
        move |request: Request, response: Response, origin: String| {
            let log = log.clone();
            boxed(async move {
                log.lock().push(request.url(&origin).path().to_string());
                response.hold().await
            })
        },
    )
    .await;
    let run = |provider: Arc<TestOAuthProvider>| {
        let (origin, stalled) = (origin.clone(), stalled.clone());
        async move {
            let controller = AbortController::new();
            let count = stalled.lock().len();
            let options = OAuthFlowOptions {
                signal: Some(controller.signal()),
                ..OAuthFlowOptions::new(url(&format!("{origin}/mcp")))
            };
            let flow_provider = provider.clone();
            let flow =
                tokio::spawn(async move { authorize_mcp(flow_provider.as_ref(), &options).await });
            for _ in 0..500 {
                if stalled.lock().len() == count + 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            assert_eq!(stalled.lock().len(), count + 1);
            controller.abort();
            let error = flow.await.unwrap().unwrap_err();
            assert_eq!(error.name(), "AbortError");
            assert_eq!(provider.authorization_url(), None);
            stalled.lock().last().cloned().unwrap()
        }
    };

    // Discovery.
    assert_eq!(
        run(TestOAuthProvider::new("http://127.0.0.1/callback")).await,
        "/.well-known/oauth-protected-resource/mcp"
    );

    // A failed refresh otherwise falls back to a new authorization.
    let refreshing = TestOAuthProvider::new("http://127.0.0.1/callback");
    {
        let mut state = refreshing.state.lock();
        state.client = Some(OAuthClientInformation::new("client"));
        state.tokens = Some(OAuthTokens {
            refresh_token: Some("r1".into()),
            ..OAuthTokens::bearer("a1")
        });
        state.discovery = Some(OAuthDiscoveryState {
            authorization_server_url: origin.clone(),
            authorization_server_metadata: Some(AuthorizationServerMetadata::new(
                &origin,
                format!("{origin}/authorize"),
                format!("{origin}/token"),
            )),
            resource_metadata: None,
            resource_metadata_url: None,
        });
    }
    assert_eq!(run(refreshing).await, "/token");
}

#[tokio::test]
async fn callback_server_renders_plain_text_by_default() {
    let callback = OAuthCallbackServer::listen(OAuthCallbackServerOptions::default())
        .await
        .unwrap();
    let pending = callback.wait_for_callback("s1", None).unwrap();
    let response = http_client(true)
        .get(format!("{}?code=abc&state=s1", callback.redirect_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert_eq!(
        response.text().await.unwrap(),
        "Authorization complete. You may close this window."
    );
    assert_eq!(pending.await.unwrap().code, "abc");
    // A state that is already pending cannot be registered twice.
    let _second = callback.wait_for_callback("s2", None).unwrap();
    assert!(callback.wait_for_callback("s2", None).is_err());
    callback.close().await.unwrap();
}

// #10302
#[tokio::test]
async fn callback_server_rejects_a_response_on_another_path_than_the_expected_one() {
    let callback = OAuthCallbackServer::listen(OAuthCallbackServerOptions {
        extra_paths: vec!["/callback/server-id".into()],
        ..Default::default()
    })
    .await
    .unwrap();
    let origin = Url::parse(callback.redirect_url())
        .unwrap()
        .origin()
        .ascii_serialization();
    let mixed_up = callback
        .wait_for_callback("s1", Some("/callback/server-id"))
        .unwrap();
    let wrong = http_client(true)
        .get(format!("{origin}/callback?code=abc&state=s1"))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status().as_u16(), 400);
    let error = mixed_up.await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("arrived on another redirect URI")
    );

    let pending = callback
        .wait_for_callback("s2", Some("/callback/server-id"))
        .unwrap();
    let right = http_client(true)
        .get(format!("{origin}/callback/server-id?code=abc&state=s2"))
        .send()
        .await
        .unwrap();
    assert_eq!(right.status().as_u16(), 200);
    assert_eq!(pending.await.unwrap().code, "abc");
    let unknown = http_client(true)
        .get(format!("{origin}/elsewhere"))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status().as_u16(), 404);
    callback.close().await.unwrap();
}

#[tokio::test]
async fn callback_server_renders_pages_through_render_page() {
    let pages: Arc<Mutex<Vec<OAuthCallbackPage>>> = Arc::default();
    let log = pages.clone();
    let callback = OAuthCallbackServer::listen(OAuthCallbackServerOptions {
        render_page: Some(Arc::new(move |page: &OAuthCallbackPage| {
            log.lock().push(page.clone());
            match page {
                OAuthCallbackPage::Ok => "<p>ok</p>".to_string(),
                OAuthCallbackPage::Error { message, .. } => format!("<p>{message}</p>"),
            }
        })),
        ..Default::default()
    })
    .await
    .unwrap();
    let denied = callback.wait_for_callback("s1", None).unwrap();
    let failure = http_client(true)
        .get(format!(
            "{}?error=access_denied&error_description=Denied&state=s1",
            callback.redirect_url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        failure.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
    assert_eq!(denied.await.unwrap_err().to_string(), "Denied");
    assert_eq!(
        pages.lock().last().cloned(),
        Some(OAuthCallbackPage::Error {
            message: "Authorization failed. You may close this window.".into(),
            details: Some("Denied".into()),
        })
    );
    let pending = callback.wait_for_callback("s2", None).unwrap();
    let success = http_client(true)
        .get(format!("{}?code=abc&state=s2", callback.redirect_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(success.text().await.unwrap(), "<p>ok</p>");
    assert_eq!(pending.await.unwrap().code, "abc");
    callback.close().await.unwrap();
}
