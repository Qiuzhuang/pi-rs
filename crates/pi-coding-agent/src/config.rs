//! Port of `packages/coding-agent/src/config.ts`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

/// Detect if we're running as a Bun compiled binary.
/// Bun binaries have import.meta.url containing "$bunfs", "~BUN", or "%7EBUN" (Bun's virtual filesystem path)
pub static IS_BUN_BINARY: LazyLock<bool> = LazyLock::new(|| todo!("port config.ts:isBunBinary"));

/// Detect if Bun is the runtime (compiled binary or bun run)
pub static IS_BUN_RUNTIME: LazyLock<bool> = LazyLock::new(|| todo!("port config.ts:isBunRuntime"));

pub static IS_BUNDLED_NODE: LazyLock<bool> =
    LazyLock::new(|| todo!("port config.ts:isBundledNode"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstallMethod {
    #[serde(rename = "bun-binary")]
    BunBinary,
    #[serde(rename = "npm")]
    Npm,
    #[serde(rename = "pnpm")]
    Pnpm,
    #[serde(rename = "yarn")]
    Yarn,
    #[serde(rename = "bun")]
    Bun,
    #[serde(rename = "unknown")]
    Unknown,
}

impl InstallMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BunBinary => "bun-binary",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUpdateCommandStep {
    pub command: String,
    pub args: Vec<String>,
    pub display: String,
}

// TS: extends SelfUpdateCommandStep (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUpdateCommand {
    pub command: String,
    pub args: Vec<String>,
    pub display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<SelfUpdateCommandStep>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUpdatePackageTargetVariant2 {
    pub package_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_spec: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SelfUpdatePackageTarget {
    String(String),
    SelfUpdatePackageTargetVariant2(SelfUpdatePackageTargetVariant2),
}

pub fn detect_install_method() -> InstallMethod {
    todo!("port config.ts:detectInstallMethod")
}

pub fn get_self_update_command(
    package_name: &str,
    npm_command: Option<&[String]>,
    update_package_target: Option<&SelfUpdatePackageTarget>,
) -> Option<SelfUpdateCommand> {
    todo!("port config.ts:getSelfUpdateCommand")
}

pub fn get_self_update_unavailable_instruction(
    package_name: &str,
    npm_command: Option<&[String]>,
    update_package_target: Option<&SelfUpdatePackageTarget>,
) -> String {
    todo!("port config.ts:getSelfUpdateUnavailableInstruction")
}

pub fn get_update_instruction(package_name: &str) -> String {
    todo!("port config.ts:getUpdateInstruction")
}

/// Get the base directory for resolving package assets (themes, package.json, README.md, CHANGELOG.md).
/// - For Bun binary: returns the directory containing the executable
/// - For Node.js: returns the package root containing package.json
/// - Ignores Bun binary metadata copied into dist/ when the package root is available
pub fn find_node_package_dir(start_dir: &str) -> String {
    todo!("port config.ts:findNodePackageDir")
}

pub fn get_package_dir() -> String {
    todo!("port config.ts:getPackageDir")
}

/// Get path to built-in themes directory (shipped with package)
/// - For Bun binary: theme/ next to executable
/// - For Node.js (dist/): dist/modes/interactive/theme/
/// - For source (src/): src/modes/interactive/theme/
pub fn get_themes_dir() -> String {
    todo!("port config.ts:getThemesDir")
}

/// Get path to HTML export template directory (shipped with package)
/// - For Bun binary: export-html/ next to executable
/// - For Node.js (dist/): dist/core/export-html/
/// - For source (src/): src/core/export-html/
pub fn get_export_template_dir() -> String {
    todo!("port config.ts:getExportTemplateDir")
}

/// Get path to package.json
pub fn get_package_json_path() -> String {
    todo!("port config.ts:getPackageJsonPath")
}

/// Get path to README.md
pub fn get_readme_path() -> String {
    todo!("port config.ts:getReadmePath")
}

/// Get path to docs directory
pub fn get_docs_path() -> String {
    todo!("port config.ts:getDocsPath")
}

/// Get path to examples directory
pub fn get_examples_path() -> String {
    todo!("port config.ts:getExamplesPath")
}

/// Get path to CHANGELOG.md
pub fn get_changelog_path() -> String {
    todo!("port config.ts:getChangelogPath")
}

/// Get path to built-in interactive assets directory.
/// - For Bun binary: assets/ next to executable
/// - For Node.js (dist/): dist/modes/interactive/assets/
/// - For source (src/): src/modes/interactive/assets/
pub fn get_interactive_assets_dir() -> String {
    todo!("port config.ts:getInteractiveAssetsDir")
}

/// Get path to a bundled interactive asset
pub fn get_bundled_interactive_asset_path(name: &str) -> String {
    todo!("port config.ts:getBundledInteractiveAssetPath")
}

/// Called by the Bun entry with the path of the QuickJS wasm file embedded in the compiled executable.
pub fn set_embedded_quick_js_wasm_path(path: &str) {
    todo!("port config.ts:setEmbeddedQuickJSWasmPath")
}

/// Get path to `quickjs-wasi/quickjs.wasm`, the VM that runs codemode scripts. Resolved once so the
/// compiled module cached per path keeps working after an update removes this install (#10439).
pub fn get_quick_js_wasm_path() -> String {
    todo!("port config.ts:getQuickJSWasmPath")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolveCodemodeWorkerSpecifierRuntime {
    #[serde(rename = "bun-binary")]
    BunBinary,
    #[serde(rename = "bundled-node")]
    BundledNode,
    #[serde(rename = "unbundled")]
    Unbundled,
}

impl ResolveCodemodeWorkerSpecifierRuntime {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BunBinary => "bun-binary",
            Self::BundledNode => "bundled-node",
            Self::Unbundled => "unbundled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResolveCodemodeWorkerSpecifierResult {
    String(String),
    String2(String),
}

/// Resolve the codemode worker entry for a release runtime.
pub fn resolve_codemode_worker_specifier(
    runtime: ResolveCodemodeWorkerSpecifierRuntime,
    module_url: &str,
) -> Option<ResolveCodemodeWorkerSpecifierResult> {
    todo!("port config.ts:resolveCodemodeWorkerSpecifier")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GetCodemodeWorkerSpecifierResult {
    String(String),
    String2(String),
}

/// Get the codemode worker entry, or undefined to use the worker that ships next to pi-codemode.
/// The Bun and Node release builds both pass the worker as an extra entrypoint.
pub fn get_codemode_worker_specifier() -> Option<GetCodemodeWorkerSpecifierResult> {
    todo!("port config.ts:getCodemodeWorkerSpecifier")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
pub enum InstallChange {
    #[serde(rename = "updated")]
    Updated { version: String },
    #[serde(rename = "removed")]
    Removed {},
}

/// Detect that the package this process runs from changed on disk, for example after `pi update`
/// in another terminal. Code loaded on demand can then be missing or from another version.
///
/// Checks the package.json read at startup. Resolving it again would walk up past a deleted install
/// and could find an unrelated package.json, such as one in the home directory.
pub fn detect_install_change(package_json_path: Option<Value>) -> Option<InstallChange> {
    todo!("port config.ts:detectInstallChange")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageJsonPiConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_config: Option<PackageJsonPiConfig>,
}

pub static PACKAGE_NAME: LazyLock<String> = LazyLock::new(|| todo!("port config.ts:PACKAGE_NAME"));

pub static APP_NAME: LazyLock<String> = LazyLock::new(|| todo!("port config.ts:APP_NAME"));

pub static APP_TITLE: LazyLock<String> = LazyLock::new(|| todo!("port config.ts:APP_TITLE"));

pub static CONFIG_DIR_NAME: LazyLock<String> =
    LazyLock::new(|| todo!("port config.ts:CONFIG_DIR_NAME"));

pub static VERSION: LazyLock<String> = LazyLock::new(|| todo!("port config.ts:VERSION"));

pub static ENV_AGENT_DIR: LazyLock<String> =
    LazyLock::new(|| todo!("port config.ts:ENV_AGENT_DIR"));

pub static ENV_SESSION_DIR: LazyLock<String> =
    LazyLock::new(|| todo!("port config.ts:ENV_SESSION_DIR"));

pub fn expand_tilde_path(path: &str) -> String {
    todo!("port config.ts:expandTildePath")
}

/// Get the share viewer URL for a gist ID.
pub fn get_share_viewer_url(gist_id: &str) -> String {
    todo!("port config.ts:getShareViewerUrl")
}

/// Get the agent config directory (e.g., ~/.pi/agent/)
pub fn get_agent_dir() -> String {
    todo!("port config.ts:getAgentDir")
}

/// Get path to user's custom themes directory
pub fn get_custom_themes_dir() -> String {
    todo!("port config.ts:getCustomThemesDir")
}

/// Get path to models.json
pub fn get_models_path() -> String {
    todo!("port config.ts:getModelsPath")
}

/// Get path to auth.json
pub fn get_auth_path() -> String {
    todo!("port config.ts:getAuthPath")
}

/// Get path to settings.json
pub fn get_settings_path() -> String {
    todo!("port config.ts:getSettingsPath")
}

/// Get path to tools directory
pub fn get_tools_dir() -> String {
    todo!("port config.ts:getToolsDir")
}

/// Get path to managed binaries directory (fd, rg)
pub fn get_bin_dir() -> String {
    todo!("port config.ts:getBinDir")
}

/// Get path to prompt templates directory
pub fn get_prompts_dir() -> String {
    todo!("port config.ts:getPromptsDir")
}

/// Get path to sessions directory
pub fn get_sessions_dir() -> String {
    todo!("port config.ts:getSessionsDir")
}

/// Get path to debug log file
pub fn get_debug_log_path() -> String {
    todo!("port config.ts:getDebugLogPath")
}
