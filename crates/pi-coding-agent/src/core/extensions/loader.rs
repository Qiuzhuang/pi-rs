//! Port of `packages/coding-agent/src/core/extensions/loader.ts`.

use crate::core::event_bus::EventBus;
use crate::core::extensions::types::Extension;
use crate::core::extensions::types::ExtensionFactory;
use crate::core::extensions::types::ExtensionRuntime;
use crate::core::extensions::types::LoadExtensionsResult;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub type HandlerFn =
    Arc<dyn Fn(Vec<Vec<Value>>) -> BoxFuture<'static, anyhow::Result<Value>> + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCacheToken {
    pub cwd: String,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub generation: f64,
}

pub fn clear_extension_cache() {
    todo!("port core/extensions/loader.ts:clearExtensionCache")
}

/// Create a runtime with throwing stubs for action methods.
/// Runner.bindCore() replaces these with real implementations.
pub fn create_extension_runtime() -> anyhow::Result<ExtensionRuntime> {
    todo!("port core/extensions/loader.ts:createExtensionRuntime")
}

/// Create an Extension from an inline factory function.
pub async fn load_extension_from_factory(
    factory: &ExtensionFactory,
    cwd: &str,
    event_bus: Arc<dyn EventBus>,
    runtime: &ExtensionRuntime,
    extension_path: Option<&str>,
) -> anyhow::Result<Extension> {
    todo!("port core/extensions/loader.ts:loadExtensionFromFactory")
}

pub async fn load_extensions(
    paths: &[String],
    cwd: &str,
    event_bus: Option<Arc<dyn EventBus>>,
    runtime: Option<&ExtensionRuntime>,
) -> anyhow::Result<LoadExtensionsResult> {
    todo!("port core/extensions/loader.ts:loadExtensions")
}

pub async fn load_extensions_cached(
    paths: &[String],
    cwd: &str,
    event_bus: Option<Arc<dyn EventBus>>,
    runtime: Option<&ExtensionRuntime>,
) -> anyhow::Result<LoadExtensionsResult> {
    todo!("port core/extensions/loader.ts:loadExtensionsCached")
}

/// Discover and load extensions from standard locations.
pub async fn discover_and_load_extensions(
    configured_paths: &[String],
    cwd: &str,
    agent_dir: Option<&str>,
    event_bus: Option<Arc<dyn EventBus>>,
) -> anyhow::Result<LoadExtensionsResult> {
    todo!("port core/extensions/loader.ts:discoverAndLoadExtensions")
}
