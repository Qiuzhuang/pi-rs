//! Offline tests for the built-in OAuth flows (ports of `oauth-*.test.ts`, `*-oauth.test.ts`).

mod oauth_support;

use std::sync::Arc;
use std::time::Duration;

use oauth_support::*;
use pi_ai::auth::oauth::*;
use pi_ai::auth::{AuthEvent, AuthPromptKind, LoginOptions, OAuthAuth, OAuthCredential};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

fn take_page(slot: &PageSlot) -> tokio::task::JoinHandle<(u16, String)> {
    slot.lock().take().expect("callback page requested")
}

type PageSlot = Arc<parking_lot::Mutex<Option<tokio::task::JoinHandle<(u16, String)>>>>;

fn cred(access: &str, refresh: &str, extra: Value) -> OAuthCredential {
    OAuthCredential {
        access: access.into(),
        refresh: refresh.into(),
        expires: 0,
        extra: extra.as_object().cloned().unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Callback server
// ---------------------------------------------------------------------------

fn server_options(complete: CompleteFnString) -> OAuthCallbackServerOptions<String> {
    OAuthCallbackServerOptions {
        provider_name: "Example".into(),
        host: "127.0.0.1".into(),
        port: 0,
        path: "/callback".into(),
        redirect_host: None,
        state: Some("expected-state".into()),
        complete,
        signal: None,
        timeout_ms: None,
    }
}

type CompleteFnString = pi_ai::auth::oauth::callback_server::CompleteFn<String>;

fn completing() -> CompleteFnString {
    Arc::new(|code| Box::pin(async move { Ok(format!("completed:{code}")) }))
}

fn with_params(base: &str, params: &[(&str, &str)]) -> String {
    let mut url = url::Url::parse(base).unwrap();
    for (k, v) in params {
        url.query_pairs_mut().append_pair(k, v);
    }
    url.to_string()
}

#[tokio::test]
async fn callback_server_ignores_stray_requests() {
    let server = start_oauth_callback_server(server_options(completing()))
        .await
        .unwrap();
    assert!(
        server.redirect_uri.starts_with("http://127.0.0.1:")
            && server.redirect_uri.ends_with("/callback")
    );
    let other = url::Url::parse(&server.redirect_uri)
        .unwrap()
        .join("/other")
        .unwrap()
        .to_string();
    assert_eq!(http_get(&other).await.0, 404);
    let (status, body) = http_get(&with_params(
        &server.redirect_uri,
        &[("code", "c"), ("state", "other")],
    ))
    .await;
    assert_eq!(status, 400);
    assert!(body.contains("State mismatch."));
    let post = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(with_params(
            &server.redirect_uri,
            &[("code", "c"), ("state", "expected-state")],
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(post.status().as_u16(), 404);
    assert_eq!(
        http_get(&with_params(
            &server.redirect_uri,
            &[("state", "expected-state")]
        ))
        .await
        .0,
        400
    );
    let (status, body) = http_get(&with_params(
        &server.redirect_uri,
        &[("code", "the-code"), ("state", "expected-state")],
    ))
    .await;
    assert_eq!(status, 200);
    assert!(body.contains("Authentication successful"));
    assert!(body.contains("Signed in to Example."));
    assert!(body.contains("fill=\"#F09082\""));
    assert_eq!(
        server.wait().await.unwrap().as_deref(),
        Some("completed:the-code")
    );
    server.close();
}

#[tokio::test]
async fn callback_server_redirect_host_and_no_state() {
    let mut options = server_options(completing());
    options.redirect_host = Some("localhost".into());
    options.state = None;
    let server = start_oauth_callback_server(options).await.unwrap();
    assert!(server.redirect_uri.starts_with("http://localhost:"));
    let url = with_params(
        &server.redirect_uri.replace("localhost", "127.0.0.1"),
        &[("code", "no-state")],
    );
    assert_eq!(http_get(&url).await.0, 200);
    assert_eq!(
        server.wait().await.unwrap().as_deref(),
        Some("completed:no-state")
    );
}

#[tokio::test]
async fn callback_server_failures() {
    let failing: CompleteFnString =
        Arc::new(|_| Box::pin(async { anyhow::bail!("token exchange failed") }));
    let server = start_oauth_callback_server(server_options(failing))
        .await
        .unwrap();
    let (status, body) = http_get(&with_params(
        &server.redirect_uri,
        &[("code", "c"), ("state", "expected-state")],
    ))
    .await;
    assert_eq!(status, 502);
    assert!(body.contains("Example sign-in failed.") && body.contains("token exchange failed"));
    assert_eq!(
        server.wait().await.unwrap_err().to_string(),
        "token exchange failed"
    );

    let server = start_oauth_callback_server(server_options(completing()))
        .await
        .unwrap();
    let url = with_params(
        &server.redirect_uri,
        &[
            ("error", "access_denied"),
            ("error_description", "User denied access"),
            ("state", "expected-state"),
        ],
    );
    let (status, body) = http_get(&url).await;
    assert_eq!(status, 400);
    assert!(body.contains("User denied access"));
    assert_eq!(
        server.wait().await.unwrap_err().to_string(),
        "Example authorization failed: User denied access"
    );
}

#[tokio::test]
async fn callback_server_completes_only_first_callback() {
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let release = Arc::new(parking_lot::Mutex::new(Some(release_rx)));
    let started = Arc::new(tokio::sync::Notify::new());
    let started_signal = started.clone();
    let complete: CompleteFnString = Arc::new(move |_| {
        let release = release.lock().take();
        let started = started_signal.clone();
        Box::pin(async move {
            started.notify_one();
            if let Some(release) = release {
                let _ = release.await;
            }
            Ok("done".to_string())
        })
    });
    let server = start_oauth_callback_server(server_options(complete))
        .await
        .unwrap();
    let url = with_params(
        &server.redirect_uri,
        &[("code", "c"), ("state", "expected-state")],
    );
    let first_url = url.clone();
    let first = tokio::spawn(async move { http_get(&first_url).await });
    started.notified().await;
    assert_eq!(http_get(&url).await.0, 409);
    // A claimed callback keeps completing even when the caller switches to manual input.
    server.cancel();
    release_tx.send(()).unwrap();
    assert_eq!(first.await.unwrap().0, 200);
    assert_eq!(server.wait().await.unwrap().as_deref(), Some("done"));
}

#[tokio::test]
async fn callback_server_cancel_abort_timeout_and_busy_port() {
    let server = start_oauth_callback_server(server_options(completing()))
        .await
        .unwrap();
    server.cancel();
    assert_eq!(server.wait().await.unwrap(), None);
    assert_eq!(
        http_get(&with_params(
            &server.redirect_uri,
            &[("code", "c"), ("state", "expected-state")]
        ))
        .await
        .0,
        409
    );

    let signal = CancellationToken::new();
    let mut options = server_options(completing());
    options.signal = Some(signal.clone());
    let aborted = start_oauth_callback_server(options).await.unwrap();
    signal.cancel();
    assert_eq!(
        aborted.wait().await.unwrap_err().to_string(),
        "Login cancelled"
    );

    let mut options = server_options(completing());
    options.timeout_ms = Some(10);
    let timed_out = start_oauth_callback_server(options).await.unwrap();
    assert_eq!(
        timed_out.wait().await.unwrap_err().to_string(),
        "Example sign-in timed out"
    );

    let mut options = server_options(completing());
    options.signal = Some(signal);
    assert_eq!(
        start_oauth_callback_server(options)
            .await
            .err()
            .unwrap()
            .to_string(),
        "Login cancelled"
    );

    let blocker = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut options = server_options(completing());
    options.port = blocker.local_addr().unwrap().port();
    let error = start_oauth_callback_server(options).await.err().unwrap();
    assert_eq!(
        error.downcast_ref::<std::io::Error>().unwrap().kind(),
        std::io::ErrorKind::AddrInUse
    );
}

#[tokio::test]
async fn wait_for_callback_or_manual_input_paths() {
    let plain: CompleteFnString = Arc::new(|code| Box::pin(async move { Ok(code) }));
    let mut options = server_options(plain.clone());
    options.state = None;
    let server = start_oauth_callback_server(options).await.unwrap();
    let (interaction_a, recorder) = interaction(pending_after_select("unused"), None, None);
    let url = with_params(&server.redirect_uri, &[("code", "from-browser")]);
    let waiter = wait_for_callback_or_manual_input(
        &interaction_a,
        Some(&server),
        "paste",
        &server.redirect_uri,
    );
    let (result, _) = tokio::join!(waiter, async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        http_get(&url).await
    });
    assert_eq!(
        result.unwrap(),
        CallbackOrManual::Callback("from-browser".to_string())
    );
    assert!(recorder.manual_signal().unwrap().is_cancelled());
    server.close();

    let mut options = server_options(plain.clone());
    options.state = None;
    let server = start_oauth_callback_server(options).await.unwrap();
    let (pasting, _) = interaction(answering("x", |_, _| Ok("pasted".into())), None, None);
    let result =
        wait_for_callback_or_manual_input(&pasting, Some(&server), "paste", &server.redirect_uri)
            .await
            .unwrap();
    assert_eq!(result, CallbackOrManual::Manual("pasted".into()));
    assert_eq!(server.wait().await.unwrap(), None);

    let result = wait_for_callback_or_manual_input::<String>(
        &pasting,
        None,
        "paste",
        "http://localhost/callback",
    )
    .await
    .unwrap();
    assert_eq!(result, CallbackOrManual::Manual("pasted".into()));

    let mut options = server_options(plain);
    options.state = None;
    let server = start_oauth_callback_server(options).await.unwrap();
    let (failing, _) = interaction(
        Arc::new(|_, _| Box::pin(async { anyhow::bail!("prompt cancelled") })),
        None,
        None,
    );
    let error =
        wait_for_callback_or_manual_input(&failing, Some(&server), "paste", &server.redirect_uri)
            .await
            .unwrap_err();
    assert_eq!(error.to_string(), "prompt cancelled");
}

// ---------------------------------------------------------------------------
// Device code polling
// ---------------------------------------------------------------------------

fn poll_options(
    interval: f64,
    expires: f64,
    wait_first: bool,
    signal: CancellationToken,
) -> OAuthDeviceCodePollOptions {
    OAuthDeviceCodePollOptions {
        interval_seconds: Some(interval),
        expires_in_seconds: Some(expires),
        wait_before_first_poll: wait_first,
        signal,
    }
}

#[tokio::test(start_paused = true)]
async fn device_code_polls_immediately_then_after_interval() {
    let start = tokio::time::Instant::now();
    let times = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = times.clone();
    let result = poll_oauth_device_code_flow(
        poll_options(2.0, 30.0, false, CancellationToken::new()),
        move || {
            let sink = sink.clone();
            async move {
                let mut list = sink.lock();
                list.push(start.elapsed().as_millis());
                Ok(if list.len() == 1 {
                    OAuthDeviceCodePollResult::Pending
                } else {
                    OAuthDeviceCodePollResult::Complete("token")
                })
            }
        },
    )
    .await
    .unwrap();
    assert_eq!(result, "token");
    assert_eq!(*times.lock(), vec![0, 2000]);
}

#[tokio::test(start_paused = true)]
async fn device_code_waits_before_first_poll_and_slows_down() {
    let start = tokio::time::Instant::now();
    let result = poll_oauth_device_code_flow(
        poll_options(2.0, 30.0, true, CancellationToken::new()),
        || async move {
            Ok(OAuthDeviceCodePollResult::Complete(
                start.elapsed().as_millis(),
            ))
        },
    )
    .await
    .unwrap();
    assert_eq!(result, 2000);

    for (slow_down, expected) in [(None, 7000u128), (Some(30.0), 30000)] {
        let start = tokio::time::Instant::now();
        let calls = Arc::new(parking_lot::Mutex::new(0));
        let counter = calls.clone();
        let result = poll_oauth_device_code_flow(
            poll_options(2.0, 900.0, false, CancellationToken::new()),
            move || {
                let counter = counter.clone();
                async move {
                    let mut n = counter.lock();
                    *n += 1;
                    Ok(if *n == 1 {
                        OAuthDeviceCodePollResult::SlowDown {
                            interval_seconds: slow_down,
                        }
                    } else {
                        OAuthDeviceCodePollResult::Complete(start.elapsed().as_millis())
                    })
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(result, expected);
    }
}

#[tokio::test(start_paused = true)]
async fn device_code_cancel_and_timeouts() {
    let signal = CancellationToken::new();
    let cancel = signal.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
    });
    let error =
        poll_oauth_device_code_flow::<(), _, _>(poll_options(5.0, 30.0, false, signal), || async {
            Ok(OAuthDeviceCodePollResult::Pending)
        })
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Login cancelled");

    let error = poll_oauth_device_code_flow::<(), _, _>(
        poll_options(5.0, 12.0, false, CancellationToken::new()),
        || async { Ok(OAuthDeviceCodePollResult::Pending) },
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "Device flow timed out");
    let error = poll_oauth_device_code_flow::<(), _, _>(
        poll_options(5.0, 12.0, false, CancellationToken::new()),
        || async {
            Ok(OAuthDeviceCodePollResult::SlowDown {
                interval_seconds: None,
            })
        },
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("Device flow timed out after one or more slow_down responses.")
    );
}

// ---------------------------------------------------------------------------
// Anthropic
// ---------------------------------------------------------------------------

fn anthropic_token_stub(expect_code: &'static str) -> FetchStub {
    stub_fetch(move |request, _| {
        assert_eq!(request.url, "https://platform.claude.com/v1/oauth/token");
        let body = request.json_body().unwrap();
        if body["grant_type"] == "authorization_code" {
            assert_eq!(body["code"], expect_code);
        }
        json_response(
            200,
            json!({ "access_token": "access-token", "refresh_token": "refresh-token", "expires_in": 3600 }),
        )
    })
}

#[tokio::test]
async fn anthropic_manual_callback_keeps_localhost_redirect() {
    let _guard = PORT_LOCK.lock().await;
    let stub = anthropic_token_stub("manual-code");
    let (login, _) = interaction(
        answering("browser", |_, recorder| {
            let url = recorder.auth_url().unwrap();
            let state = query_param(&url, "state").unwrap();
            let redirect = query_param(&url, "redirect_uri").unwrap();
            Ok(format!("{redirect}?code=manual-code&state={state}"))
        }),
        None,
        None,
    );
    let credential = with_oauth_fetch(
        stub.fetch.clone(),
        anthropic::AnthropicOAuth.login(login, None),
    )
    .await
    .unwrap();
    assert_eq!(
        (credential.access.as_str(), credential.refresh.as_str()),
        ("access-token", "refresh-token")
    );
    let requests = stub.requests.lock().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].json_body().unwrap()["redirect_uri"],
        "http://localhost:53692/callback"
    );
}

#[tokio::test]
async fn anthropic_copy_code_flow_and_select_prompt() {
    let stub = anthropic_token_stub("copied-code");
    let (login, recorder) = interaction(
        answering("copy_code", |_, recorder| {
            Ok(format!(
                "copied-code#{}",
                query_param(&recorder.auth_url().unwrap(), "state").unwrap()
            ))
        }),
        None,
        None,
    );
    let credential = with_oauth_fetch(
        stub.fetch.clone(),
        anthropic::AnthropicOAuth.login(login, None),
    )
    .await
    .unwrap();
    assert_eq!(credential.access, "access-token");
    let url = recorder.auth_url().unwrap();
    assert_eq!(
        query_param(&url, "redirect_uri").as_deref(),
        Some("https://platform.claude.com/oauth/code/callback")
    );
    let body = stub.requests.lock()[0].json_body().unwrap();
    assert_eq!(body["state"], json!(query_param(&url, "state").unwrap()));
    let prompts = recorder.prompts.lock().clone();
    assert_eq!(prompts[0].kind, AuthPromptKind::Select);
    assert_eq!(prompts[0].message, "Select Anthropic login method:");
    let options: Vec<(String, String)> = prompts[0]
        .options
        .iter()
        .map(|o| (o.id.clone(), o.label.clone()))
        .collect();
    assert_eq!(
        options,
        vec![
            ("browser".into(), "Browser login (default)".into()),
            ("copy_code".into(), "Copy code login (headless)".into())
        ]
    );
}

#[tokio::test]
async fn anthropic_cancelled_selection_and_refresh() {
    let (login, _) = interaction(
        Arc::new(|_, _| Box::pin(async { anyhow::bail!("Login cancelled") })),
        None,
        None,
    );
    assert_eq!(
        anthropic::AnthropicOAuth
            .login(login, None)
            .await
            .unwrap_err()
            .to_string(),
        "Login cancelled"
    );

    let stub = stub_fetch(|request, _| {
        let body = request.json_body().unwrap();
        assert_eq!(body["grant_type"], "refresh_token");
        assert_eq!(body["refresh_token"], "refresh-token");
        assert!(body.get("scope").is_none());
        assert!(body["client_id"].as_str().is_some_and(|c| !c.is_empty()));
        json_response(
            200,
            json!({ "access_token": "new-access-token", "refresh_token": "new-refresh-token", "expires_in": 3600 }),
        )
    });
    let refreshed = with_oauth_fetch(
        stub.fetch.clone(),
        anthropic::AnthropicOAuth.refresh(
            cred("old-access-token", "refresh-token", json!({})),
            CancellationToken::new(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        (refreshed.access.as_str(), refreshed.refresh.as_str()),
        ("new-access-token", "new-refresh-token")
    );
    assert!(refreshed.extra.is_empty());
}

#[tokio::test]
async fn anthropic_browser_callback_and_port_fallback() {
    let _guard = PORT_LOCK.lock().await;
    for block_preferred in [false, true] {
        let blocker = if block_preferred {
            tokio::net::TcpListener::bind("127.0.0.1:53692").await.ok()
        } else {
            None
        };
        let exchanged: Arc<parking_lot::Mutex<Option<Value>>> = Arc::default();
        let sink = exchanged.clone();
        let stub = stub_fetch(move |request, _| {
            *sink.lock() = request.json_body();
            json_response(
                200,
                json!({ "access_token": "access", "refresh_token": "refresh", "expires_in": 3600 }),
            )
        });
        let page: PageSlot = Arc::default();
        let page_sink = page.clone();
        let notify: NotifyHook = Arc::new(move |event| {
            if let AuthEvent::AuthUrl { url, .. } = event {
                let redirect = query_param(url, "redirect_uri").unwrap();
                let state = query_param(url, "state").unwrap();
                let mut callback = url::Url::parse(&redirect).unwrap();
                callback.set_host(Some("127.0.0.1")).unwrap();
                callback
                    .query_pairs_mut()
                    .append_pair("code", "browser-code")
                    .append_pair("state", &state);
                let target = callback.to_string();
                *page_sink.lock() = Some(tokio::spawn(async move { http_get(&target).await }));
            }
        });
        let (login, recorder) = interaction(pending_after_select("browser"), Some(notify), None);
        let credential = with_oauth_fetch(
            stub.fetch.clone(),
            anthropic::AnthropicOAuth.login(login, None),
        )
        .await
        .unwrap();
        assert_eq!(credential.access, "access");
        let redirect = query_param(&recorder.auth_url().unwrap(), "redirect_uri").unwrap();
        let exchanged = exchanged.lock().clone().unwrap();
        assert_eq!(exchanged["code"], "browser-code");
        assert_eq!(exchanged["redirect_uri"], json!(redirect));
        let parsed = url::Url::parse(&redirect).unwrap();
        assert_eq!(parsed.host_str(), Some("localhost"));
        assert_eq!(parsed.path(), "/callback");
        if block_preferred {
            assert_ne!(parsed.port(), Some(53692));
        }
        let handle = page.lock().take().unwrap();
        let (status, text) = handle.await.unwrap();
        assert_eq!(status, 200);
        assert!(text.contains("Signed in to Anthropic."));
        assert!(recorder.manual_signal().unwrap().is_cancelled());
        drop(blocker);
    }
}

// ---------------------------------------------------------------------------
// OpenAI Codex
// ---------------------------------------------------------------------------

fn codex_device_stub(
    access: String,
    poll_responses: Vec<anyhow::Result<pi_ai::auth::oauth::OAuthHttpResponse>>,
) -> FetchStub {
    let polls = Arc::new(parking_lot::Mutex::new(poll_responses));
    stub_fetch(move |request, _| match request.url.as_str() {
        "https://auth.openai.com/api/accounts/deviceauth/usercode" => {
            assert_eq!(
                request.header_value("Content-Type"),
                Some("application/json")
            );
            assert_eq!(
                request.json_body().unwrap(),
                json!({ "client_id": "app_EMoamEEZ73f0CkXaXp7hrann" })
            );
            json_response(
                200,
                json!({ "device_auth_id": "device-auth-id", "user_code": "ABCD-1234", "interval": "5" }),
            )
        }
        "https://auth.openai.com/api/accounts/deviceauth/token" => {
            assert_eq!(
                request.json_body().unwrap(),
                json!({ "device_auth_id": "device-auth-id", "user_code": "ABCD-1234" })
            );
            let mut polls = polls.lock();
            if polls.is_empty() {
                anyhow::bail!("Unexpected extra device auth poll")
            } else {
                polls.remove(0)
            }
        }
        "https://auth.openai.com/oauth/token" => {
            assert_eq!(
                request.form_field("grant_type").as_deref(),
                Some("authorization_code")
            );
            assert_eq!(request.form_field("code").as_deref(), Some("oauth-code"));
            assert_eq!(
                request.form_field("redirect_uri").as_deref(),
                Some("https://auth.openai.com/deviceauth/callback")
            );
            assert_eq!(
                request.form_field("code_verifier").as_deref(),
                Some("device-code-verifier")
            );
            json_response(
                200,
                json!({ "access_token": access, "refresh_token": "refresh-token", "expires_in": 3600 }),
            )
        }
        other => anyhow::bail!("Unexpected fetch URL: {other}"),
    })
}

fn device_pending() -> anyhow::Result<pi_ai::auth::oauth::OAuthHttpResponse> {
    json_response(
        403,
        json!({ "error": { "message": "pending", "type": "invalid_request_error", "code": "deviceauth_authorization_pending" } }),
    )
}

fn device_complete() -> anyhow::Result<pi_ai::auth::oauth::OAuthHttpResponse> {
    json_response(
        200,
        json!({ "authorization_code": "oauth-code", "code_challenge": "c", "code_verifier": "device-code-verifier" }),
    )
}

#[tokio::test(start_paused = true)]
async fn codex_device_code_login() {
    let access = access_token_for("account-123");
    let stub = codex_device_stub(access.clone(), vec![device_pending(), device_complete()]);
    let (login, recorder) = interaction(
        answering("device_code", |p, _| {
            anyhow::bail!("Unexpected prompt: {:?}", p.kind)
        }),
        None,
        None,
    );
    let started = tokio::time::Instant::now();
    let credential = with_oauth_fetch(
        stub.fetch.clone(),
        openai_codex::OpenAICodexOAuth.login(login, None),
    )
    .await
    .unwrap();
    assert_eq!(started.elapsed().as_millis(), 5000);
    assert_eq!(credential.access, access);
    assert_eq!(credential.refresh, "refresh-token");
    assert_eq!(
        credential.extra.get("accountId"),
        Some(&json!("account-123"))
    );
    assert_eq!(
        recorder.device_codes(),
        vec![AuthEvent::DeviceCode {
            user_code: "ABCD-1234".into(),
            verification_uri: "https://auth.openai.com/codex/device".into(),
            interval_seconds: Some(5),
            expires_in_seconds: Some(900),
        }]
    );
    let prompts = recorder.prompts.lock().clone();
    assert_eq!(prompts[0].message, "Select OpenAI Codex login method:");
    assert_eq!(prompts[0].options[0].id, "browser");
}

#[tokio::test(start_paused = true)]
async fn codex_device_code_pending_statuses_failures_and_timeout() {
    let access = access_token_for("a");
    let stub = codex_device_stub(
        access,
        vec![
            json_response(404, json!({})),
            json_response(403, json!({})),
            device_complete(),
        ],
    );
    let (login, _) = interaction(
        answering("device_code", |_, _| anyhow::bail!("unexpected")),
        None,
        None,
    );
    with_oauth_fetch(
        stub.fetch.clone(),
        openai_codex::OpenAICodexOAuth.login(login, None),
    )
    .await
    .unwrap();

    let stub = codex_device_stub(
        access_token_for("a"),
        vec![Ok(OAuthHttpResponse::new(500, "upstream exploded"))],
    );
    let (login, _) = interaction(
        answering("device_code", |_, _| anyhow::bail!("unexpected")),
        None,
        None,
    );
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        openai_codex::OpenAICodexOAuth.login(login, None),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "OpenAI Codex device auth failed with status 500: upstream exploded"
    );

    let pending: Vec<_> = (0..400).map(|_| device_pending()).collect();
    let stub = codex_device_stub(access_token_for("a"), pending);
    let (login, _) = interaction(
        answering("device_code", |_, _| anyhow::bail!("unexpected")),
        None,
        None,
    );
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        openai_codex::OpenAICodexOAuth.login(login, None),
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "Device flow timed out");

    let signal = CancellationToken::new();
    let cancel = signal.clone();
    let stub = codex_device_stub(
        access_token_for("a"),
        (0..10).map(|_| device_pending()).collect(),
    );
    let (login, _) = interaction(
        answering("device_code", |_, _| anyhow::bail!("unexpected")),
        None,
        Some(signal),
    );
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(2000)).await;
        cancel.cancel();
    });
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        openai_codex::OpenAICodexOAuth.login(login, None),
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "Login cancelled");
}

