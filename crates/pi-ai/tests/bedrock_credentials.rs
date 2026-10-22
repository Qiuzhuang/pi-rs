//! AWS credential chain tests for Bedrock (profiles, AssumeRole, credential_process, SSO, web
//! identity, container and instance metadata credentials). These change the process
//! environment, so they live alone in this test binary and run as one test.

mod mock_http;

use std::path::Path;

use mock_http::{MockServer, Request, Response};
use pi_ai::api::aws_eventstream::{HeaderValue, encode_message};
use pi_ai::api::bedrock_converse_stream;
use pi_ai::providers::all::get_builtin_model;
use pi_ai::*;
use serde_json::json;
use sha1::Digest;

fn set(name: &str, value: &str) {
    // SAFETY: this binary has a single test; no other thread reads the environment concurrently.
    unsafe { std::env::set_var(name, value) }
}

fn unset(name: &str) {
    // SAFETY: see `set`.
    unsafe { std::env::remove_var(name) }
}

fn ok_stream() -> Vec<u8> {
    let frame = |event_type: &str, payload: serde_json::Value| {
        encode_message(
            &[(":message-type", HeaderValue::String("event".into())), (":event-type", HeaderValue::String(event_type.into()))],
            payload.to_string().as_bytes(),
        )
    };
    let mut body = frame("messageStart", json!({ "role": "assistant" }));
    body.extend(frame("messageStop", json!({ "stopReason": "end_turn" })));
    body
}

fn sts_xml(action: &str, access: &str) -> String {
    format!(
        "<{action}Response><{action}Result><Credentials><AccessKeyId>{access}</AccessKeyId><SecretAccessKey>secret-{access}</SecretAccessKey><SessionToken>token-{access}</SessionToken><Expiration>2999-01-01T00:00:00Z</Expiration></Credentials></{action}Result></{action}Response>"
    )
}

/// One mock for every AWS endpoint involved.
async fn aws_mock() -> MockServer {
    MockServer::start(|request: &Request, _| {
        let path = request.path_only().to_string();
        if path.starts_with("/model/") {
            return Response::new(200, "application/vnd.amazon.eventstream", ok_stream());
        }
        if path == "/" && request.method == "POST" {
            let body = request.text();
            if body.contains("Action=AssumeRoleWithWebIdentity") {
                return Response::new(200, "text/xml", sts_xml("AssumeRoleWithWebIdentity", "ASIAWEB"));
            }
            return Response::new(200, "text/xml", sts_xml("AssumeRole", "ASIACHAIN"));
        }
        match path.as_str() {
            "/federation/credentials" => Response::json(
                200,
                json!({ "roleCredentials": { "accessKeyId": "ASIASSO", "secretAccessKey": "s", "sessionToken": "t", "expiration": 32503680000000i64 } }),
            ),
            "/container-credentials" => Response::json(
                200,
                json!({ "AccessKeyId": "ASIACONTAINER", "SecretAccessKey": "s", "Token": "t", "Expiration": "2999-01-01T00:00:00Z" }),
            ),
            "/latest/api/token" => Response::text(200, "imds-token"),
            "/latest/meta-data/iam/security-credentials/" => Response::text(200, "instance-role"),
            "/latest/meta-data/iam/security-credentials/instance-role" => Response::json(
                200,
                json!({ "Code": "Success", "AccessKeyId": "ASIAIMDS", "SecretAccessKey": "s", "Token": "t", "Expiration": "2999-01-01T00:00:00Z" }),
            ),
            _ => Response::text(404, "not found"),
        }
    })
    .await
}

fn hello() -> TranscriptContext {
    normalize_context(&Context { system_prompt: None, tools: None, messages: vec![Message::User(UserMessage::new("hello", 1))] })
}

fn model_at(url: &str) -> Model {
    let mut model = get_builtin_model("amazon-bedrock", "us.anthropic.claude-opus-4-8").unwrap();
    model.base_url = url.to_string();
    model
}

