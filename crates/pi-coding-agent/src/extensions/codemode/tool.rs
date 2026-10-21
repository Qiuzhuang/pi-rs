//! Port of `packages/coding-agent/src/extensions/codemode/tool.ts`.

use crate::compat::CodemodeJsonSchema;
use crate::core::extensions::types::ToolDefinition;
use crate::core::extensions::types::ToolNamespace;
use crate::core::model_registry::ModelRegistry;
use crate::core::settings_schema::CodemodeMode;
use indexmap::{IndexMap, IndexSet};
use pi_agent::types::AgentToolRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

pub const CODEMODE_TOOL_NAME: &str = "codemode";

/// Custom entry type holding one script's `store()` writes: {@link CodemodeStoreEntryData}.
pub const CODEMODE_STORE_ENTRY_TYPE: &str = "codemode-store";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodemodeStoreEntryData {
    pub set: IndexMap<String, Value>,
    pub delete: Vec<String>,
}

/// The part of the model registry that scripts reach through `models`.
/// TS: Pick< ModelRegistry, "getModelsOfType" | "getAvailableOfType" | "getModelOfType" | "classify" | "generateImages" >
pub type CodemodeModelRuntime = Arc<ModelRegistry>;

#[derive(Clone)]
pub struct CodemodeToolOptions {
    /// Namespace of a tool, for `searchTools()` ranking and its `namespace` filter.
    pub get_tool_namespace: Option<Arc<dyn Fn(String) -> Option<ToolNamespace> + Send + Sync>>,
    /// Prompt guidelines of every tool, by tool name, shown with declarations by `describeTool()` and `ALL_TOOLS`.
    pub get_tool_guidelines: Option<Arc<dyn Fn() -> IndexMap<String, Vec<String>> + Send + Sync>>,
    /// Expose the `models` namespace to scripts, backed by the session's model registry
    /// (`ctx.modelRegistry`). Without it, `models` is not declared.
    pub models: Option<bool>,
    /// Persists `store()` writes as a session custom entry. Without it, writes last only for the
    /// current script; `load()` still reads entries already on the branch.
    pub append_entry: Option<Arc<dyn Fn(String, CodemodeStoreEntryData) + Send + Sync>>,
    /// How the tool presents the loadout while active (the `codemode.mode` setting). Default: `on`.
    pub get_mode: Option<Arc<dyn Fn() -> CodemodeMode + Send + Sync>>,
    /// Token budget for tool declarations in the description. Default: {@link DEFAULT_CODEMODE_INLINE_BUDGET}.
    pub get_inline_budget: Option<Arc<dyn Fn() -> Option<f64> + Send + Sync>>,
}