#[tokio::test]
async fn codex_refresh_errors_and_originator() {
    let stub = stub_fetch(|_, _| Ok(OAuthHttpResponse::new(401, "bad refresh")));
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        openai_codex::OpenAICodexOAuth.refresh(cred("a", "r", json!({})), CancellationToken::new()),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "OpenAI Codex token refresh failed (401): bad refresh"
    );

    let url = openai_codex::create_authorization_url("challenge", "state", "my-app");
    assert_eq!(query_param(&url, "originator").as_deref(), Some("my-app"));
    assert_eq!(
        query_param(&url, "id_token_add_organizations").as_deref(),
        Some("true")
    );
}

#[tokio::test]
async fn codex_browser_login_falls_back_to_pasted_redirect_when_port_taken() {
    let _guard = PORT_LOCK.lock().await;
    let blocker = tokio::net::TcpListener::bind("127.0.0.1:1455").await.ok();
    let access = access_token_for("account-789");
    let token = access.clone();
    let stub = stub_fetch(move |request, _| {
        assert_eq!(request.url, "https://auth.openai.com/oauth/token");
        assert_eq!(request.form_field("code").as_deref(), Some("pasted-code"));
        assert_eq!(
            request.form_field("redirect_uri").as_deref(),
            Some("http://localhost:1455/auth/callback")
        );
        json_response(
            200,
            json!({ "access_token": token, "refresh_token": "refresh", "expires_in": 3600 }),
        )
    });
    let (login, recorder) = interaction(
        answering("browser", |_, recorder| {
            let state = query_param(&recorder.auth_url().unwrap(), "state").unwrap();
            Ok(format!(
                "http://localhost:1455/auth/callback?code=pasted-code&state={state}"
            ))
        }),
        None,
        None,
    );
    let options = LoginOptions {
        agent_name: Some("my-app".into()),
        ..Default::default()
    };
    let credential = with_oauth_fetch(
        stub.fetch.clone(),
        openai_codex::OpenAICodexOAuth.login(login, Some(options)),
    )
    .await
    .unwrap();
    assert_eq!(
        credential.extra.get("accountId"),
        Some(&json!("account-789"))
    );
    assert_eq!(
        query_param(&recorder.auth_url().unwrap(), "originator").as_deref(),
        Some("my-app")
    );
    drop(blocker);
}

