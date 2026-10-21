//! Port of `packages/coding-agent/src/config.ts`.
//!
//! The Rust port ships as a single native executable, so it behaves like the TS "Bun compiled
//! binary" distribution: package assets live next to the executable (or under `PI_PACKAGE_DIR`),
//! and self-update is not done through a package manager. The npm/pnpm/yarn/bun install-method
//! branches of the TS source do not apply and are not ported.
//!
//! App identity comes from constants instead of `package.json` `piConfig`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::utils::paths::normalize_path;

/// The TS source distinguishes the Bun compiled binary; the Rust executable is equivalent.
pub const IS_BUN_BINARY: bool = true;

/// Detect if Bun is the runtime. Never true for the Rust port.
pub const IS_BUN_RUNTIME: bool = false;

/// Detect the esbuild-bundled Node.js distribution. Never true for the Rust port.
pub const IS_BUNDLED_NODE: bool = false;

// =============================================================================
// Install Method Detection
// =============================================================================

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUpdateCommandStep {
    pub command: String,
    pub args: Vec<String>,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUpdateCommand {
    pub command: String,
    pub args: Vec<String>,
    pub display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<SelfUpdateCommandStep>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUpdatePackageSpec {
    pub package_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_spec: Option<String>,
}

/// `string | { packageName; installSpec? }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SelfUpdatePackageTarget {
    Name(String),
    Spec(SelfUpdatePackageSpec),
}

impl SelfUpdatePackageTarget {
    /// `normalizeSelfUpdatePackageTarget`: `(packageName, installSpec)`.
    pub fn normalize(&self) -> (String, String) {
        match self {
            Self::Name(name) => (name.clone(), name.clone()),
            Self::Spec(spec) => (
                spec.package_name.clone(),
                spec.install_spec.clone().unwrap_or_else(|| spec.package_name.clone()),
            ),
        }
    }
}

/// Always [`InstallMethod::BunBinary`] for the native executable.
pub fn detect_install_method() -> InstallMethod {
    InstallMethod::BunBinary
}

/// The command that updates this installation, if a package manager manages it. Never for the
/// native executable.
pub fn get_self_update_command(
    package_name: &str,
    npm_command: Option<&[String]>,
    update_package_target: Option<&SelfUpdatePackageTarget>,
) -> Option<SelfUpdateCommand> {
    None
}

pub fn get_self_update_unavailable_instruction(
    package_name: &str,
    npm_command: Option<&[String]>,
    update_package_target: Option<&SelfUpdatePackageTarget>,
) -> String {
    "Download from: https://github.com/earendil-works/pi/releases/latest".to_string()
}

pub fn get_update_instruction(package_name: &str) -> String {
    match get_self_update_command(package_name, None, None) {
        Some(command) => format!("Run: {}", command.display),
        None => get_self_update_unavailable_instruction(package_name, None, None),
    }
}

// =============================================================================
// Package Asset Paths (shipped with executable)
// =============================================================================

fn path_string(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}

/// Walk up from `start_dir` to the directory containing `package.json` (skipping a `dist/` with a
/// parent package). Returns `start_dir` when none is found.
pub fn find_node_package_dir(start_dir: &str) -> String {
    let mut dir = PathBuf::from(start_dir);
    loop {
        if dir.join("package.json").exists() {
            if let Some(parent) = dir.parent()
                && dir.file_name().is_some_and(|n| n == "dist")
                && parent.join("package.json").exists()
            {
                return path_string(parent);
            }
            return path_string(&dir);
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => return start_dir.to_string(),
        }
    }
}

/// Base directory for package assets: `PI_PACKAGE_DIR`, else the directory of the executable.
pub fn get_package_dir() -> String {
    if let Ok(env_dir) = std::env::var("PI_PACKAGE_DIR")
        && !env_dir.is_empty()
    {
        return normalize_path(&env_dir, None);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(path_string))
        .unwrap_or_else(|| ".".to_string())
}

/// Built-in themes directory (`theme/` next to the executable).
pub fn get_themes_dir() -> String {
    path_string(Path::new(&get_package_dir()).join("theme"))
}

/// HTML export template directory (`export-html/` next to the executable).
pub fn get_export_template_dir() -> String {
    path_string(Path::new(&get_package_dir()).join("export-html"))
}

/// Path to package.json.
pub fn get_package_json_path() -> String {
    path_string(Path::new(&get_package_dir()).join("package.json"))
}

fn absolute(path: PathBuf) -> String {
    path_string(std::path::absolute(&path).unwrap_or(path))
}

/// Path to README.md.
pub fn get_readme_path() -> String {
    absolute(Path::new(&get_package_dir()).join("README.md"))
}

/// Path to docs directory.
pub fn get_docs_path() -> String {
    absolute(Path::new(&get_package_dir()).join("docs"))
}

/// Path to examples directory.
pub fn get_examples_path() -> String {
    absolute(Path::new(&get_package_dir()).join("examples"))
}

/// Path to CHANGELOG.md.
pub fn get_changelog_path() -> String {
    absolute(Path::new(&get_package_dir()).join("CHANGELOG.md"))
}

/// Built-in interactive assets directory (`assets/` next to the executable).
pub fn get_interactive_assets_dir() -> String {
    path_string(Path::new(&get_package_dir()).join("assets"))
}

/// Path to a bundled interactive asset.
pub fn get_bundled_interactive_asset_path(name: &str) -> String {
    path_string(Path::new(&get_interactive_assets_dir()).join(name))
}

static QUICKJS_WASM_PATH: Mutex<Option<String>> = Mutex::new(None);

/// TS: path of the QuickJS wasm embedded in the Bun executable. The Rust codemode runtime uses
/// `pi-jsrt` instead; kept for API parity.
pub fn set_embedded_quick_js_wasm_path(path: &str) {
    *QUICKJS_WASM_PATH.lock().unwrap() = Some(path.to_string());
}

/// TS: path to `quickjs-wasi/quickjs.wasm`. Not used by the Rust codemode runtime.
pub fn get_quick_js_wasm_path() -> Option<String> {
    QUICKJS_WASM_PATH.lock().unwrap().clone()
}

/// Result of [`detect_install_change`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum InstallChange {
    Updated { version: String },
    Removed,
}

