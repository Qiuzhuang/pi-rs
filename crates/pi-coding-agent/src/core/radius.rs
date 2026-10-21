//! Port of `packages/coding-agent/src/core/radius.ts`.

use std::sync::LazyLock;

pub const RADIUS_PROVIDER_ID: &str = "radius";

pub const ENV_RADIUS_GATEWAY: &str = "PI_RADIUS_GATEWAY";

/// MCP endpoint of the gateway the built-in Radius provider signs in to.
pub static RADIUS_MCP_URL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{}/mcp",
        pi_ai::providers::radius_config::normalize_radius_gateway_url(
            pi_ai::providers::radius_config::DEFAULT_RADIUS_GATEWAY
        )
    )
});

/// Radius gateway origin, honoring the `PI_RADIUS_GATEWAY` override.
pub fn get_radius_gateway_url() -> String {
    let gateway = std::env::var(ENV_RADIUS_GATEWAY)
        .unwrap_or_else(|_| pi_ai::providers::radius_config::DEFAULT_RADIUS_GATEWAY.to_string());
    pi_ai::providers::radius_config::normalize_radius_gateway_url(&gateway)
}