// ---------------------------------------------------------------------------
// OpenAI ChatGPT
// ---------------------------------------------------------------------------

const DEVICE_ID: &str = "e61bbe28-07ef-466d-8e5d-a344f94ab305";
const REQUIRED_SCOPE: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

fn chatgpt_token(scope: &str) -> Value {
    json!({ "access_token": "access-token", "refresh_token": "refresh-token", "expires_in": 3600, "id_token": "id-token", "scope": scope })
}

fn chatgpt_interaction(
    client_id: Option<&'static str>,
) -> (pi_ai::auth::ProviderAuthInteraction, Arc<Recorder>) {
    interaction(
        answering("unused", move |prompt, recorder| {
            assert_eq!(prompt.kind, AuthPromptKind::ManualCode);
            let url = recorder.auth_url().unwrap();
            let mut callback =
                url::Url::parse(&query_param(&url, "redirect_uri").unwrap()).unwrap();
            callback
                .query_pairs_mut()
                .append_pair("code", "authorization-code")
                .append_pair("state", &query_param(&url, "state").unwrap());
            if let Some(id) = client_id {
                callback.query_pairs_mut().append_pair("client_id", id);
            }
            Ok(callback.to_string())
        }),
        None,
        None,
    )
}

fn device_options(agent: Option<&str>) -> LoginOptions {
    LoginOptions {
        get_device_id: Some(Arc::new(|| DEVICE_ID.to_string())),
        agent_name: agent.map(str::to_string),
    }
}