fn region_options() -> StreamOptions {
    let mut env = ProviderEnv::new();
    env.insert("AWS_REGION".into(), "us-east-1".into());
    StreamOptions { env: Some(env), cache_retention: Some(CacheRetention::None), ..Default::default() }
}

/// Run one request and return the access key id the Bedrock request was signed with.
fn signed_with(options: StreamOptions) -> String {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async move {
        let server = aws_mock().await;
        let message = bedrock_converse_stream::stream(&model_at(&server.url), &hello(), options).result().await;
        assert_eq!(message.stop_reason, StopReason::Stop, "{:?}", message.error_message);
        let requests = server.requests();
        let bedrock = requests.iter().find(|r| r.path.starts_with("/model/")).expect("bedrock request");
        let authorization = bedrock.header("authorization").unwrap().to_string();
        authorization.split("Credential=").nth(1).unwrap().split('/').next().unwrap().to_string()
    })
}

#[test]
fn credential_chain() {
    for name in [
        "AWS_PROFILE",
        "AWS_REGION",
        "AWS_DEFAULT_REGION",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AWS_WEB_IDENTITY_TOKEN_FILE",
        "AWS_ROLE_ARN",
        "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
        "AWS_CONTAINER_CREDENTIALS_FULL_URI",
        "AWS_BEARER_TOKEN_BEDROCK",
        "AWS_ENDPOINT_URL",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "http_proxy",
        "https_proxy",
        "ALL_PROXY",
    ] {
        unset(name);
    }
    let home = std::env::temp_dir().join(format!("pi-ai-aws-test-{}", std::process::id()));
    std::fs::create_dir_all(home.join(".aws/sso/cache")).unwrap();
    set("HOME", home.to_str().unwrap());
    set("AWS_EC2_METADATA_DISABLED", "true");
    let write = |path: &Path, text: &str| std::fs::write(path, text).unwrap();
    write(&home.join(".aws/credentials"), "[static]\naws_access_key_id = AKIASTATIC\naws_secret_access_key = static-secret\n");
    let config = "[profile chain]\nrole_arn = arn:aws:iam::123456789012:role/bedrock\nsource_profile = static\nregion = us-west-2\n\
[profile proc]\ncredential_process = echo '{\"Version\":1,\"AccessKeyId\":\"AKIAPROC\",\"SecretAccessKey\":\"p\"}'\n\
[profile sso]\nsso_session = corp\nsso_account_id = 123456789012\nsso_role_name = Dev\n\
[sso-session corp]\nsso_region = us-east-1\nsso_start_url = https://corp.awsapps.com/start\n\
[profile login]\nlogin_session = arn:aws:iam::123456789012:user/me\nregion = us-east-1\n\
[profile mfa]\nrole_arn = arn:aws:iam::123456789012:role/mfa\nsource_profile = static\nmfa_serial = arn:aws:iam::123456789012:mfa/me\n\
[profile regional]\naws_access_key_id = AKIAREGIONAL\naws_secret_access_key = r\nregion = eu-west-3\n";
    write(&home.join(".aws/config"), config);
    let cache_name: String = sha1::Sha1::digest(b"corp").iter().map(|b| format!("{b:02x}")).collect();
    write(
        &home.join(format!(".aws/sso/cache/{cache_name}.json")),
        &json!({ "accessToken": "sso-access-token", "expiresAt": "2999-01-01T00:00:00Z", "region": "us-east-1", "startUrl": "https://corp.awsapps.com/start" }).to_string(),
    );

    // Static keys from the credentials file.
    assert_eq!(signed_with(region_options().with_extra("profile", "static")), "AKIASTATIC");

    // `aws login` sessions use the cached credentials while they are fresh.
    let login_cache = home.join(".aws/login/cache");
    std::fs::create_dir_all(&login_cache).unwrap();
    let login_name: String =
        <sha2::Sha256 as sha2::Digest>::digest(b"arn:aws:iam::123456789012:user/me").iter().map(|b| format!("{b:02x}")).collect();
    write(
        &login_cache.join(format!("{login_name}.json")),
        &json!({
            "accessToken": { "accessKeyId": "ASIALOGIN", "secretAccessKey": "s", "sessionToken": "t", "accountId": "123456789012", "expiresAt": "2999-01-01T00:00:00Z" },
            "clientId": "arn:aws:signin:::devtools/same-device",
            "refreshToken": "r",
            "dpopKey": "-----BEGIN EC PRIVATE KEY-----\nunused\n-----END EC PRIVATE KEY-----\n"
        })
        .to_string(),
    );
    assert_eq!(signed_with(region_options().with_extra("profile", "login")), "ASIALOGIN");

    // MFA role profiles stop the chain: the default chain has no MFA code callback.
    {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let message = runtime.block_on(async {
            let server = aws_mock().await;
            bedrock_converse_stream::stream(&model_at(&server.url), &hello(), region_options().with_extra("profile", "mfa")).result().await
        });
        assert_eq!(
            message.error_message.as_deref(),
            Some("Profile mfa requires multi-factor authentication, but no MFA code callback was provided.")
        );
    }

    // credential_process.
    if !cfg!(windows) {
        assert_eq!(signed_with(region_options().with_extra("profile", "proc")), "AKIAPROC");
    }

    // The remaining sources call STS/SSO/container/IMDS endpoints: point the configured endpoint
    // variables at the mock.
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let server = aws_mock().await;
        set("AWS_ENDPOINT_URL_STS", &server.url);
        set("AWS_ENDPOINT_URL_SSO", &server.url);

        // AssumeRole with a source profile, signed with the source keys in the profile's region.
        let message = bedrock_converse_stream::stream(&model_at(&server.url), &hello(), region_options().with_extra("profile", "chain")).result().await;
        assert_eq!(message.stop_reason, StopReason::Stop, "{:?}", message.error_message);
        let requests = server.requests();
        let sts = requests.iter().find(|r| r.path == "/" && r.text().contains("Action=AssumeRole")).unwrap();
        assert!(sts.text().contains("RoleArn=arn%3Aaws%3Aiam%3A%3A123456789012%3Arole%2Fbedrock"));
        assert!(sts.header("authorization").unwrap().contains("Credential=AKIASTATIC/"));
        assert!(sts.header("authorization").unwrap().contains("/us-west-2/sts/aws4_request"));
        let bedrock = requests.iter().rfind(|r| r.path.starts_with("/model/")).unwrap();
        assert!(bedrock.header("authorization").unwrap().contains("Credential=ASIACHAIN/"));
        assert_eq!(bedrock.header("x-amz-security-token"), Some("token-ASIACHAIN"));

        // IAM Identity Center (SSO) from the token cache.
        let message = bedrock_converse_stream::stream(&model_at(&server.url), &hello(), region_options().with_extra("profile", "sso")).result().await;
        assert_eq!(message.stop_reason, StopReason::Stop, "{:?}", message.error_message);
        let requests = server.requests();
        let portal = requests.iter().find(|r| r.path_only() == "/federation/credentials").unwrap();
        assert_eq!(portal.path, "/federation/credentials?account_id=123456789012&role_name=Dev");
        assert_eq!(portal.header("x-amz-sso_bearer_token"), Some("sso-access-token"));
        assert!(requests.iter().rfind(|r| r.path.starts_with("/model/")).unwrap().header("authorization").unwrap().contains("Credential=ASIASSO/"));

        // No source at all.
        let message = bedrock_converse_stream::stream(&model_at(&server.url), &hello(), region_options()).result().await;
        assert_eq!(message.error_message.as_deref(), Some("Could not load credentials from any providers"));

        // Web identity from the environment.
        let token_file = home.join("web-identity-token");
        std::fs::write(&token_file, "jwt-token").unwrap();
        set("AWS_WEB_IDENTITY_TOKEN_FILE", token_file.to_str().unwrap());
        set("AWS_ROLE_ARN", "arn:aws:iam::123456789012:role/web");
        let message = bedrock_converse_stream::stream(&model_at(&server.url), &hello(), region_options()).result().await;
        assert_eq!(message.stop_reason, StopReason::Stop, "{:?}", message.error_message);
        let requests = server.requests();
        let web = requests.iter().find(|r| r.text().contains("Action=AssumeRoleWithWebIdentity")).unwrap();
        assert!(web.text().contains("WebIdentityToken=jwt-token"));
        assert!(web.header("authorization").is_none());
        assert!(requests.iter().rfind(|r| r.path.starts_with("/model/")).unwrap().header("authorization").unwrap().contains("Credential=ASIAWEB/"));
        unset("AWS_WEB_IDENTITY_TOKEN_FILE");
        unset("AWS_ROLE_ARN");

        // Container credentials.
        set("AWS_CONTAINER_CREDENTIALS_FULL_URI", &format!("{}/container-credentials", server.url));
        set("AWS_CONTAINER_AUTHORIZATION_TOKEN", "container-auth");
        let message = bedrock_converse_stream::stream(&model_at(&server.url), &hello(), region_options()).result().await;
        assert_eq!(message.stop_reason, StopReason::Stop, "{:?}", message.error_message);
        let requests = server.requests();
        assert_eq!(requests.iter().find(|r| r.path == "/container-credentials").unwrap().header("authorization"), Some("container-auth"));
        assert!(requests.iter().rfind(|r| r.path.starts_with("/model/")).unwrap().header("authorization").unwrap().contains("Credential=ASIACONTAINER/"));
        unset("AWS_CONTAINER_CREDENTIALS_FULL_URI");
        unset("AWS_CONTAINER_AUTHORIZATION_TOKEN");

        // Instance metadata (IMDSv2).
        unset("AWS_EC2_METADATA_DISABLED");
        set("AWS_EC2_METADATA_SERVICE_ENDPOINT", &server.url);
        let message = bedrock_converse_stream::stream(&model_at(&server.url), &hello(), region_options()).result().await;
        assert_eq!(message.stop_reason, StopReason::Stop, "{:?}", message.error_message);
        let requests = server.requests();
        let role = requests.iter().find(|r| r.path == "/latest/meta-data/iam/security-credentials/instance-role").unwrap();
        assert_eq!(role.header("x-aws-ec2-metadata-token"), Some("imds-token"));
        assert!(requests.iter().rfind(|r| r.path.starts_with("/model/")).unwrap().header("authorization").unwrap().contains("Credential=ASIAIMDS/"));
        set("AWS_EC2_METADATA_DISABLED", "true");

        // An ambient profile without a configured region uses the profile's region and the
        // configured Bedrock endpoint.
        set("AWS_PROFILE", "regional");
        set("AWS_ENDPOINT_URL_BEDROCK_RUNTIME", &server.url);
        let standard = get_builtin_model("amazon-bedrock", "us.anthropic.claude-opus-4-8").unwrap();
        let options = StreamOptions { cache_retention: Some(CacheRetention::None), ..Default::default() };
        let config = bedrock_converse_stream::resolve_client_config(&standard, &options);
        assert_eq!(config.profile.as_deref(), Some("regional"));
        assert_eq!(config.region, None);
        assert_eq!(config.endpoint, None);
        let message = bedrock_converse_stream::stream(&standard, &hello(), options).result().await;
        assert_eq!(message.stop_reason, StopReason::Stop, "{:?}", message.error_message);
        let requests = server.requests();
        let authorization = requests.iter().rfind(|r| r.path.starts_with("/model/")).unwrap().header("authorization").unwrap().to_string();
        assert!(authorization.contains("Credential=AKIAREGIONAL/"), "{authorization}");
        assert!(authorization.contains("/eu-west-3/bedrock/aws4_request"), "{authorization}");
        unset("AWS_PROFILE");
        unset("AWS_ENDPOINT_URL_BEDROCK_RUNTIME");
    });
    let _ = std::fs::remove_dir_all(&home);
}
