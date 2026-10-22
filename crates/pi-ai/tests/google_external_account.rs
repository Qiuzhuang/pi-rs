//! Workload/workforce identity federation (`external_account`, `external_account_authorized_user`)
//! credentials, reproducing `google-auth-library` 10.6.2 against local mock endpoints.

mod mock_http;

use mock_http::{MockServer, Response};
use pi_ai::api::google_auth::{GoogleCredentials, VERTEX_AI_SCOPE, get_request_headers};
use serde_json::{Value, json};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pi-ai-gext-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn form(request: &mock_http::Request) -> Vec<(String, String)> {
    url::form_urlencoded::parse(&request.body).into_owned().collect()
}

fn form_value(request: &mock_http::Request, key: &str) -> Option<String> {
    form(request).into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

async fn headers_for(config: Value) -> anyhow::Result<Vec<(String, String)>> {
    let credentials = GoogleCredentials::from_json(&config)?;
    get_request_headers(&credentials, VERTEX_AI_SCOPE, &reqwest::Client::new()).await
}

#[tokio::test]
async fn file_source_exchanges_the_subject_token_at_sts() {
    let sts = MockServer::start(|_, _| {
        Response::json(200, json!({ "access_token": "sts-token", "issued_token_type": "urn:ietf:params:oauth:token-type:access_token", "token_type": "Bearer", "expires_in": 3600 }))
    })
    .await;
    let dir = temp_dir("file");
    let token_file = dir.join("token.txt");
    std::fs::write(&token_file, "subject-jwt").unwrap();
    let audience = "//iam.googleapis.com/projects/123/locations/global/workloadIdentityPools/pool/providers/prov";
    let headers = headers_for(json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": "urn:ietf:params:oauth:token-type:jwt",
        "token_url": format!("{}/v1/token", sts.url),
        "credential_source": { "file": token_file.to_string_lossy() },
        "quota_project_id": "quota"
    }))
    .await
    .unwrap();
    assert_eq!(
        headers,
        vec![("authorization".to_string(), "Bearer sts-token".to_string()), ("x-goog-user-project".into(), "quota".into())]
    );
    let request = sts.request(0);
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/token");
    assert_eq!(request.header("content-type"), Some("application/x-www-form-urlencoded;charset=UTF-8"));
    assert_eq!(
        request.header("x-goog-api-client"),
        Some("gl-node/22.19.0 auth/10.6.2 google-byoid-sdk source/file sa-impersonation/false config-lifetime/false")
    );
    assert_eq!(request.header("user-agent"), Some("google-api-nodejs-client/10.6.2"));
    assert!(request.header("authorization").is_none());
    assert_eq!(
        form(&request),
        vec![
            ("grant_type".to_string(), "urn:ietf:params:oauth:grant-type:token-exchange".to_string()),
            ("audience".into(), audience.into()),
            ("scope".into(), "https://www.googleapis.com/auth/cloud-platform".into()),
            ("requested_token_type".into(), "urn:ietf:params:oauth:token-type:access_token".into()),
            ("subject_token".into(), "subject-jwt".into()),
            ("subject_token_type".into(), "urn:ietf:params:oauth:token-type:jwt".into()),
        ]
    );

    // Each request builds a new GoogleAuth in TS, so nothing is cached across requests.
    headers_for(json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": "urn:ietf:params:oauth:token-type:jwt",
        "token_url": format!("{}/v1/token", sts.url),
        "credential_source": { "file": token_file.to_string_lossy() },
        "quota_project_id": "quota"
    }))
    .await
    .unwrap();
    assert_eq!(sts.requests().len(), 2);
}