#[tokio::test]
async fn chatgpt_login_registers_client_and_stores_scopes() {
    let _guard = PORT_LOCK.lock().await;
    let stub = stub_fetch(|request, _| {
        assert_eq!(
            request.url,
            "https://auth.openai.com/api/accounts/oauth/token"
        );
        json_response(200, chatgpt_token(REQUIRED_SCOPE))
    });
    let (login, recorder) = chatgpt_interaction(Some("oaiapp_issued"));
    let credential = with_oauth_fetch(
        stub.fetch.clone(),
        openai_chatgpt::OpenAIChatGPTOAuth.login(login, Some(device_options(None))),
    )
    .await
    .unwrap();
    let url = recorder.auth_url().unwrap();
    assert_eq!(
        query_param(&url, "client_id").as_deref(),
        Some("dynamic_agent_client")
    );
    assert_eq!(query_param(&url, "agent_name_hint").as_deref(), Some("Pi"));
    assert_eq!(
        query_param(&url, "ext_agent_host_id"),
        Some(format!("urn:uuid:{DEVICE_ID}"))
    );
    assert_eq!(query_param(&url, "scope").as_deref(), Some(REQUIRED_SCOPE));
    assert_eq!(
        query_param(&url, "redirect_uri").as_deref(),
        Some("http://127.0.0.1:1455/auth/callback")
    );
    assert_eq!(
        query_param(&url, "resource").as_deref(),
        Some("https://api.openai.com/v1")
    );
    let request = stub.requests.lock()[0].clone();
    assert_eq!(
        request.form_field("client_id").as_deref(),
        Some("oaiapp_issued")
    );
    assert_eq!(
        request.form_field("code").as_deref(),
        Some("authorization-code")
    );
    assert!(
        request
            .form_field("code_verifier")
            .is_some_and(|v| !v.is_empty())
    );
    assert_eq!(credential.access, "access-token");
    assert_eq!(
        credential.extra.get("clientId"),
        Some(&json!("oaiapp_issued"))
    );
    assert_eq!(
        credential.extra.get("scopes"),
        Some(&json!(REQUIRED_SCOPE.split(' ').collect::<Vec<_>>()))
    );
}

#[tokio::test]
async fn chatgpt_login_validation() {
    let _guard = PORT_LOCK.lock().await;
    let stub = stub_fetch(|_, _| json_response(200, chatgpt_token(REQUIRED_SCOPE)));
    let (login, recorder) = chatgpt_interaction(Some("oaiapp_issued"));
    with_oauth_fetch(
        stub.fetch.clone(),
        openai_chatgpt::OpenAIChatGPTOAuth.login(login, Some(device_options(Some("my-app")))),
    )
    .await
    .unwrap();
    assert_eq!(
        query_param(&recorder.auth_url().unwrap(), "agent_name_hint").as_deref(),
        Some("my-app")
    );

    let stub = stub_fetch(|_, _| json_response(200, chatgpt_token(REQUIRED_SCOPE)));
    let (login, _) = chatgpt_interaction(None);
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        openai_chatgpt::OpenAIChatGPTOAuth.login(login, Some(device_options(None))),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("registration callback did not contain an issued client ID")
    );
    assert!(stub.requests.lock().is_empty());

    let stub = stub_fetch(|_, _| {
        json_response(
            200,
            chatgpt_token("openid profile email offline_access resource.invoke"),
        )
    });
    let (login, _) = chatgpt_interaction(Some("oaiapp_issued"));
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        openai_chatgpt::OpenAIChatGPTOAuth.login(login, Some(device_options(None))),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("grant did not include chatgpt.tokens.use.direct")
    );

    let (login, recorder) = chatgpt_interaction(None);
    let error = openai_chatgpt::OpenAIChatGPTOAuth
        .login(login.clone(), None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("requires a device ID"));
    let bad = LoginOptions {
        get_device_id: Some(Arc::new(|| "not-a-uuid".to_string())),
        agent_name: None,
    };
    assert!(
        openai_chatgpt::OpenAIChatGPTOAuth
            .login(login, Some(bad))
            .await
            .unwrap_err()
            .to_string()
            .contains("requires a device ID")
    );
    assert!(recorder.auth_url().is_none());
}

