//! Port of `packages/coding-agent/src/extensions/mcp/resources.ts`.

use crate::core::extensions::types::ToolDefinition;
use crate::core::mcp_servers::McpExposure;
use pi_mcp::client::McpRequestOptions;
use pi_mcp::protocol::types::ListResourceTemplatesResult;
use pi_mcp::protocol::types::ListResourcesResult;
use pi_mcp::protocol::types::ReadResourceResult;
use pi_mcp::protocol::types::Resource;
use pi_mcp::protocol::types::ResourceTemplate;
use std::sync::Arc;

/// A connected server that offers resources.
#[async_trait::async_trait]
pub trait McpResourceServer: Send + Sync {
    fn name(&self) -> String;
    fn set_name(&self, value: String);
    fn timeout_ms(&self) -> i64;
    fn set_timeout_ms(&self, value: i64);
    async fn resources_page(
        &self,
        cursor: Option<&str>,
        options: McpRequestOptions,
    ) -> anyhow::Result<ListResourcesResult>;
    async fn resource_templates_page(
        &self,
        cursor: Option<&str>,
        options: McpRequestOptions,
    ) -> anyhow::Result<ListResourceTemplatesResult>;
    async fn all_resources(&self, options: McpRequestOptions) -> anyhow::Result<Vec<Resource>>;
    async fn all_resource_templates(
        &self,
        options: McpRequestOptions,
    ) -> anyhow::Result<Vec<ResourceTemplate>>;
    async fn read_resource(
        &self,
        uri: &str,
        options: McpRequestOptions,
    ) -> anyhow::Result<ReadResourceResult>;
}

#[derive(Clone)]
pub struct CreateMcpResourceToolDefinitionsOptions {
    pub exposure: McpExposure,
    pub servers: Arc<dyn Fn() -> Vec<Arc<dyn McpResourceServer>> + Send + Sync>,
}

/// The three resource tools. `servers` returns the servers whose resources they reach, at call time.
pub fn create_mcp_resource_tool_definitions(
    options: &CreateMcpResourceToolDefinitionsOptions,
) -> anyhow::Result<Vec<Arc<dyn ToolDefinition>>> {
    todo!("port extensions/mcp/resources.ts:createMcpResourceToolDefinitions")
}
