//! Error types. The TypeScript package throws distinct error classes; here they are variants of
//! one cloneable [`Error`] enum, and [`Error::name`] returns the TypeScript class name.

use serde_json::Value;

use crate::oauth::errors::OAuthError;

/// A JSON-RPC error returned by the peer (`McpError`).
#[derive(Debug, Clone, PartialEq)]
pub struct McpError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl McpError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: None }
    }

    pub fn with_data(code: i64, message: impl Into<String>, data: Option<Value>) -> Self {
        Self { code, message: message.into(), data }
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for McpError {}

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// JSON-RPC error from the peer (`McpError`).
    #[error("{0}")]
    Rpc(McpError),
    /// `McpConnectionClosedError`.
    #[error("{0}")]
    ConnectionClosed(String),
    /// `McpTimeoutError`.
    #[error("MCP request timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    /// `AbortError`: a request or flow was aborted through its signal.
    #[error("{0}")]
    Aborted(String),
    /// `McpHttpError`: an HTTP request to the MCP server failed.
    #[error("{message}")]
    Http { status: u16, message: String, body: String },
    /// `McpAuthRequiredError`: the server answered 401.
    #[error("MCP server requires authentication")]
    AuthRequired { body: String, www_authenticate: Option<String> },
    /// `McpSessionExpiredError`: the server answered 404 for an established session.
    #[error("MCP session expired")]
    SessionExpired { body: String },
    /// A network failure (`TypeError` from `fetch`), worth retrying.
    #[error("{0}")]
    Network(String),
    /// Other `TypeError`s, for example an invalid URL.
    #[error("{0}")]
    Type(String),
    /// `OAuthError`: an error response from an authorization server.
    #[error("{0}")]
    OAuth(OAuthError),
    /// `OAuthIssuerMismatchError`.
    #[error("OAuth issuer mismatch: expected {}, received {}", format_issuer(.expected), format_issuer(.received))]
    OAuthIssuerMismatch { expected: Option<String>, received: Option<String> },
    /// `OAuthInsecureEndpointError`.
    #[error("Refusing to send OAuth credentials to non-HTTPS endpoint {endpoint}")]
    OAuthInsecureEndpoint { endpoint: String },
    /// `OAuthRegistrationError`.
    #[error("OAuth dynamic client registration failed with status {status}: {body}")]
    OAuthRegistration { status: u16, body: String },
    /// `McpOAuthAuthorizationRequiredError`: the user has to authorize (again).
    #[error("MCP OAuth authorization requires user interaction")]
    OAuthAuthorizationRequired,
    /// Any other error (`Error`).
    #[error("{0}")]
    Other(String),
}

fn format_issuer(issuer: &Option<String>) -> String {
    match issuer {
        Some(issuer) => serde_json::to_string(issuer).unwrap_or_default(),
        None => "none".to_string(),
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn other(message: impl Into<String>) -> Self {
        Error::Other(message.into())
    }

    pub fn rpc(code: i64, message: impl Into<String>) -> Self {
        Error::Rpc(McpError::new(code, message))
    }

    pub fn connection_closed() -> Self {
        Error::ConnectionClosed("MCP connection closed".to_string())
    }

    pub fn aborted() -> Self {
        Error::Aborted("MCP request aborted".to_string())
    }

    /// The TypeScript error class name.
    pub fn name(&self) -> &'static str {
        match self {
            Error::Rpc(_) => "McpError",
            Error::ConnectionClosed(_) => "McpConnectionClosedError",
            Error::Timeout { .. } => "McpTimeoutError",
            Error::Aborted(_) => "AbortError",
            Error::Http { .. } => "McpHttpError",
            Error::AuthRequired { .. } => "McpAuthRequiredError",
            Error::SessionExpired { .. } => "McpSessionExpiredError",
            Error::Network(_) | Error::Type(_) => "TypeError",
            Error::OAuth(_) => "OAuthError",
            Error::OAuthIssuerMismatch { .. } => "OAuthIssuerMismatchError",
            Error::OAuthInsecureEndpoint { .. } => "OAuthInsecureEndpointError",
            Error::OAuthRegistration { .. } => "OAuthRegistrationError",
            Error::OAuthAuthorizationRequired => "McpOAuthAuthorizationRequiredError",
            Error::Other(_) => "Error",
        }
    }

    /// HTTP status of `McpHttpError` and its subclasses (`McpAuthRequiredError`, `McpSessionExpiredError`).
    pub fn http_status(&self) -> Option<u16> {
        match self {
            Error::Http { status, .. } => Some(*status),
            Error::AuthRequired { .. } => Some(401),
            Error::SessionExpired { .. } => Some(404),
            _ => None,
        }
    }

    /// Response body of `McpHttpError` and its subclasses.
    pub fn http_body(&self) -> Option<&str> {
        match self {
            Error::Http { body, .. } | Error::AuthRequired { body, .. } | Error::SessionExpired { body } => Some(body),
            _ => None,
        }
    }

    /// Whether this is an `McpHttpError` (including its subclasses).
    pub fn is_http(&self) -> bool {
        self.http_status().is_some()
    }

    /// Whether this is a JavaScript `TypeError` (network failures and invalid URLs).
    pub fn is_type_error(&self) -> bool {
        matches!(self, Error::Network(_) | Error::Type(_))
    }

    /// JSON-RPC error code of an `McpError`.
    pub fn rpc_code(&self) -> Option<i64> {
        match self {
            Error::Rpc(error) => Some(error.code),
            _ => None,
        }
    }
}

impl From<McpError> for Error {
    fn from(error: McpError) -> Self {
        Error::Rpc(error)
    }
}

impl From<OAuthError> for Error {
    fn from(error: OAuthError) -> Self {
        Error::OAuth(error)
    }
}

impl From<url::ParseError> for Error {
    fn from(_: url::ParseError) -> Self {
        Error::Type("Invalid URL".to_string())
    }
}

/// Parse a URL like `new URL(input)`.
pub(crate) fn parse_url(input: &str) -> Result<url::Url> {
    url::Url::parse(input).map_err(|_| Error::Type(format!("Invalid URL: {input}")))
}

/// Resolve like `new URL(input, base)`.
pub(crate) fn join_url(base: &url::Url, input: &str) -> Result<url::Url> {
    base.join(input).map_err(|_| Error::Type(format!("Invalid URL: {input}")))
}