#[tokio::test]
async fn chatgpt_refresh() {
    let existing = cred(
        "old-access",
        "old-refresh",
        json!({ "clientId": "oaiapp_existing", "scopes": REQUIRED_SCOPE.split(' ').collect::<Vec<_>>() }),
    );
    let mut without_refresh = chatgpt_token(REQUIRED_SCOPE);
    without_refresh
        .as_object_mut()
        .unwrap()
        .remove("refresh_token");
    let stub = stub_fetch(move |_, _| json_response(200, without_refresh.clone()));
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        openai_chatgpt::OpenAIChatGPTOAuth.refresh(existing.clone(), CancellationToken::new()),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("token response has invalid refresh_token")
    );

    let mut rotated = chatgpt_token(REQUIRED_SCOPE);
    rotated["access_token"] = json!("new-access");
    rotated["refresh_token"] = json!("new-refresh");
    let stub = stub_fetch(move |_, _| json_response(200, rotated.clone()));
    let before = pi_ai::utils::now_ms();
    let refreshed = with_oauth_fetch(
        stub.fetch.clone(),
        openai_chatgpt::OpenAIChatGPTOAuth.refresh(existing, CancellationToken::new()),
    )
    .await
    .unwrap();
    assert!(refreshed.expires >= before + (3600 - 180) * 1000);
    assert!(refreshed.expires <= pi_ai::utils::now_ms() + (3600 - 180) * 1000);
    let request = stub.requests.lock()[0].clone();
    assert_eq!(
        request.form_field("grant_type").as_deref(),
        Some("refresh_token")
    );
    assert_eq!(
        request.form_field("client_id").as_deref(),
        Some("oaiapp_existing")
    );
    assert_eq!(
        request.form_field("refresh_token").as_deref(),
        Some("old-refresh")
    );
    assert!(request.form_field("scope").is_none());
    assert_eq!(refreshed.access, "new-access");
    assert_eq!(
        refreshed.extra.get("clientId"),
        Some(&json!("oaiapp_existing"))
    );
}

// ---------------------------------------------------------------------------
// GitHub Copilot
// ---------------------------------------------------------------------------

fn copilot_token() -> &'static str {
    "tid=1;exp=2;proxy-ep=proxy.individual.githubcopilot.com;"
}

fn copilot_stub(models: Value, policy: impl Fn(&str) -> u16 + Send + Sync + 'static) -> FetchStub {
    stub_fetch(move |request, _| {
        let url = request.url.as_str();
        if url == "https://github.com/login/device/code" {
            assert_eq!(request.form_field("scope").as_deref(), Some("read:user"));
            return json_response(
                200,
                json!({ "device_code": "dc", "user_code": "UC-1", "verification_uri": "https://github.com/login/device", "interval": 1, "expires_in": 900 }),
            );
        }
        if url == "https://github.com/login/oauth/access_token" {
            assert_eq!(
                request.form_field("grant_type").as_deref(),
                Some("urn:ietf:params:oauth:grant-type:device_code")
            );
            return json_response(200, json!({ "access_token": "gho_token" }));
        }
        if url == "https://api.github.com/copilot_internal/v2/token" {
            assert_eq!(
                request.header_value("Authorization"),
                Some("Bearer gho_token")
            );
            return json_response(
                200,
                json!({ "token": copilot_token(), "expires_at": 2_000_000_000 }),
            );
        }
        if url == "https://api.individual.githubcopilot.com/models" {
            assert_eq!(
                request.header_value("X-GitHub-Api-Version"),
                Some("2026-06-01")
            );
            return json_response(200, models.clone());
        }
        if let Some(model) = url
            .strip_prefix("https://api.individual.githubcopilot.com/models/")
            .and_then(|r| r.strip_suffix("/policy"))
        {
            assert_eq!(request.json_body().unwrap(), json!({ "state": "enabled" }));
            return Ok(OAuthHttpResponse::new(policy(model), "{}"));
        }
        anyhow::bail!("Unexpected fetch: {url}")
    })
}

fn known_copilot_models() -> Vec<String> {
    pi_ai::models_generated::CATALOG
        .chat
        .get("github-copilot")
        .unwrap()
        .keys()
        .take(3)
        .cloned()
        .collect()
}

#[tokio::test(start_paused = true)]
async fn copilot_login_filters_picker_models_and_enables_policies() {
    let known = known_copilot_models();
    let models = json!({ "data": [
        { "id": known[0], "model_picker_enabled": true, "policy": { "state": "enabled" } },
        { "id": known[1], "model_picker_enabled": true, "policy": { "state": "unconfigured" } },
        { "id": known[2], "model_picker_enabled": true, "policy": { "state": "disabled" } },
        { "id": "unknown-model", "model_picker_enabled": true, "policy": { "state": "unconfigured" } },
        { "id": "no-tools", "model_picker_enabled": true, "capabilities": { "supports": { "tool_calls": false } } },
    ] });
    let stub = copilot_stub(models, |_| 200);
    let (login, recorder) = interaction(answering("unused", |_, _| Ok(String::new())), None, None);
    let credential = with_oauth_fetch(
        stub.fetch.clone(),
        github_copilot::GitHubCopilotOAuth.login(login, None),
    )
    .await
    .unwrap();
    assert_eq!(credential.refresh, "gho_token");
    assert_eq!(credential.access, copilot_token());
    assert_eq!(credential.expires, 2_000_000_000_000 - 300_000);
    assert_eq!(
        credential.extra.get("availableModelIds"),
        Some(&json!([known[0], known[1], "unknown-model"]))
    );
    assert!(credential.extra.get("enterpriseUrl").is_none());
    let policy_urls: Vec<String> = stub
        .requests
        .lock()
        .iter()
        .filter(|r| r.url.ends_with("/policy"))
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(
        policy_urls,
        vec![format!(
            "https://api.individual.githubcopilot.com/models/{}/policy",
            known[1]
        )]
    );
    assert_eq!(
        recorder.device_codes(),
        vec![AuthEvent::DeviceCode {
            user_code: "UC-1".into(),
            verification_uri: "https://github.com/login/device".into(),
            interval_seconds: Some(1),
            expires_in_seconds: Some(900),
        }]
    );
}

#[tokio::test(start_paused = true)]
async fn copilot_policy_fallback_and_untrusted_verification_uri() {
    let known = known_copilot_models();
    let models = json!({ "data": [
        { "id": known[0], "model_picker_enabled": false, "policy": { "state": "enabled" } },
        { "id": known[1], "model_picker_enabled": false, "policy": { "state": "unconfigured" } },
    ] });
    let stub = copilot_stub(models, |_| 200);
    let (login, _) = interaction(answering("unused", |_, _| Ok(String::new())), None, None);
    let credential = with_oauth_fetch(
        stub.fetch.clone(),
        github_copilot::GitHubCopilotOAuth.login(login, None),
    )
    .await
    .unwrap();
    assert_eq!(
        credential.extra.get("availableModelIds"),
        Some(&json!([known[0], known[1]]))
    );

    let stub = stub_fetch(|_, _| {
        json_response(
            200,
            json!({ "device_code": "dc", "user_code": "UC", "verification_uri": "file:///etc/passwd", "expires_in": 900 }),
        )
    });
    let (login, recorder) = interaction(answering("unused", |_, _| Ok(String::new())), None, None);
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        github_copilot::GitHubCopilotOAuth.login(login, None),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Untrusted verification_uri in device code response"
    );
    assert!(recorder.device_codes().is_empty());

    let (login, _) = interaction(answering("unused", |_, _| Ok("http://".into())), None, None);
    assert_eq!(
        github_copilot::GitHubCopilotOAuth
            .login(login, None)
            .await
            .unwrap_err()
            .to_string(),
        "Invalid GitHub Enterprise URL/domain"
    );
}