#[tokio::test]
async fn url_source_with_json_format_client_auth_and_impersonation() {
    let server = MockServer::start(|request, _| match request.path_only() {
        "/subject" => Response::json(200, json!({ "id_token": "url-jwt" })),
        "/token" => Response::json(200, json!({ "access_token": "federated", "expires_in": 3600 })),
        _ => Response::json(200, json!({ "accessToken": "impersonated", "expireTime": "2099-01-01T00:00:00Z" })),
    })
    .await;
    let audience = "//iam.googleapis.com/locations/global/workforcePools/pool/providers/prov";
    let impersonation =
        format!("{}/v1/projects/-/serviceAccounts/sa@p.iam.gserviceaccount.com:generateAccessToken", server.url);
    let headers = headers_for(json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": "urn:ietf:params:oauth:token-type:id_token",
        "tokenUrl": format!("{}/token", server.url),
        "client_id": "cid",
        "client_secret": "secret",
        "workforce_pool_user_project": "user-project",
        "service_account_impersonation_url": impersonation,
        "service_account_impersonation": { "token_lifetime_seconds": 600 },
        "credential_source": {
            "url": format!("{}/subject", server.url),
            "headers": { "Metadata": "True" },
            "format": { "type": "json", "subject_token_field_name": "id_token" }
        }
    }))
    .await
    .unwrap();
    assert_eq!(headers, vec![("authorization".to_string(), "Bearer impersonated".to_string())]);

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].header("metadata"), Some("True"));
    let sts = &requests[1];
    assert_eq!(sts.header("authorization"), Some("Basic Y2lkOnNlY3JldA=="));
    // Impersonation exchanges with the cloud-platform scope; client auth replaces `options.userProject`.
    assert_eq!(form_value(sts, "scope").as_deref(), Some("https://www.googleapis.com/auth/cloud-platform"));
    assert_eq!(form_value(sts, "options"), None);
    assert_eq!(
        sts.header("x-goog-api-client"),
        Some("gl-node/22.19.0 auth/10.6.2 google-byoid-sdk source/url sa-impersonation/true config-lifetime/true")
    );
    let impersonate = &requests[2];
    assert_eq!(impersonate.header("authorization"), Some("Bearer federated"));
    assert_eq!(impersonate.json(), json!({ "scope": ["https://www.googleapis.com/auth/cloud-platform"], "lifetime": "600s" }));
}

