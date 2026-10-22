//! OAuth discovery: protected resource metadata (RFC 9728) and authorization server metadata
//! (RFC 8414 / OpenID Connect Discovery), with issuer validation.
//! Adapted from modelcontextprotocol/typescript-sdk v1.29.0 (MIT, Copyright (c) 2024 Anthropic, PBC).

use once_cell::sync::Lazy;
use regex::Regex;
use url::Url;

use super::types::{
    AuthorizationServerMetadata, OAuthChallenge, OAuthProtectedResourceMetadata, OAuthServerInfo,
    parse_authorization_server_metadata, parse_protected_resource_metadata,
};
use crate::abort::AbortSignal;
use crate::auth_provider::{HttpRequest, HttpResponse, McpFetch, default_fetch};
use crate::error::{Error, Result, join_url, parse_url};
use crate::protocol::types::LATEST_PROTOCOL_VERSION;

/// 4xx and 502 mean "not here", so discovery tries the next candidate URL.
fn is_discovery_miss(status: u16) -> bool {
    (400..500).contains(&status) || status == 502
}

/// Path suffix for `/.well-known/<kind><path>`; empty for the root path.
fn path_suffix(pathname: &str) -> &str {
    pathname.strip_suffix('/').unwrap_or(pathname)
}

fn origin_url(url: &Url) -> Result<Url> {
    parse_url(&url.origin().ascii_serialization())
}

fn field(header: &str, name: &str) -> Option<String> {
    let pattern = format!(
        r#"(?i)(?:^|[,\s]){}=(?:"([^"]*)"|([^\s,]+))"#,
        regex::escape(name)
    );
    let regex = Regex::new(&pattern).ok()?;
    let captures = regex.captures(header)?;
    // An empty value (`scope=""`) carries no information, so it counts as absent.
    captures
        .get(1)
        .filter(|value| !value.as_str().is_empty())
        .or_else(|| captures.get(2).filter(|value| !value.as_str().is_empty()))
        .map(|value| value.as_str().to_string())
}

static WHITESPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").expect("valid regex"));

/// Parse a `WWW-Authenticate` header; only `Bearer` and `DPoP` challenges carry information.
pub fn parse_www_authenticate(header: Option<&str>) -> OAuthChallenge {
    let Some(header) = header.filter(|header| !header.is_empty()) else {
        return OAuthChallenge::default();
    };
    let scheme = WHITESPACE
        .split(header.trim_start())
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if scheme != "bearer" && scheme != "dpop" {
        return OAuthChallenge::default();
    }
    OAuthChallenge {
        resource_metadata_url: field(header, "resource_metadata")
            .and_then(|url| Url::parse(&url).ok()),
        scope: field(header, "scope"),
        error: field(header, "error"),
        error_description: field(header, "error_description"),
    }
}

async fn fetch_metadata(
    url: &Url,
    fetch: &McpFetch,
    protocol_version: &str,
    signal: Option<&AbortSignal>,
) -> Result<HttpResponse> {
    let request = HttpRequest::get(url.clone())
        .header("Accept", "application/json")
        .header("MCP-Protocol-Version", protocol_version)
        .signal(signal.cloned());
    fetch(request).await
}

#[derive(Clone, Default)]
pub struct ProtectedResourceDiscoveryOptions {
    pub resource_metadata_url: Option<Url>,
    pub protocol_version: Option<String>,
    pub fetch: Option<McpFetch>,
    pub signal: Option<AbortSignal>,
}