#[tokio::test]
async fn copilot_to_auth_and_refresh_preserve_enterprise() {
    let auth = github_copilot::GitHubCopilotOAuth
        .to_auth(&cred(
            "tid=1;proxy-ep=proxy.business.githubcopilot.com;",
            "r",
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(
        auth.base_url.as_deref(),
        Some("https://api.business.githubcopilot.com")
    );
    let auth = github_copilot::GitHubCopilotOAuth
        .to_auth(&cred(
            "opaque",
            "r",
            json!({ "enterpriseUrl": "https://ghe.example.com" }),
        ))
        .await
        .unwrap();
    assert_eq!(
        auth.base_url.as_deref(),
        Some("https://copilot-api.ghe.example.com")
    );
    let auth = github_copilot::GitHubCopilotOAuth
        .to_auth(&cred("opaque", "r", json!({})))
        .await
        .unwrap();
    assert_eq!(
        auth.base_url.as_deref(),
        Some("https://api.individual.githubcopilot.com")
    );
    assert_eq!(auth.api_key.as_deref(), Some("opaque"));

    let stub = stub_fetch(|request, _| match request.url.as_str() {
        "https://api.ghe.example.com/copilot_internal/v2/token" => json_response(
            200,
            json!({ "token": "enterprise-token", "expires_at": 2_000_000_000 }),
        ),
        "https://copilot-api.ghe.example.com/models" => json_response(429, json!({})),
        other => anyhow::bail!("Unexpected fetch: {other}"),
    });
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        github_copilot::GitHubCopilotOAuth.refresh(
            cred("old", "gho", json!({ "enterpriseUrl": "ghe.example.com" })),
            CancellationToken::new(),
        ),
    )
    .await
    .unwrap_err();
    // Model catalog throttling is not retried during refresh.
    assert!(error.to_string().starts_with("429"));
    assert_eq!(stub.requests.lock().len(), 2);

    let stub = stub_fetch(|request, _| match request.url.as_str() {
        "https://api.ghe.example.com/copilot_internal/v2/token" => json_response(
            200,
            json!({ "token": "enterprise-token", "expires_at": 2_000_000_000 }),
        ),
        "https://copilot-api.ghe.example.com/models" => json_response(
            200,
            json!({ "data": [{ "id": "m", "model_picker_enabled": true }] }),
        ),
        other => anyhow::bail!("Unexpected fetch: {other}"),
    });
    let refreshed = with_oauth_fetch(
        stub.fetch.clone(),
        github_copilot::GitHubCopilotOAuth.refresh(
            cred("old", "gho", json!({ "enterpriseUrl": "ghe.example.com" })),
            CancellationToken::new(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        refreshed.extra.get("enterpriseUrl"),
        Some(&json!("ghe.example.com"))
    );
    assert_eq!(
        refreshed.extra.get("availableModelIds"),
        Some(&json!(["m"]))
    );
}

// ---------------------------------------------------------------------------
// Kimi Code, Meta, xAI
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn kimi_device_login_refresh_and_errors() {
    let polls = Arc::new(parking_lot::Mutex::new(vec![
        json!({ "error": "authorization_pending" }),
        json!({ "access_token": "kimi-access", "refresh_token": "kimi-refresh", "expires_in": 3600 }),
    ]));
    let stub = stub_fetch(move |request, _| match request.url.as_str() {
        "https://auth.kimi.com/api/oauth/device_authorization" => json_response(
            200,
            json!({
                "device_code": "dc", "user_code": "KIMI-1", "verification_uri": "https://kimi.com/device",
                "verification_uri_complete": "https://kimi.com/device?code=KIMI-1", "interval": 5, "expires_in": 600
            }),
        ),
        "https://auth.kimi.com/api/oauth/token" => {
            let body = polls.lock().remove(0);
            json_response(
                if body.get("error").is_some() {
                    400
                } else {
                    200
                },
                body,
            )
        }
        other => anyhow::bail!("Unexpected fetch: {other}"),
    });
    let (login, recorder) = interaction(answering("unused", |_, _| Ok(String::new())), None, None);
    let credential = with_oauth_fetch(
        stub.fetch.clone(),
        kimi_coding::KimiCodingOAuth.login(login, None),
    )
    .await
    .unwrap();
    assert_eq!(
        (credential.access.as_str(), credential.refresh.as_str()),
        ("kimi-access", "kimi-refresh")
    );
    assert_eq!(
        recorder.device_codes(),
        vec![AuthEvent::DeviceCode {
            user_code: "KIMI-1".into(),
            verification_uri: "https://kimi.com/device?code=KIMI-1".into(),
            interval_seconds: Some(5),
            expires_in_seconds: Some(600),
        }]
    );
    let auth = kimi_coding::KimiCodingOAuth
        .to_auth(&credential)
        .await
        .unwrap();
    assert_eq!(
        auth.headers
            .unwrap()
            .get("Authorization")
            .cloned()
            .flatten()
            .as_deref(),
        Some("Bearer kimi-access")
    );

    for (error, message) in [
        (
            "expired_token",
            "Kimi Code device authorization expired. Please restart login.",
        ),
        ("access_denied", "Kimi Code login was denied."),
    ] {
        let stub = stub_fetch(move |request, _| {
            if request.url.ends_with("device_authorization") {
                json_response(
                    200,
                    json!({ "device_code": "dc", "user_code": "U", "verification_uri": "https://k/d", "verification_uri_complete": "https://k/d?c" }),
                )
            } else {
                json_response(400, json!({ "error": error }))
            }
        });
        let (login, _) = interaction(answering("unused", |_, _| Ok(String::new())), None, None);
        let result = with_oauth_fetch(
            stub.fetch.clone(),
            kimi_coding::KimiCodingOAuth.login(login, None),
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), message);
    }

    let attempts = Arc::new(parking_lot::Mutex::new(0));
    let counter = attempts.clone();
    let stub = stub_fetch(move |request, _| {
        assert_eq!(
            request.form_field("grant_type").as_deref(),
            Some("refresh_token")
        );
        let mut n = counter.lock();
        *n += 1;
        if *n == 1 {
            json_response(429, json!({}))
        } else {
            json_response(
                200,
                json!({ "access_token": "new", "refresh_token": "new-r", "expires_in": 60 }),
            )
        }
    });
    let refreshed = with_oauth_fetch(
        stub.fetch.clone(),
        kimi_coding::KimiCodingOAuth.refresh(cred("a", "r", json!({})), CancellationToken::new()),
    )
    .await
    .unwrap();
    assert_eq!(refreshed.access, "new");
    assert_eq!(*attempts.lock(), 2);

    let stub = stub_fetch(|_, _| {
        json_response(
            400,
            json!({ "error": "invalid_grant", "error_description": "revoked" }),
        )
    });
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        kimi_coding::KimiCodingOAuth.refresh(cred("a", "r", json!({})), CancellationToken::new()),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Kimi Code token refresh unauthorized (status 400): revoked"
    );
}

#[tokio::test(start_paused = true)]
async fn meta_device_login_and_key_mint() {
    let stub = stub_fetch(|request, _| match request.url.as_str() {
        "https://auth.meta.com/oidc/device/authorization/" => json_response(
            200,
            json!({
                "device_code": "dc", "user_code": "META-1", "verification_uri": "https://meta.com/device", "interval": 2, "expires_in": 600
            }),
        ),
        "https://auth.meta.com/oidc/device/token/" => {
            json_response(200, json!({ "access_token": "identity-token" }))
        }
        "https://api.meta.ai/muse-code/key" => {
            assert_eq!(
                request.header_value("Authorization"),
                Some("Bearer identity-token")
            );
            assert_eq!(request.header_value("x-api-version"), Some("1.0.0"));
            assert_eq!(request.body.as_deref(), Some("{}"));
            json_response(200, json!({ "api_key": "meta-key" }))
        }
        other => anyhow::bail!("Unexpected fetch: {other}"),
    });
    let (login, recorder) = interaction(answering("unused", |_, _| Ok(String::new())), None, None);
    let credential = with_oauth_fetch(stub.fetch.clone(), meta::MetaOAuth.login(login, None))
        .await
        .unwrap();
    assert_eq!(
        (credential.access.as_str(), credential.refresh.as_str()),
        ("meta-key", "identity-token")
    );
    assert!(recorder.events.lock().iter().any(|e| matches!(e, AuthEvent::Progress { message } if message == "Enabling Meta Model API access...")));
    assert_eq!(
        meta::MetaOAuth
            .to_auth(&credential)
            .await
            .unwrap()
            .api_key
            .as_deref(),
        Some("meta-key")
    );

    let stub =
        stub_fetch(|_, _| json_response(200, json!({ "action_url": "https://meta.ai/setup" })));
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        meta::MetaOAuth.refresh(credential.clone(), CancellationToken::new()),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Meta did not issue an API key. Complete setup at https://meta.ai/setup"
    );

    let stub = stub_fetch(|_, _| json_response(401, json!({ "detail": "expired" })));
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        meta::MetaOAuth.refresh(credential, CancellationToken::new()),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Meta session expired (status 401). Run `/login meta` to sign in again.: expired"
    );
}