pub static CODEMODE_SCHEMA: LazyLock<Value> =
    LazyLock::new(|| todo!("port extensions/codemode/tool.ts:codemodeSchema"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodemodeToolInput {
    /// Raw JavaScript source.
    pub code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CodemodeNestedCallStatus {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "cancelled")]
    Cancelled,
}

impl CodemodeNestedCallStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodemodeNestedCall {
    /// Tool call id of the nested call, `<codemode call id>/<n>`.
    pub id: String,
    pub name: String,
    /// Compact JSON of the arguments, truncated for display.
    pub args: String,
    pub status: CodemodeNestedCallStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub duration_ms: Option<f64>,
    /// Error text, truncated for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Cost in USD of a `models.*` call that reported usage.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodemodeToolDetails {
    pub calls: Vec<CodemodeNestedCall>,
    /// Temp file with the full text output, when the output was truncated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

pub static CODEMODE_TOOL_SYSTEM_PROMPT_CONTRIBUTION: LazyLock<Value> = LazyLock::new(|| {
    todo!("port extensions/codemode/tool.ts:codemodeToolSystemPromptContribution")
});

/// The reference for scripts: globals, tool results, `store()`, the `models` API, and limits.
pub static CODEMODE_DOCS_PATH: LazyLock<Value> =
    LazyLock::new(|| todo!("port extensions/codemode/tool.ts:CODEMODE_DOCS_PATH"));

/// Default for {@link CodemodeDescriptionOptions.inlineBudget}, in estimated tokens.
pub static DEFAULT_CODEMODE_INLINE_BUDGET: LazyLock<i64> =
    LazyLock::new(|| todo!("port extensions/codemode/tool.ts:DEFAULT_CODEMODE_INLINE_BUDGET"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToCodemodeDeclarationResult {
    /// The script calls tools as `tools.<id>(args)`, where `<id>` is the name with characters that
    /// are not valid in identifiers replaced by `_` (see `toCodemodeIdentifier`), and also as
    /// `tools["<name>"](args)`. Globals are called as `<name>(args)` and must be identifiers, or
    /// `<namespace>.<member>`, which groups them into a frozen namespace object.
    pub name: String,
    /// Shown as a doc comment in {@link renderDeclarations}, and listed in `ALL_TOOLS` for tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Schema of the single argument. Rendered as the parameter type; `unknown` when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<CodemodeJsonSchema>,
    /// Schema of the resolved value. Rendered as the promise type; `unknown` when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<CodemodeJsonSchema>,
    /// Globals only: `execute` receives all call arguments as an array instead of the first one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread: Option<bool>,
    /// Globals only: TypeScript parameter list and return type for {@link renderDeclarations}, for
    /// example `(type: string, id?: string): Promise<Model[]>`. Replaces the rendering from the schemas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// What a script sees of a tool: its description followed by its prompt guidelines, which the system
/// prompt only has for declared tools. Tools without an output schema resolve to their text output.
pub fn to_codemode_declaration(
    tool: AgentToolRef,
    guidelines: Option<&[String]>,
) -> ToCodemodeDeclarationResult {
    todo!("port extensions/codemode/tool.ts:toCodemodeDeclaration")
}

/// Tools a script may call: every given tool except the codemode tool itself.
pub fn get_codemode_callable_tools(tools: &[AgentToolRef]) -> Vec<AgentToolRef> {
    todo!("port extensions/codemode/tool.ts:getCodemodeCallableTools")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodemodeDescriptionOptions {
    /// Declare the `models` namespace; only for tools created with model access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<bool>,
    /// Namespace of each tool, by tool name. Tools of one namespace are listed under one heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespaces: Option<IndexMap<String, ToolNamespace>>,
    /// Tools that are callable but never listed with their declaration (`deferred` exposure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deferred: Option<IndexSet<String>>,
    /// Prompt guidelines of each tool, by tool name, listed after its description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guidelines: Option<IndexMap<String, Vec<String>>>,
    /// Estimated tokens (characters / 4) the tool sections may use. Tools that do not fit are left
    /// out, like deferred tools. Unset lists every tool that is not deferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_budget: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub name: String,
    pub section: String,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<ToolNamespace>,
    pub entries: Vec<CatalogEntry>,
}

/// Model-facing description: the helper list, guidance for finding tools that are not listed, the
/// `models` API, and one section per listed tool, grouped by namespace. Deferred tools are never
/// listed and do not affect the description at all, so it stays the same while tools are registered
/// late (for example by MCP servers that connect). Tool sections are limited to `inlineBudget`.
pub fn create_codemode_description(
    tools: &[AgentToolRef],
    options: Option<&CodemodeDescriptionOptions>,
) -> String {
    todo!("port extensions/codemode/tool.ts:createCodemodeDescription")
}

pub fn create_codemode_tool_definition(options: Option<&CodemodeToolOptions>) -> ToolDefinition {
    todo!("port extensions/codemode/tool.ts:createCodemodeToolDefinition")
}

/// Create the codemode tool as an AgentTool. The description lists the given tools; the script can
/// call whatever tools the agent loop provides at execution time.
pub fn create_codemode_tool(
    tools: Option<&[AgentToolRef]>,
    options: Option<&CodemodeToolOptions>,
) -> AgentToolRef {
    todo!("port extensions/codemode/tool.ts:createCodemodeTool")
}