/// Detect that the package this process runs from changed on disk. The native executable embeds
/// its code, so replacing it does not affect this process: always `None` (as for the Bun binary).
pub fn detect_install_change(package_json_path: Option<&str>) -> Option<InstallChange> {
    None
}

// =============================================================================
// App Config
// =============================================================================

/// `package.json` shape read by the TS version (kept for package resources that ship one).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageJsonPiConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_config: Option<PackageJsonPiConfig>,
}

pub const PACKAGE_NAME: &str = "@earendil-works/pi-coding-agent";
pub const APP_NAME: &str = "pi";
pub const APP_TITLE: &str = "π";
pub const CONFIG_DIR_NAME: &str = ".pi";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// e.g. `PI_CODING_AGENT_DIR`.
pub const ENV_AGENT_DIR: &str = "PI_CODING_AGENT_DIR";
/// e.g. `PI_CODING_AGENT_SESSION_DIR`.
pub const ENV_SESSION_DIR: &str = "PI_CODING_AGENT_SESSION_DIR";

pub fn expand_tilde_path(path: &str) -> String {
    normalize_path(path, None)
}

const DEFAULT_SHARE_VIEWER_URL: &str = "https://pi.dev/session/";

/// Get the share viewer URL for a gist ID.
pub fn get_share_viewer_url(gist_id: &str) -> String {
    let base_url = std::env::var("PI_SHARE_VIEWER_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_SHARE_VIEWER_URL.to_string());
    format!("{base_url}#{gist_id}")
}

// =============================================================================
// User Config Paths (~/.pi/agent/*)
// =============================================================================

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Get the agent config directory (e.g., `~/.pi/agent/`).
pub fn get_agent_dir() -> String {
    if let Ok(env_dir) = std::env::var(ENV_AGENT_DIR)
        && !env_dir.is_empty()
    {
        return expand_tilde_path(&env_dir);
    }
    path_string(home_dir().join(CONFIG_DIR_NAME).join("agent"))
}

fn agent_path(name: &str) -> String {
    path_string(Path::new(&get_agent_dir()).join(name))
}

/// Path to user's custom themes directory.
pub fn get_custom_themes_dir() -> String {
    agent_path("themes")
}

/// Path to models.json.
pub fn get_models_path() -> String {
    agent_path("models.json")
}

/// Path to auth.json.
pub fn get_auth_path() -> String {
    agent_path("auth.json")
}

/// Path to settings.json.
pub fn get_settings_path() -> String {
    agent_path("settings.json")
}

/// Path to tools directory.
pub fn get_tools_dir() -> String {
    agent_path("tools")
}

/// Path to managed binaries directory (fd, rg).
pub fn get_bin_dir() -> String {
    agent_path("bin")
}

/// Path to prompt templates directory.
pub fn get_prompts_dir() -> String {
    agent_path("prompts")
}

/// Path to sessions directory.
pub fn get_sessions_dir() -> String {
    agent_path("sessions")
}

/// Path to debug log file.
pub fn get_debug_log_path() -> String {
    agent_path(&format!("{APP_NAME}-debug.log"))
}
