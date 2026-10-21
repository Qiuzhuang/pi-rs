//! OAuth error types. `OAuthIssuerMismatchError`, `OAuthInsecureEndpointError`,
//! `OAuthRegistrationError`, and `McpOAuthAuthorizationRequiredError` are variants of
//! [`crate::Error`].

/// An error response from an authorization server (`OAuthError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthError {
    pub code: String,
    pub message: String,
    pub error_uri: Option<String>,
}

impl OAuthError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        error_uri: Option<String>,
    ) -> Self {
        let code = code.into();
        let message = message.into();
        let message = if message.is_empty() {
            code.clone()
        } else {
            message
        };
        Self {
            code,
            message,
            error_uri,
        }
    }
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for OAuthError {}