#[tokio::test]
async fn workforce_user_project_and_sts_errors() {
    let server = MockServer::start(|request, _| {
        if form_value(request, "subject_token").as_deref() == Some("bad") {
            Response::json(400, json!({ "error": "invalid_grant", "error_description": "The subject token is invalid." }))
        } else {
            Response::json(200, json!({ "access_token": "ok" }))
        }
    })
    .await;
    let dir = temp_dir("workforce");
    let good = dir.join("good.json");
    std::fs::write(&good, r#"{"token":"good"}"#).unwrap();
    let bad = dir.join("bad.txt");
    std::fs::write(&bad, "bad").unwrap();
    let audience = "//iam.googleapis.com/locations/global/workforcePools/pool/providers/prov";
    let config = |file: &std::path::Path, format: Value| {
        json!({
            "type": "external_account",
            "audience": audience,
            "subject_token_type": "urn:ietf:params:oauth:token-type:id_token",
            "token_url": format!("{}/token", server.url),
            "workforce_pool_user_project": "user-project",
            "credential_source": { "file": file.to_string_lossy(), "format": format }
        })
    };
    let headers = headers_for(config(&good, json!({ "type": "json", "subject_token_field_name": "token" }))).await.unwrap();
    assert_eq!(headers[0].1, "Bearer ok");
    assert_eq!(form_value(&server.request(0), "options").as_deref(), Some(r#"{"userProject":"user-project"}"#));

    let error = headers_for(config(&bad, json!({ "type": "text" }))).await.unwrap_err();
    assert_eq!(error.to_string(), "Error code invalid_grant: The subject token is invalid.");
}

#[tokio::test]
async fn validates_external_account_configs_like_the_auth_library() {
    let base = |extra: Value| {
        let mut config = json!({
            "type": "external_account",
            "audience": "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/x",
            "subject_token_type": "urn:ietf:params:oauth:token-type:jwt",
        });
        for (k, v) in extra.as_object().unwrap() {
            config[k] = v.clone();
        }
        config
    };
    let error = |config: Value| GoogleCredentials::from_json(&config).unwrap_err().to_string();
    assert_eq!(
        error(base(json!({ "workforce_pool_user_project": "p", "credential_source": { "file": "/x" } }))),
        "workforcePoolUserProject should not be set for non-workforce pool credentials."
    );
    assert_eq!(
        error(base(json!({ "credential_source": { "file": "/x", "format": { "type": "xml" } } }))),
        "Invalid credential_source format \"xml\""
    );
    assert_eq!(
        error(base(json!({ "credential_source": { "file": "/x", "format": { "type": "json" } } }))),
        "Missing subject_token_field_name for JSON credential_source format"
    );
    assert_eq!(
        error(base(json!({ "credential_source": { "file": "/x", "url": "https://y" } }))),
        "No valid Identity Pool \"credential_source\" provided, must be either file, url, or certificate."
    );
    assert_eq!(error(base(json!({}))), "A credential source or subject token supplier must be specified.");
    assert_eq!(
        error(base(json!({ "credential_source": { "environment_id": "aws2", "regional_cred_verification_url": "https://sts" } }))),
        "aws version \"2\" is not supported in the current build."
    );
    assert_eq!(
        error(base(json!({ "credential_source": { "environment_id": "aws1" } }))),
        "No valid AWS \"credential_source\" provided"
    );
    assert_eq!(
        error(base(json!({ "credential_source": { "executable": { "command": "run", "timeout_millis": 1000 } } }))),
        "Timeout must be between 5000 and 120000 milliseconds."
    );
    assert_eq!(
        error(base(json!({ "credential_source": { "certificate": {} } }))),
        "Either `useDefaultCertificateConfig` must be true or a `certificateConfigLocation` must be provided."
    );
    assert_eq!(
        error(base(json!({ "credential_source": { "certificate": { "use_default_certificate_config": false } } }))),
        "Either `useDefaultCertificateConfig` must be true or a `certificateConfigLocation` must be provided."
    );
    assert_eq!(
        error(base(json!({ "credential_source": { "certificate": { "certificate_config_location": "" , "trust_chain_path": "/x" } } }))),
        "Either `useDefaultCertificateConfig` must be true or a `certificateConfigLocation` must be provided."
    );

    let missing = headers_for(base(json!({ "credential_source": { "file": "/definitely/missing/token" } }))).await.unwrap_err();
    assert_eq!(
        missing.to_string(),
        "The file at /definitely/missing/token does not exist, or it is not a file. ENOENT: no such file or directory, lstat '/definitely'"
    );
}

#[tokio::test]
async fn external_account_authorized_user_refreshes_with_basic_auth() {
    let sts = MockServer::start(|_, _| Response::json(200, json!({ "access_token": "workforce", "expires_in": 3600 }))).await;
    let headers = headers_for(json!({
        "type": "external_account_authorized_user",
        "audience": "//iam.googleapis.com/locations/global/workforcePools/pool/providers/prov",
        "refresh_token": "refresh",
        "client_id": "cid",
        "client_secret": "csecret",
        "token_url": format!("{}/v1/oauthtoken", sts.url),
        "quota_project_id": "quota"
    }))
    .await
    .unwrap();
    assert_eq!(headers[0], ("authorization".to_string(), "Bearer workforce".to_string()));
    assert_eq!(headers[1], ("x-goog-user-project".to_string(), "quota".to_string()));
    let request = sts.request(0);
    assert_eq!(request.path, "/v1/oauthtoken");
    assert_eq!(request.header("authorization"), Some("Basic Y2lkOmNzZWNyZXQ="));
    assert_eq!(
        form(&request),
        vec![("grant_type".to_string(), "refresh_token".to_string()), ("refresh_token".into(), "refresh".into())]
    );
}

/// Executable and AWS sources read process environment variables, so they share one test.
#[cfg(unix)]
#[tokio::test]
async fn executable_and_aws_sources() {
    let server = MockServer::start(|request, _| match (request.method.as_str(), request.path_only()) {
        ("PUT", "/imds/token") => Response::new(200, "text/plain", "imds-session"),
        ("GET", "/zone") => Response::new(200, "text/plain", "us-east-2b"),
        ("GET", "/creds") => Response::new(200, "text/plain", "role"),
        ("GET", "/creds/role") => {
            Response::json(200, json!({ "AccessKeyId": "AKID", "SecretAccessKey": "SECRET", "Token": "SESSION" }))
        }
        _ => Response::json(200, json!({ "access_token": "sts", "expires_in": 3600 })),
    })
    .await;
    let dir = temp_dir("exec");
    let script = dir.join("token.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '{\"version\":1,\"success\":true,\"token_type\":\"urn:ietf:params:oauth:token-type:jwt\",\"id_token\":\"exec-%s\",\"expiration_time\":9999999999}' \"$GOOGLE_EXTERNAL_ACCOUNT_INTERACTIVE\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let failing = dir.join("fail.sh");
    std::fs::write(&failing, "#!/bin/sh\necho '{\"version\":1,\"success\":false,\"code\":\"401\",\"message\":\"denied\"}'\n").unwrap();
    std::fs::set_permissions(&failing, std::fs::Permissions::from_mode(0o755)).unwrap();
    let executable = |command: &std::path::Path| {
        json!({
            "type": "external_account",
            "audience": "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/x",
            "subject_token_type": "urn:ietf:params:oauth:token-type:jwt",
            "token_url": format!("{}/token", server.url),
            "credential_source": { "executable": { "command": command.to_string_lossy() } }
        })
    };

    // SAFETY: this is the only test in this binary that reads these variables.
    unsafe { std::env::remove_var("GOOGLE_EXTERNAL_ACCOUNT_ALLOW_EXECUTABLES") };
    assert_eq!(
        headers_for(executable(&script)).await.unwrap_err().to_string(),
        "Pluggable Auth executables need to be explicitly allowed to run by setting the GOOGLE_EXTERNAL_ACCOUNT_ALLOW_EXECUTABLES environment Variable to 1."
    );
    unsafe { std::env::set_var("GOOGLE_EXTERNAL_ACCOUNT_ALLOW_EXECUTABLES", "1") };
    let headers = headers_for(executable(&script)).await.unwrap();
    assert_eq!(headers[0].1, "Bearer sts");
    let sts_request = server.requests().into_iter().find(|r| r.path_only() == "/token").unwrap();
    assert_eq!(form_value(&sts_request, "subject_token").as_deref(), Some("exec-0"));
    assert!(sts_request.header("x-goog-api-client").unwrap().contains("source/executable"));
    assert_eq!(
        headers_for(executable(&failing)).await.unwrap_err().to_string(),
        "The executable failed with exit code: 401 and error message: denied."
    );

    // AWS: region from the metadata zone, credentials from the metadata role, IMDSv2 session token.
    for name in ["AWS_REGION", "AWS_DEFAULT_REGION", "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN"] {
        unsafe { std::env::remove_var(name) };
    }
    let audience = "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/aws";
    let headers = headers_for(json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": "urn:ietf:params:aws:token-type:aws4_request",
        "token_url": format!("{}/aws-token", server.url),
        "credential_source": {
            "environment_id": "aws1",
            "region_url": format!("{}/zone", server.url),
            "url": format!("{}/creds", server.url),
            "imdsv2_session_token_url": format!("{}/imds/token", server.url),
            "regional_cred_verification_url": "https://sts.{region}.amazonaws.com?Action=GetCallerIdentity&Version=2011-06-15"
        }
    }))
    .await
    .unwrap();
    assert_eq!(headers[0].1, "Bearer sts");
    let requests = server.requests();
    let zone = requests.iter().find(|r| r.path_only() == "/zone").unwrap();
    assert_eq!(zone.header("x-aws-ec2-metadata-token"), Some("imds-session"));
    let token_request = requests.iter().find(|r| r.path_only() == "/aws-token").unwrap();
    let subject = form_value(token_request, "subject_token").unwrap();
    let decoded: Value =
        serde_json::from_str(&percent_decode(&subject)).expect("subject token is URI-encoded JSON");
    assert_eq!(decoded["url"], "https://sts.us-east-2.amazonaws.com?Action=GetCallerIdentity&Version=2011-06-15");
    assert_eq!(decoded["method"], "POST");
    let keys: Vec<&str> = decoded["headers"].as_array().unwrap().iter().map(|h| h["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["authorization", "host", "x-amz-date", "x-amz-security-token", "x-goog-cloud-target-resource"]);
    let header = |name: &str| {
        decoded["headers"].as_array().unwrap().iter().find(|h| h["key"] == name).unwrap()["value"].as_str().unwrap().to_string()
    };
    assert_eq!(header("host"), "sts.us-east-2.amazonaws.com");
    assert_eq!(header("x-amz-security-token"), "SESSION");
    assert_eq!(header("x-goog-cloud-target-resource"), audience);
    let authorization = header("authorization");
    assert!(authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKID/"), "{authorization}");
    assert!(authorization.contains("/us-east-2/sts/aws4_request, SignedHeaders=host;x-amz-date;x-amz-security-token, Signature="));
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            out.push(u8::from_str_radix(&text[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}