#[tokio::test(start_paused = true)]
async fn xai_device_login_and_refresh() {
    let stub = stub_fetch(|request, _| match request.url.as_str() {
        "https://auth.x.ai/oauth2/device/code" => {
            assert_eq!(request.form_field("referrer").as_deref(), Some("pi"));
            json_response(
                200,
                json!({ "device_code": "dc", "user_code": "XAI-1", "verification_uri": "https://x.ai/device", "verification_uri_complete": "https://x.ai/device?c=1", "interval": 0, "expires_in": 600 }),
            )
        }
        "https://auth.x.ai/oauth2/token" => json_response(
            200,
            json!({ "access_token": "xai-access", "refresh_token": "xai-refresh" }),
        ),
        other => anyhow::bail!("Unexpected fetch: {other}"),
    });
    let (login, recorder) = interaction(answering("unused", |_, _| Ok(String::new())), None, None);
    let before = pi_ai::utils::now_ms();
    let credential = with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.login(login, None))
        .await
        .unwrap();
    assert_eq!(
        (credential.access.as_str(), credential.refresh.as_str()),
        ("xai-access", "xai-refresh")
    );
    assert!(credential.expires >= before + 3600 * 1000 - 300_000);
    assert_eq!(
        recorder.device_codes(),
        vec![AuthEvent::DeviceCode {
            user_code: "XAI-1".into(),
            verification_uri: "https://x.ai/device?c=1".into(),
            interval_seconds: None,
            expires_in_seconds: Some(600),
        }]
    );

    let stub = stub_fetch(|_, _| {
        json_response(200, json!({ "access_token": "rotated", "expires_in": 100 }))
    });
    let refreshed = with_oauth_fetch(
        stub.fetch.clone(),
        xai::XaiOAuth.refresh(cred("a", "keep-me", json!({})), CancellationToken::new()),
    )
    .await
    .unwrap();
    assert_eq!(
        (refreshed.access.as_str(), refreshed.refresh.as_str()),
        ("rotated", "keep-me")
    );

    let stub = stub_fetch(|_, _| {
        json_response(
            400,
            json!({ "error": "invalid_grant", "error_description": "nope" }),
        )
    });
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        xai::XaiOAuth.refresh(cred("a", "r", json!({})), CancellationToken::new()),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "xAI OAuth token refresh failed (HTTP 400): invalid_grant: nope"
    );

    let stub = stub_fetch(|_, _| {
        json_response(
            200,
            json!({ "device_code": "dc", "user_code": "U", "verification_uri": "http://insecure", "expires_in": 600 }),
        )
    });
    let (login, _) = interaction(answering("unused", |_, _| Ok(String::new())), None, None);
    let error = with_oauth_fetch(stub.fetch.clone(), xai::XaiOAuth.login(login, None))
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Untrusted verification URI in xAI OAuth response"
    );
}

// ---------------------------------------------------------------------------
// Radius
// ---------------------------------------------------------------------------

const GATEWAY: &str = "https://radius.example";

