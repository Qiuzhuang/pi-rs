//! Port of `packages/coding-agent/src/core/radius.ts`.

use std::sync::LazyLock;

pub const RADIUS_PROVIDER_ID: &str = "radius";

pub const ENV_RADIUS_GATEWAY: &str = "PI_RADIUS_GATEWAY";

/// MCP endpoint of the gateway the built-in Radius provider signs in to.
pub static RADIUS_MCP_URL: LazyLock<String> =
    LazyLock::new(|| todo!("port core/radius.ts:RADIUS_MCP_URL"));

/// Radius gateway origin, honoring the `PI_RADIUS_GATEWAY` override.
pub fn get_radius_gateway_url() -> String {
    todo!("port core/radius.ts:getRadiusGatewayUrl")
}