pub async fn discover_protected_resource_metadata(
    server_url: &Url,
    options: ProtectedResourceDiscoveryOptions,
) -> Result<OAuthProtectedResourceMetadata> {
    let fetch = options.fetch.clone().unwrap_or_else(default_fetch);
    let version = options
        .protocol_version
        .as_deref()
        .unwrap_or(LATEST_PROTOCOL_VERSION);
    let origin = origin_url(server_url)?;
    let url = match &options.resource_metadata_url {
        Some(url) => url.clone(),
        None => join_url(
            &origin,
            &format!(
                "/.well-known/oauth-protected-resource{}",
                path_suffix(server_url.path())
            ),
        )?,
    };
    let mut response = fetch_metadata(&url, &fetch, version, options.signal.as_ref()).await?;
    if options.resource_metadata_url.is_none()
        && server_url.path() != "/"
        && is_discovery_miss(response.status())
    {
        drop(response);
        let fallback = join_url(&origin, "/.well-known/oauth-protected-resource")?;
        response = fetch_metadata(&fallback, &fetch, version, options.signal.as_ref()).await?;
    }
    if !response.ok() {
        return Err(Error::Other(format!(
            "HTTP {} loading OAuth protected resource metadata",
            response.status()
        )));
    }
    parse_protected_resource_metadata(response.json().await?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryKind {
    OAuth,
    Oidc,
}

pub fn build_authorization_server_discovery_urls(
    authorization_server_url: &str,
) -> Result<Vec<(Url, DiscoveryKind)>> {
    let issuer = parse_url(authorization_server_url)?;
    let origin = origin_url(&issuer)?;
    let path = path_suffix(issuer.path());
    let mut urls = vec![
        (
            join_url(
                &origin,
                &format!("/.well-known/oauth-authorization-server{path}"),
            )?,
            DiscoveryKind::OAuth,
        ),
        (
            join_url(&origin, &format!("/.well-known/openid-configuration{path}"))?,
            DiscoveryKind::Oidc,
        ),
    ];
    if !path.is_empty() {
        urls.push((
            join_url(&origin, &format!("{path}/.well-known/openid-configuration"))?,
            DiscoveryKind::Oidc,
        ));
    }
    Ok(urls)
}

#[derive(Clone, Default)]
pub struct AuthorizationServerDiscoveryOptions {
    pub fetch: Option<McpFetch>,
    pub protocol_version: Option<String>,
    pub skip_issuer_validation: bool,
    pub signal: Option<AbortSignal>,
}

pub async fn discover_authorization_server_metadata(
    authorization_server_url: &str,
    options: AuthorizationServerDiscoveryOptions,
) -> Result<Option<AuthorizationServerMetadata>> {
    let fetch = options.fetch.clone().unwrap_or_else(default_fetch);
    let version = options
        .protocol_version
        .as_deref()
        .unwrap_or(LATEST_PROTOCOL_VERSION);
    for (url, _) in build_authorization_server_discovery_urls(authorization_server_url)? {
        let response = fetch_metadata(&url, &fetch, version, options.signal.as_ref()).await?;
        if !response.ok() {
            let status = response.status();
            drop(response);
            if is_discovery_miss(status) {
                continue;
            }
            return Err(Error::Other(format!(
                "HTTP {status} loading authorization server metadata from {url}"
            )));
        }
        let metadata = parse_authorization_server_metadata(response.json().await?)?;
        if !options.skip_issuer_validation {
            // URL parsing adds a trailing slash to bare origins, so compare without one on either side.
            let trim = |value: &str| value.strip_suffix('/').unwrap_or(value).to_string();
            if trim(&metadata.issuer) != trim(authorization_server_url) {
                return Err(Error::OAuthIssuerMismatch {
                    expected: Some(authorization_server_url.to_string()),
                    received: Some(metadata.issuer.clone()),
                });
            }
        }
        return Ok(Some(metadata));
    }
    Ok(None)
}

#[derive(Clone, Default)]
pub struct OAuthServerInfoOptions {
    pub resource_metadata_url: Option<Url>,
    /// Metadata document to use instead of discovery. It is trusted as configured, so its issuer is not checked.
    pub authorization_server_metadata_url: Option<Url>,
    pub fetch: Option<McpFetch>,
    pub skip_issuer_validation: bool,
    pub signal: Option<AbortSignal>,
}

pub async fn discover_oauth_server_info(
    server_url: &Url,
    options: OAuthServerInfoOptions,
) -> Result<OAuthServerInfo> {
    let resource_metadata = match discover_protected_resource_metadata(
        server_url,
        ProtectedResourceDiscoveryOptions {
            resource_metadata_url: options.resource_metadata_url.clone(),
            protocol_version: None,
            fetch: options.fetch.clone(),
            signal: options.signal.clone(),
        },
    )
    .await
    {
        Ok(metadata) => Some(metadata),
        // Network failures abort discovery; missing or invalid metadata falls back to the server origin.
        Err(error) if error.is_type_error() => return Err(error),
        Err(_) => None,
    };
    if let Some(url) = &options.authorization_server_metadata_url {
        let fetch = options.fetch.clone().unwrap_or_else(default_fetch);
        let response = fetch_metadata(
            url,
            &fetch,
            LATEST_PROTOCOL_VERSION,
            options.signal.as_ref(),
        )
        .await?;
        if !response.ok() {
            return Err(Error::Other(format!(
                "HTTP {} loading authorization server metadata from {url}",
                response.status()
            )));
        }
        let metadata = parse_authorization_server_metadata(response.json().await?)?;
        return Ok(OAuthServerInfo {
            authorization_server_url: metadata.issuer.clone(),
            authorization_server_metadata: Some(metadata),
            resource_metadata,
        });
    }
    let authorization_server_url = match resource_metadata
        .as_ref()
        .and_then(|metadata| metadata.authorization_servers.as_ref())
        .and_then(|servers| servers.first())
    {
        Some(server) => server.clone(),
        None => join_url(server_url, "/")?.to_string(),
    };
    let authorization_server_metadata = discover_authorization_server_metadata(
        &authorization_server_url,
        AuthorizationServerDiscoveryOptions {
            fetch: options.fetch.clone(),
            protocol_version: None,
            skip_issuer_validation: options.skip_issuer_validation,
            signal: options.signal.clone(),
        },
    )
    .await?;
    Ok(OAuthServerInfo {
        authorization_server_url,
        authorization_server_metadata,
        resource_metadata,
    })
}

/// The server URL without its fragment, as an OAuth resource indicator.
pub fn resource_url_from_server_url(value: &Url) -> Url {
    let mut url = value.clone();
    url.set_fragment(None);
    url
}

/// The protected resource to request tokens for, checking that the metadata covers the server.
pub fn select_resource(
    server_url: &Url,
    metadata: Option<&OAuthProtectedResourceMetadata>,
) -> Result<Option<String>> {
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let requested = resource_url_from_server_url(server_url);
    let configured = parse_url(&metadata.resource)?;
    let mismatch = || {
        Error::Other(format!(
            "Protected resource {} does not match MCP server {requested}",
            metadata.resource
        ))
    };
    // `URL.origin` is the string "null" for opaque origins, so two of them compare equal in JS.
    if requested.origin().ascii_serialization() != configured.origin().ascii_serialization() {
        return Err(mismatch());
    }
    let with_slash = |path: &str| {
        if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        }
    };
    if !with_slash(requested.path()).starts_with(&with_slash(configured.path())) {
        return Err(mismatch());
    }
    Ok(Some(metadata.resource.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bearer_challenges() {
        let challenge = parse_www_authenticate(Some(
            r#"Bearer resource_metadata="https://example.com/meta", scope="", error="insufficient_scope""#,
        ));
        assert_eq!(
            challenge.resource_metadata_url.unwrap().as_str(),
            "https://example.com/meta"
        );
        assert_eq!(challenge.scope, None);
        assert_eq!(challenge.error.as_deref(), Some("insufficient_scope"));
        assert_eq!(
            parse_www_authenticate(Some("Basic realm=x")),
            OAuthChallenge::default()
        );
        assert_eq!(
            parse_www_authenticate(Some("Bearer scope=repo,error=x"))
                .scope
                .as_deref(),
            Some("repo")
        );
        assert_eq!(parse_www_authenticate(None), OAuthChallenge::default());
    }

    #[test]
    fn builds_discovery_urls() {
        let urls: Vec<String> =
            build_authorization_server_discovery_urls("https://as.example/tenant/")
                .unwrap()
                .into_iter()
                .map(|(url, _)| url.to_string())
                .collect();
        assert_eq!(
            urls,
            [
                "https://as.example/.well-known/oauth-authorization-server/tenant",
                "https://as.example/.well-known/openid-configuration/tenant",
                "https://as.example/tenant/.well-known/openid-configuration",
            ]
        );
        assert_eq!(
            build_authorization_server_discovery_urls("https://as.example")
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn selects_resources_under_the_configured_path() {
        let metadata = |resource: &str| OAuthProtectedResourceMetadata {
            resource: resource.into(),
            authorization_servers: None,
            scopes_supported: None,
            extra: Default::default(),
        };
        let server = Url::parse("https://mcp.example/api/mcp#x").unwrap();
        assert_eq!(
            select_resource(&server, Some(&metadata("https://mcp.example/api")))
                .unwrap()
                .as_deref(),
            Some("https://mcp.example/api")
        );
        assert!(select_resource(&server, Some(&metadata("https://mcp.example/other"))).is_err());
        assert!(select_resource(&server, Some(&metadata("https://other.example/api"))).is_err());
        assert_eq!(select_resource(&server, None).unwrap(), None);
        // Opaque origins serialize as "null" in JS and so compare equal.
        let opaque = Url::parse("custom://a/mcp").unwrap();
        assert_eq!(
            select_resource(&opaque, Some(&metadata("other://b/"))).unwrap().as_deref(),
            Some("other://b/")
        );
        assert!(select_resource(&opaque, Some(&metadata("other://b/x"))).is_err());
        assert!(select_resource(&server, Some(&metadata("other://b/"))).is_err());
    }
}