#[tokio::test(start_paused = true)]
async fn radius_device_login_refresh_and_discovery() {
    let stub = stub_fetch(|request, _| match request.url.as_str() {
        "https://radius.example/v1/oauth/device" => {
            assert_eq!(
                request.form_field("client_id").as_deref(),
                Some("pi-gateway")
            );
            assert_eq!(
                request.form_field("scope").as_deref(),
                Some("gateway offline_access")
            );
            json_response(
                200,
                json!({ "device_code": "device-code", "user_code": "ABCD-1234", "verification_uri": "https://radius-ui.example/pair", "expires_in": 600, "interval": 5 }),
            )
        }
        "https://radius.example/v1/oauth/token" => {
            assert_eq!(
                request.form_field("grant_type").as_deref(),
                Some("urn:ietf:params:oauth:grant-type:device_code")
            );
            json_response(
                200,
                json!({ "access_token": "access-token", "refresh_token": "refresh-token", "expires_in": 3600, "scope": "gateway offline_access" }),
            )
        }
        other => anyhow::bail!("Unexpected request: {other}"),
    });
    let oauth = radius::create_radius_oauth(radius::RadiusOAuthOptions {
        name: "Radius".into(),
        gateway: GATEWAY.into(),
    });
    let (login, recorder) = interaction(
        answering("device-code", |_, _| Ok(String::new())),
        None,
        None,
    );
    let credential = with_oauth_fetch(stub.fetch.clone(), oauth.login(login, None))
        .await
        .unwrap();
    assert_eq!(
        (credential.access.as_str(), credential.refresh.as_str()),
        ("access-token", "refresh-token")
    );
    assert_eq!(
        credential.extra.get("scope"),
        Some(&json!("gateway offline_access"))
    );
    assert_eq!(
        recorder.device_codes(),
        vec![AuthEvent::DeviceCode {
            user_code: "ABCD-1234".into(),
            verification_uri: "https://radius-ui.example/pair".into(),
            interval_seconds: Some(5),
            expires_in_seconds: Some(600),
        }]
    );
    let urls: Vec<String> = stub.requests.lock().iter().map(|r| r.url.clone()).collect();
    assert_eq!(
        urls,
        vec![
            "https://radius.example/v1/oauth/device",
            "https://radius.example/v1/oauth/token"
        ]
    );
    assert_eq!(recorder.prompts.lock()[0].message, "Sign in to Radius:");

    let stub = stub_fetch(|request, _| {
        assert_eq!(
            request.form_field("refresh_token").as_deref(),
            Some("old-refresh")
        );
        json_response(
            200,
            json!({ "access_token": "new-access", "refresh_token": "new-refresh", "expires_in": 3600 }),
        )
    });
    let refreshed = with_oauth_fetch(
        stub.fetch.clone(),
        oauth.refresh(
            cred("old", "old-refresh", json!({})),
            CancellationToken::new(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(refreshed.access, "new-access");
    assert!(refreshed.extra.is_empty());

    let stub =
        stub_fetch(|_, _| json_response(200, json!({ "issuer": "https://radius-ui.example" })));
    let (login, _) = interaction(answering("browser", |_, _| Ok(String::new())), None, None);
    let error = with_oauth_fetch(stub.fetch.clone(), oauth.login(login, None))
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Invalid Radius OAuth config from https://radius.example"
    );
}

#[tokio::test]
async fn radius_browser_login_exchanges_code_before_page() {
    let _guard = PORT_LOCK.lock().await;
    for (token_status, expect_ok) in [(400u16, false), (200, true)] {
        let stub = stub_fetch(move |request, _| match request.url.as_str() {
            "https://radius.example/v1/oauth" => json_response(
                200,
                json!({ "authorizationEndpoint": "https://radius.example/authorize" }),
            ),
            "https://radius.example/v1/oauth/token" => {
                assert_eq!(request.form_field("code").as_deref(), Some("browser-code"));
                if token_status == 200 {
                    json_response(
                        200,
                        json!({ "access_token": "access", "refresh_token": "refresh", "expires_in": 3600 }),
                    )
                } else {
                    json_response(
                        token_status,
                        json!({ "error": "invalid_grant", "error_description": "code expired" }),
                    )
                }
            }
            other => anyhow::bail!("Unexpected request: {other}"),
        });
        let page: PageSlot = Arc::default();
        let page_sink = page.clone();
        let notify: NotifyHook = Arc::new(move |event| {
            if let AuthEvent::AuthUrl { url, .. } = event {
                let mut callback =
                    url::Url::parse(&query_param(url, "redirect_uri").unwrap()).unwrap();
                callback
                    .query_pairs_mut()
                    .append_pair("code", "browser-code")
                    .append_pair("state", &query_param(url, "state").unwrap());
                let target = callback.to_string();
                *page_sink.lock() = Some(tokio::spawn(async move { http_get(&target).await }));
            }
        });
        let oauth = radius::create_radius_oauth(radius::RadiusOAuthOptions {
            name: "Radius".into(),
            gateway: GATEWAY.into(),
        });
        let (login, recorder) = interaction(
            answering("browser", |_, _| Ok(String::new())),
            Some(notify),
            None,
        );
        let result = with_oauth_fetch(stub.fetch.clone(), oauth.login(login, None)).await;
        let (status, body) = take_page(&page).await.unwrap();
        let auth_url = recorder.auth_url().unwrap();
        assert!(auth_url.starts_with("https://radius.example/authorize?"));
        assert_eq!(query_param(&auth_url, "handoff").as_deref(), Some("url"));
        if expect_ok {
            assert_eq!(result.unwrap().access, "access");
            assert_eq!(status, 200);
        } else {
            assert_eq!(
                result.unwrap_err().to_string(),
                "Radius OAuth token request failed: invalid_grant: code expired"
            );
            assert_eq!(status, 502);
            assert!(body.contains("code expired"));
        }
    }
}

// ---------------------------------------------------------------------------
// OpenRouter
// ---------------------------------------------------------------------------

fn openrouter_callback_notify(code: &'static str) -> (NotifyHook, PageSlot) {
    let page: PageSlot = Arc::default();
    let sink = page.clone();
    let notify: NotifyHook = Arc::new(move |event| {
        if let AuthEvent::AuthUrl { url, .. } = event {
            let mut callback = url::Url::parse(&query_param(url, "callback_url").unwrap()).unwrap();
            callback.query_pairs_mut().append_pair("code", code);
            let target = callback.to_string();
            *sink.lock() = Some(tokio::spawn(async move { http_get(&target).await }));
        }
    });
    (notify, page)
}

#[tokio::test]
async fn openrouter_loopback_pkce_login() {
    let stub = stub_fetch(|request, _| {
        assert_eq!(request.url, "https://openrouter.ai/api/v1/auth/keys");
        json_response(200, json!({ "key": "sk-or-test" }))
    });
    let (notify, page) = openrouter_callback_notify("authorization-code");
    let (login, recorder) = interaction(pending_after_select("unused"), Some(notify), None);
    let credential = with_oauth_fetch(
        stub.fetch.clone(),
        openrouter::OpenRouterOAuth.login(login, None),
    )
    .await
    .unwrap();
    assert_eq!(
        credential,
        OAuthCredential {
            access: "sk-or-test".into(),
            refresh: String::new(),
            expires: 9_007_199_254_740_991,
            extra: Map::new()
        }
    );
    assert_eq!(take_page(&page).await.unwrap().0, 200);
    assert!(recorder.manual_signal().unwrap().is_cancelled());
    let auth_url = url::Url::parse(&recorder.auth_url().unwrap()).unwrap();
    assert_eq!(
        auth_url.origin().ascii_serialization(),
        "https://openrouter.ai"
    );
    assert_eq!(auth_url.path(), "/auth");
    let callback =
        url::Url::parse(&query_param(auth_url.as_str(), "callback_url").unwrap()).unwrap();
    assert_eq!(callback.host_str(), Some("127.0.0.1"));
    assert!(callback.path().starts_with("/oauth/callback/"));
    let body = stub.requests.lock()[0].json_body().unwrap();
    assert_eq!(body["code"], "authorization-code");
    assert_eq!(body["code_challenge_method"], "S256");
    let verifier = body["code_verifier"].as_str().unwrap();
    assert_eq!(
        query_param(auth_url.as_str(), "code_challenge").unwrap(),
        pkce::pkce_challenge(verifier)
    );
}

#[tokio::test]
async fn openrouter_exchange_failures_and_manual_input() {
    let stub =
        stub_fetch(|_, _| json_response(403, json!({ "error": { "message": "invalid code" } })));
    let (notify, page) = openrouter_callback_notify("bad-code");
    let (login, _) = interaction(pending_after_select("unused"), Some(notify), None);
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        openrouter::OpenRouterOAuth.login(login, None),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "OpenRouter OAuth key exchange failed (HTTP 403): invalid code"
    );
    assert_eq!(take_page(&page).await.unwrap().0, 502);

    let stub = stub_fetch(|_, _| json_response(200, json!({ "user_id": "user-1" })));
    let (notify, page) = openrouter_callback_notify("code-without-key");
    let (login, _) = interaction(pending_after_select("unused"), Some(notify), None);
    let error = with_oauth_fetch(
        stub.fetch.clone(),
        openrouter::OpenRouterOAuth.login(login, None),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "OpenRouter OAuth response carries no \"key\""
    );
    assert_eq!(take_page(&page).await.unwrap().0, 502);

    for answer in ["manual", "bare"] {
        let stub = stub_fetch(|_, _| json_response(200, json!({ "key": "sk-or-manual" })));
        let (login, _) = interaction(
            answering("unused", move |_, recorder| {
                if answer == "bare" {
                    return Ok("manual-code".into());
                }
                Ok(format!(
                    "{}?code=manual-code",
                    query_param(&recorder.auth_url().unwrap(), "callback_url").unwrap()
                ))
            }),
            None,
            None,
        );
        let credential = with_oauth_fetch(
            stub.fetch.clone(),
            openrouter::OpenRouterOAuth.login(login, None),
        )
        .await
        .unwrap();
        assert_eq!(credential.access, "sk-or-manual");
        assert_eq!(
            stub.requests.lock()[0].json_body().unwrap()["code"],
            "manual-code"
        );
    }

    let (login, _) = interaction(
        Arc::new(|_, _| Box::pin(async { anyhow::bail!("Login cancelled") })),
        None,
        None,
    );
    assert_eq!(
        openrouter::OpenRouterOAuth
            .login(login, None)
            .await
            .unwrap_err()
            .to_string(),
        "Login cancelled"
    );
    let kept = openrouter::OpenRouterOAuth
        .refresh(cred("k", "", json!({})), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(kept.access, "k");
}

// ---------------------------------------------------------------------------
// Registry and toAuth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn builtin_flows_resolve_through_lazy_oauth() {
    let subscriptions: Vec<(&str, Option<bool>)> = BUILTIN_OAUTH_FLOW_KEYS
        .iter()
        .map(|key| {
            let args = pi_ai::auth::oauth_load::OAuthFlowArgs {
                name: Some("Radius".into()),
                gateway: Some(GATEWAY.into()),
            };
            (
                *key,
                pi_ai::auth::oauth_load::load_oauth_flow(key, &args)
                    .unwrap()
                    .is_subscription(),
            )
        })
        .collect();
    for (key, subscription) in subscriptions {
        let expected = !matches!(key, "openrouter" | "radius");
        assert_eq!(subscription == Some(true), expected, "{key}");
    }
    let lazy = pi_ai::auth::oauth_load::lazy_builtin_oauth(
        "Anthropic (Claude Pro/Max)",
        Some(true),
        None,
        "anthropic",
        Default::default(),
    );
    let auth = lazy
        .to_auth(&cred("sk-ant-oat-access", "r", json!({})))
        .await
        .unwrap();
    assert_eq!(auth.api_key.as_deref(), Some("sk-ant-oat-access"));
    for flow in [
        openai_codex::openai_codex_oauth(),
        xai::xai_oauth(),
        openrouter::openrouter_oauth(),
    ] {
        assert_eq!(
            flow.to_auth(&cred("tok", "r", json!({})))
                .await
                .unwrap()
                .api_key
                .as_deref(),
            Some("tok")
        );
    }
    assert_eq!(
        openai_chatgpt::openai_chatgpt_oauth().login_label(),
        Some("Sign in with ChatGPT")
    );
}

#[tokio::test]
async fn models_get_auth_resolves_stored_oauth_through_lazy_flows() {
    use pi_ai::auth::{Credential, CredentialStore, InMemoryCredentialStore};
    use pi_ai::providers::anthropic::anthropic_provider;
    use pi_ai::providers::builtin::{github_copilot_provider, openai_provider};

    let openai = openai_provider();
    let oauth = openai.auth().oauth.clone().unwrap();
    assert!(openai.auth().api_key.is_some());
    assert_eq!(oauth.name(), "OpenAI (ChatGPT subscription)");
    assert_eq!(oauth.is_subscription(), Some(true));
    assert_eq!(oauth.login_label(), Some("Sign in with ChatGPT"));

    let expires = pi_ai::utils::now_ms() + 10 * 60_000;
    for (provider, access, base_url) in [
        (anthropic_provider(), "oauth-access-token".to_string(), None),
        (
            github_copilot_provider(),
            "tid=abc;exp=123;proxy-ep=proxy.business.githubcopilot.com;rest".to_string(),
            Some("https://api.business.githubcopilot.com"),
        ),
    ] {
        let store = Arc::new(InMemoryCredentialStore::new());
        let stored = OAuthCredential {
            access: access.clone(),
            refresh: "r".into(),
            expires,
            extra: Map::new(),
        };
        store
            .modify(
                provider.id(),
                Box::new(move |_| Box::pin(async move { Ok(Some(Credential::OAuth(stored))) })),
                None,
            )
            .await
            .unwrap();
        let models = pi_ai::create_models(pi_ai::CreateModelsOptions {
            credentials: Some(store),
            ..Default::default()
        });
        let id = provider.id().to_string();
        models.set_provider(provider);
        let result = models
            .get_auth(&id, Default::default())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.auth.api_key.as_deref(), Some(access.as_str()));
        assert_eq!(result.auth.base_url.as_deref(), base_url);
        assert_eq!(result.source.as_deref(), Some("OAuth"));
    }
}
