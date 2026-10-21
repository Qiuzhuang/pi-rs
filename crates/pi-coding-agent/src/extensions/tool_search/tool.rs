//! Port of `packages/coding-agent/src/extensions/tool-search/tool.ts`.

use crate::core::extensions::types::ToolDefinition;
use crate::core::extensions::types::ToolInfo;
use crate::core::extensions::types::ToolNamespace;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

pub const TOOL_SEARCH_TOOL_NAME: &str = "tool_search";

pub const DEFAULT_TOOL_SEARCH_LIMIT: i64 = 8;

/// A tool as the ranker sees it: its name and the text built by {@link createToolSearchDocument}.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSearchDocument {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSearchMatch {
    pub name: String,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub score: f64,
}

/// Ranks tools for a query. BM25 today; a hybrid ranker with embeddings can replace it.
pub trait ToolRanker: Send + Sync {
    fn rank(
        &self,
        query: &str,
        documents: &[ToolSearchDocument],
        limit: i64,
    ) -> Vec<ToolSearchMatch>;
}

/// Lowercase terms, split at camelCase boundaries and non-alphanumerics, without stop words.
pub fn tokenize(text: &str) -> Vec<String> {
    todo!("port extensions/tool-search/tool.ts:tokenize")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateToolSearchDocumentTool {
    /// Tool name (used in LLM tool calls)
    pub name: String,
    /// Description for LLM
    pub description: String,
    /// Parameter schema (TypeBox)
    pub parameters: Value,
}

/// Search text of a tool: the name, the name with `_`
/// as spaces, the description, schema descriptions and property names, and the namespace with its
/// description and instructions.
pub fn create_tool_search_document(
    tool: &CreateToolSearchDocumentTool,
    namespace: Option<&ToolNamespace>,
) -> ToolSearchDocument {
    todo!("port extensions/tool-search/tool.ts:createToolSearchDocument")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bm25RankerOptions {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub k1: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub b: Option<f64>,
}

/// Okapi BM25 with the usual parameters. Ties keep document order.
/// TS: `implements ToolRanker`
pub struct Bm25Ranker {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl Bm25Ranker {
    pub fn new(options: Option<&Bm25RankerOptions>) -> Self {
        todo!("port extensions/tool-search/tool.ts:Bm25Ranker.constructor")
    }
}

impl ToolRanker for Bm25Ranker {
    fn rank(
        &self,
        query: &str,
        documents: &[ToolSearchDocument],
        limit: i64,
    ) -> Vec<ToolSearchMatch> {
        todo!("port extensions/tool-search/tool.ts:Bm25Ranker.rank")
    }
}

pub static TOOL_SEARCH_SCHEMA: LazyLock<Value> =
    LazyLock::new(|| todo!("port extensions/tool-search/tool.ts:toolSearchSchema"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSearchInput {
    /// Search query for deferred tools.
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSearchResultTool {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSearchToolDetails {
    /// Tools loaded by this call.
    pub loaded: Vec<String>,
}

#[derive(Clone)]
pub struct ToolSearchToolOptionsTools {
    /// Get all configured tools with parameter schema, prompt guidelines, exposure, and source metadata.
    pub get_all_tools: Arc<dyn Fn() -> Vec<ToolInfo> + Send + Sync>,
    /// Get the names of the active tools, which are the tools declared to the model.
    pub get_active_tools: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    /// Set the active tools by name. Unknown and `hidden` tools are ignored. Tools with `codemode` or
    /// `deferred` exposure stay callable from codemode scripts whether active or not.
    pub set_active_tools: Arc<dyn Fn(Vec<String>) + Send + Sync>,
}

#[derive(Clone)]
pub struct ToolSearchToolOptions {
    /// The session's tools. `tool_search` searches the tools that are not declared to the model and
    /// activates the matches. Without it, the tool finds nothing. An `ExtensionAPI` fits.
    pub tools: Option<ToolSearchToolOptionsTools>,
}

/// The `tool_search` description. It does not list the searchable tools or their namespaces, so it
/// stays the same while tools are registered, for example when MCP servers connect.
pub static TOOL_SEARCH_DESCRIPTION: LazyLock<String> =
    LazyLock::new(|| todo!("port extensions/tool-search/tool.ts:TOOL_SEARCH_DESCRIPTION"));

pub fn create_tool_search_tool_definition(
    options: Option<&ToolSearchToolOptions>,
) -> anyhow::Result<ToolDefinition> {
    todo!("port extensions/tool-search/tool.ts:createToolSearchToolDefinition")
}
