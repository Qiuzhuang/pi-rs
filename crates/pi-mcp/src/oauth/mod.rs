//! MCP OAuth (`@earendil-works/pi-mcp/oauth`): discovery, the authorization code flow with PKCE,
//! dynamic client registration, token refresh, a stateful provider, and a loopback callback server.

pub mod callback;
pub mod discovery;
pub mod errors;
pub mod flow;
pub mod provider;
pub mod types;

pub use callback::{OAuthCallback, OAuthCallbackPage, OAuthCallbackServer, OAuthCallbackServerOptions, RenderPage};
pub use discovery::{
    AuthorizationServerDiscoveryOptions, DiscoveryKind, OAuthServerInfoOptions, ProtectedResourceDiscoveryOptions,
    build_authorization_server_discovery_urls, discover_authorization_server_metadata, discover_oauth_server_info,
    discover_protected_resource_metadata, parse_www_authenticate, resource_url_from_server_url, select_resource,
};
pub use errors::OAuthError;
pub use flow::{
    AddClientAuthentication, InvalidateCredentials, OAuthClientMetadataDocument, OAuthClientProvider, OAuthFlowOptions,
    OAuthFlowResult, RegisterClientOptions, StartAuthorizationOptions, TokenRequestOptions, UrlSearchParams,
    adapt_oauth_provider, authorize_mcp, exchange_authorization_code, get_search_param, refresh_authorization,
    register_client, set_search_param, start_authorization, step_up_scope,
};
pub use provider::{
    ClientMetadataDocumentFn, McpOAuthProvider, McpOAuthProviderOptions, McpOAuthState, McpOAuthStateStore,
    MemoryOAuthStateStore, RedirectFn,
};
pub use types::{
    AuthorizationServerMetadata, OAuthChallenge, OAuthClientInformation, OAuthClientInformationFull,
    OAuthClientInformationMixed, OAuthClientMetadata, OAuthDiscoveryState, OAuthProtectedResourceMetadata,
    OAuthServerInfo, OAuthTokens, parse_authorization_server_metadata, parse_client_information, parse_oauth_tokens,
    parse_protected_resource_metadata,
};
