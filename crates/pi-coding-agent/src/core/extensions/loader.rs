//! Port of `packages/coding-agent/src/core/extensions/loader.ts`.
//!
//! Extension files (`.ts` / `.js`) are loaded by the embedded JavaScript runtime
//! (`pi-jsrt`, see [`crate::core::extensions::js`]) instead of jiti. Inline extensions are Rust
//! factories ([`ExtensionFactory`]). Both receive an [`ExtensionAPI`] implemented here.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, LazyLock};

use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use pi_agent::types::ThinkingLevel;
use pi_ai::models::Provider;
use pi_ai::types::{Model, UserContent};

use crate::config::{CONFIG_DIR_NAME, get_agent_dir};
use crate::core::event_bus::{EventBus, create_event_bus};
use crate::core::exec::{ExecOptions, ExecResult, exec_command};
use crate::core::extensions::js::JsExtensionHost;
use crate::core::extensions::types::*;
use crate::core::mcp_servers::{
    McpServerConfig, McpServerRegistry, RegisteredMcpServer, mcp_namespace, validate_mcp_server_config,
};
use crate::core::pi_manifest::read_pi_manifest;
use crate::core::settings_manager::Settings;
use crate::core::slash_commands::SlashCommandInfo;
use crate::core::source_info::{
    CreateSyntheticSourceInfoOptions, create_synthetic_source_info, get_synthetic_path_source, is_synthetic_path,
};
use crate::core::timings::{TimingLabel, time};
use crate::core::virtual_models::VirtualModelDefinition;
use crate::utils::paths::{PathInputOptions, node_dirname, node_join, node_resolve, resolve_path};


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCacheToken {
    pub cwd: String,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub generation: f64,
}

/// Module-level extension cache (TS `extensionCacheCwd`, `extensionCacheGeneration`, `extensionCache`).
#[derive(Default)]
struct ExtensionCache {
    cwd: Option<String>,
    generation: u64,
    factories: HashMap<String, ExtensionFactory>,
}

static EXTENSION_CACHE: LazyLock<Mutex<ExtensionCache>> = LazyLock::new(|| Mutex::new(ExtensionCache::default()));

pub fn clear_extension_cache() {
    let mut cache = EXTENSION_CACHE.lock();
    cache.factories.clear();
    cache.cwd = None;
    cache.generation += 1;
    drop(cache);
    // Cached modules lived in the shared JS runtime of this generation; the next load starts
    // a fresh one.
    JsExtensionHost::reset_shared();
}

fn use_extension_cache_cwd(cwd: &str) -> ExtensionCacheToken {
    let resolved_cwd = resolve_path(cwd, None, None);
    let needs_clear = {
        let cache = EXTENSION_CACHE.lock();
        cache.cwd.as_ref().is_some_and(|c| c != &resolved_cwd)
    };
    if needs_clear {
        clear_extension_cache();
    }
    let mut cache = EXTENSION_CACHE.lock();
    cache.cwd = Some(resolved_cwd.clone());
    ExtensionCacheToken { cwd: resolved_cwd, generation: cache.generation as f64 }
}

fn is_current_cache_token(token: Option<&ExtensionCacheToken>) -> bool {
    let Some(token) = token else { return false };
    let cache = EXTENSION_CACHE.lock();
    cache.cwd.as_deref() == Some(token.cwd.as_str()) && cache.generation as f64 == token.generation
}

const NOT_INITIALIZED: &str = "Extension runtime not initialized. Action methods cannot be called during extension loading.";

/// `pi.*` actions before `runner.bindCore()`: throwing stubs.
struct NotInitializedActions;

#[async_trait::async_trait]
impl ExtensionActions for NotInitializedActions {
    fn send_message(&self, _message: CustomMessageInput, _options: SendMessageOptions) -> anyhow::Result<()> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn send_user_message(&self, _content: UserContent, _options: SendUserMessageOptions) -> anyhow::Result<()> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn append_entry(&self, _custom_type: &str, _data: Option<Value>) -> anyhow::Result<()> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn set_session_name(&self, _name: &str) -> anyhow::Result<()> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn get_session_name(&self) -> anyhow::Result<Option<String>> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn set_label(&self, _entry_id: &str, _label: Option<&str>) -> anyhow::Result<()> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn get_active_tools(&self) -> anyhow::Result<Vec<String>> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn get_all_tools(&self) -> anyhow::Result<Vec<ToolInfo>> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn get_settings(&self) -> anyhow::Result<Settings> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn set_active_tools(&self, _tool_names: Vec<String>) -> anyhow::Result<()> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    // registerTool() is valid during extension load; refresh is only needed post-bind.
    fn refresh_tools(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn get_commands(&self) -> anyhow::Result<Vec<SlashCommandInfo>> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    async fn set_model(&self, _model: Model) -> anyhow::Result<bool> {
        anyhow::bail!("Extension runtime not initialized")
    }
    fn get_thinking_level(&self) -> anyhow::Result<ThinkingLevel> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn set_thinking_level(&self, _level: ThinkingLevel) -> anyhow::Result<()> {
        anyhow::bail!(NOT_INITIALIZED)
    }
}

/// Pre-bind registration backend: queue registrations so `bindCore()` can flush them once the
/// model registry is available. `bindCore()` replaces it with direct calls.
struct QueueingRuntimeHooks {
    runtime: std::sync::Weak<ExtensionRuntime>,
}

impl ExtensionRuntimeHooks for QueueingRuntimeHooks {
    fn create_context(&self) -> anyhow::Result<ExtensionContext> {
        anyhow::bail!(NOT_INITIALIZED)
    }
    fn register_provider(&self, name: &str, config: ProviderConfig, extension_path: Option<&str>) -> anyhow::Result<()> {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.pending_provider_registrations.lock().push(PendingProviderRegistration {
                name: name.to_string(),
                config,
                extension_path: extension_path.unwrap_or("<unknown>").to_string(),
            });
        }
        Ok(())
    }
    fn register_native_provider(&self, provider: Arc<dyn Provider>, extension_path: Option<&str>) -> anyhow::Result<()> {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.pending_native_provider_registrations.lock().push(PendingNativeProviderRegistration {
                provider,
                extension_path: extension_path.unwrap_or("<unknown>").to_string(),
            });
        }
        Ok(())
    }
    fn unregister_provider(&self, name: &str, _extension_path: Option<&str>) -> anyhow::Result<()> {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.pending_provider_registrations.lock().retain(|r| r.name != name);
            runtime.pending_native_provider_registrations.lock().retain(|r| r.provider.id() != name);
        }
        Ok(())
    }
    fn register_virtual_model(&self, definition: VirtualModelDefinition, extension_path: Option<&str>) -> anyhow::Result<()> {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.pending_virtual_model_registrations.lock().push(PendingVirtualModelRegistration {
                definition,
                extension_path: extension_path.unwrap_or("<unknown>").to_string(),
            });
        }
        Ok(())
    }
    fn unregister_virtual_model(&self, provider: &str, id: &str) -> anyhow::Result<()> {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime
                .pending_virtual_model_registrations
                .lock()
                .retain(|r| r.definition.provider != provider || r.definition.id != id);
        }
        Ok(())
    }
}

/// Create a runtime with throwing stubs for action methods.
/// Runner.bindCore() replaces these with real implementations.
pub fn create_extension_runtime() -> Arc<ExtensionRuntime> {
    Arc::new_cyclic(|weak| ExtensionRuntime {
        flag_values: Mutex::new(IndexMap::new()),
        pending_provider_registrations: Mutex::new(Vec::new()),
        pending_native_provider_registrations: Mutex::new(Vec::new()),
        pending_virtual_model_registrations: Mutex::new(Vec::new()),
        mcp_servers: Arc::new(McpServerRegistry::new()),
        actions: RwLock::new(Arc::new(NotInitializedActions)),
        hooks: RwLock::new(Arc::new(QueueingRuntimeHooks { runtime: weak.clone() })),
        stale_message: Mutex::new(None),
        event_bus_subscriptions: Mutex::new(Vec::new()),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadState {
    Loading,
    Active,
    Failed,
}

type RuntimeChange = Box<dyn FnOnce() -> anyhow::Result<()> + Send>;

/// The `pi` object of one extension (TS `createExtensionAPI`).
/// Registration methods write to the extension object; action methods delegate to the shared runtime.
pub struct LoaderExtensionApi {
    extension: ExtensionRef,
    extension_path: String,
    runtime: Arc<ExtensionRuntime>,
    cwd: String,
    event_bus: Arc<dyn EventBus>,
    state: Mutex<LoadState>,
    pending_flag_values: Mutex<IndexMap<String, FlagValue>>,
    pending_runtime_changes: Mutex<Vec<RuntimeChange>>,
    loading_unsubscribers: Mutex<Vec<Unsubscribe>>,
}

impl LoaderExtensionApi {
    fn new(extension: ExtensionRef, runtime: Arc<ExtensionRuntime>, cwd: &str, event_bus: Arc<dyn EventBus>) -> Arc<Self> {
        let extension_path = extension.read().path.clone();
        Arc::new(Self {
            extension,
            extension_path,
            runtime,
            cwd: cwd.to_string(),
            event_bus,
            state: Mutex::new(LoadState::Loading),
            pending_flag_values: Mutex::new(IndexMap::new()),
            pending_runtime_changes: Mutex::new(Vec::new()),
            loading_unsubscribers: Mutex::new(Vec::new()),
        })
    }

    /// Path of the extension this API belongs to.
    pub fn extension_path(&self) -> &str {
        &self.extension_path
    }

    /// The extension this API registers into.
    pub fn extension(&self) -> &ExtensionRef {
        &self.extension
    }

    /// The working directory extensions were loaded for.
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// The shared extension runtime.
    pub fn runtime(&self) -> &Arc<ExtensionRuntime> {
        &self.runtime
    }

    fn assert_active(&self) -> anyhow::Result<()> {
        if *self.state.lock() == LoadState::Failed {
            anyhow::bail!("Extension \"{}\" failed to load and its API is no longer active.", self.extension_path);
        }
        self.runtime.assert_active()
    }

    fn is_loading(&self) -> bool {
        *self.state.lock() == LoadState::Loading
    }

    fn apply_runtime_change(&self, change: RuntimeChange) -> anyhow::Result<()> {
        if self.is_loading() {
            self.pending_runtime_changes.lock().push(change);
            Ok(())
        } else {
            change()
        }
    }

    fn clear_pending(&self) {
        self.pending_flag_values.lock().clear();
        self.pending_runtime_changes.lock().clear();
        self.loading_unsubscribers.lock().clear();
    }

    fn commit(&self) -> anyhow::Result<()> {
        if !self.is_loading() {
            return Ok(());
        }
        self.runtime.assert_active()?;
        {
            let pending = std::mem::take(&mut *self.pending_flag_values.lock());
            let mut values = self.runtime.flag_values.lock();
            for (name, value) in pending {
                values.entry(name).or_insert(value);
            }
        }
        let changes = std::mem::take(&mut *self.pending_runtime_changes.lock());
        *self.state.lock() = LoadState::Active;
        let mut result = Ok(());
        for apply in changes {
            if let Err(e) = apply()
                && result.is_ok()
            {
                result = Err(e);
            }
        }
        self.clear_pending();
        result
    }

    fn discard(&self) {
        if !self.is_loading() {
            return;
        }
        *self.state.lock() = LoadState::Failed;
        let unsubscribers = std::mem::take(&mut *self.loading_unsubscribers.lock());
        for unsubscribe in unsubscribers {
            unsubscribe();
        }
        self.clear_pending();
    }

    fn hooks(&self) -> Arc<dyn ExtensionRuntimeHooks> {
        self.runtime.hooks.read().clone()
    }
}

/// `pi.events`: the shared bus with stale checks and subscription tracking.
struct ExtensionEventBus {
    api: std::sync::Weak<LoaderExtensionApi>,
    inner: Arc<dyn EventBus>,
    runtime: Arc<ExtensionRuntime>,
}

impl EventBus for ExtensionEventBus {
    fn emit(&self, channel: &str, data: Value) {
        if let Some(api) = self.api.upgrade()
            && let Err(e) = api.assert_active()
        {
            tracing::warn!("pi.events.emit({channel}) ignored: {e}");
            return;
        }
        self.inner.emit(channel, data);
    }

    fn on(&self, channel: &str, handler: Arc<dyn Fn(Value) + Send + Sync>) -> Arc<dyn Fn() + Send + Sync> {
        let api = self.api.upgrade();
        if let Some(api) = &api
            && let Err(e) = api.assert_active()
        {
            tracing::warn!("pi.events.on({channel}) ignored: {e}");
            return Arc::new(|| {});
        }
        let unsubscribe = self.runtime.track_event_bus_subscription(self.inner.on(channel, handler));
        if let Some(api) = api
            && api.is_loading()
        {
            api.loading_unsubscribers.lock().push(unsubscribe.clone());
        }
        unsubscribe
    }
}

#[async_trait::async_trait]
impl ExtensionAPI for Arc<LoaderExtensionApi> {
    fn on(&self, event: &str, handler: ExtensionHandler) -> anyhow::Result<Unsubscribe> {
        (**self).on(event, handler)
    }
    fn on_project_trust(&self, handler: ProjectTrustHandler) -> anyhow::Result<Unsubscribe> {
        (**self).on_project_trust(handler)
    }
    fn register_tool(&self, tool: ToolDefinition) -> anyhow::Result<()> {
        (**self).register_tool(tool)
    }
    fn register_command(&self, name: &str, options: CommandOptions) -> anyhow::Result<()> {
        (**self).register_command(name, options)
    }
    fn register_shortcut(&self, shortcut: &str, options: ShortcutOptions) -> anyhow::Result<()> {
        (**self).register_shortcut(shortcut, options)
    }
    fn register_flag(&self, name: &str, options: FlagOptions) -> anyhow::Result<()> {
        (**self).register_flag(name, options)
    }
    fn get_flag(&self, name: &str) -> anyhow::Result<Option<FlagValue>> {
        (**self).get_flag(name)
    }
    fn register_message_renderer(&self, custom_type: &str, renderer: MessageRenderer) -> anyhow::Result<()> {
        (**self).register_message_renderer(custom_type, renderer)
    }
    fn register_markdown_transformer(&self, transformer: MarkdownTransformer) -> anyhow::Result<()> {
        (**self).register_markdown_transformer(transformer)
    }
    fn register_entry_renderer(&self, custom_type: &str, renderer: EntryRenderer) -> anyhow::Result<()> {
        (**self).register_entry_renderer(custom_type, renderer)
    }
    fn register_tool_renderer(&self, resolver: ToolRendererResolver) -> anyhow::Result<()> {
        (**self).register_tool_renderer(resolver)
    }
    fn send_message(&self, message: CustomMessageInput, options: SendMessageOptions) -> anyhow::Result<()> {
        (**self).send_message(message, options)
    }
    fn send_user_message(&self, content: UserContent, options: SendUserMessageOptions) -> anyhow::Result<()> {
        (**self).send_user_message(content, options)
    }
    fn append_entry(&self, custom_type: &str, data: Option<Value>) -> anyhow::Result<()> {
        (**self).append_entry(custom_type, data)
    }
    fn set_session_name(&self, name: &str) -> anyhow::Result<()> {
        (**self).set_session_name(name)
    }
    fn get_session_name(&self) -> anyhow::Result<Option<String>> {
        (**self).get_session_name()
    }
    fn set_label(&self, entry_id: &str, label: Option<&str>) -> anyhow::Result<()> {
        (**self).set_label(entry_id, label)
    }
    async fn exec(&self, command: &str, args: &[String], options: ExecOptions) -> anyhow::Result<ExecResult> {
        (**self).exec(command, args, options).await
    }
    fn get_active_tools(&self) -> anyhow::Result<Vec<String>> {
        (**self).get_active_tools()
    }
    fn get_all_tools(&self) -> anyhow::Result<Vec<ToolInfo>> {
        (**self).get_all_tools()
    }
    fn get_settings(&self) -> anyhow::Result<Settings> {
        (**self).get_settings()
    }
    fn set_active_tools(&self, tool_names: Vec<String>) -> anyhow::Result<()> {
        (**self).set_active_tools(tool_names)
    }
    fn get_commands(&self) -> anyhow::Result<Vec<SlashCommandInfo>> {
        (**self).get_commands()
    }
    async fn set_model(&self, model: Model) -> anyhow::Result<bool> {
        (**self).set_model(model).await
    }
    fn get_thinking_level(&self) -> anyhow::Result<ThinkingLevel> {
        (**self).get_thinking_level()
    }
    fn set_thinking_level(&self, level: ThinkingLevel) -> anyhow::Result<()> {
        (**self).set_thinking_level(level)
    }
    fn register_provider(&self, registration: ProviderRegistration) -> anyhow::Result<()> {
        (**self).register_provider(registration)
    }
    fn unregister_provider(&self, name: &str) -> anyhow::Result<()> {
        (**self).unregister_provider(name)
    }
    fn register_mcp_server(&self, name: &str, config: McpServerConfig) -> anyhow::Result<()> {
        (**self).register_mcp_server(name, config)
    }
    fn unregister_mcp_server(&self, name: &str) -> anyhow::Result<()> {
        (**self).unregister_mcp_server(name)
    }
    fn get_mcp_servers(&self) -> anyhow::Result<Vec<RegisteredMcpServer>> {
        (**self).get_mcp_servers()
    }
    fn register_virtual_model(&self, model: ExtensionVirtualModel) -> anyhow::Result<()> {
        (**self).register_virtual_model(model)
    }
    fn unregister_virtual_model(&self, provider: &str, id: &str) -> anyhow::Result<()> {
        (**self).unregister_virtual_model(provider, id)
    }
    fn extension_path(&self) -> Option<String> {
        Some(self.extension_path.clone())
    }
    fn events(&self) -> Arc<dyn EventBus> {
        Arc::new(ExtensionEventBus { api: Arc::downgrade(self), inner: self.event_bus.clone(), runtime: self.runtime.clone() })
    }
}

impl LoaderExtensionApi {
    // Registration methods - write to extension

    pub fn on(&self, event: &str, handler: ExtensionHandler) -> anyhow::Result<Unsubscribe> {
        // A fresh wrapper per registration, so each registration is removed individually.
        let registered: ExtensionHandler = Arc::new(move |e, c| handler(e, c));
        self.push_handler(event, HandlerFn::Event(registered))
    }

    pub fn on_project_trust(&self, handler: ProjectTrustHandler) -> anyhow::Result<Unsubscribe> {
        let registered: ProjectTrustHandler = Arc::new(move |e, c| handler(e, c));
        self.push_handler("project_trust", HandlerFn::ProjectTrust(registered))
    }

    fn push_handler(&self, event: &str, handler: HandlerFn) -> anyhow::Result<Unsubscribe> {
        self.assert_active()?;
        let ptr = handler_ptr(&handler);
        self.extension.write().handlers.entry(event.to_string()).or_default().push(handler);
        let extension = Arc::downgrade(&self.extension);
        let event = event.to_string();
        Ok(Arc::new(move || {
            let Some(extension) = extension.upgrade() else { return };
            let mut ext = extension.write();
            let Some(handlers) = ext.handlers.get_mut(&event) else { return };
            let Some(index) = handlers.iter().position(|h| handler_ptr(h) == ptr) else { return };
            handlers.remove(index);
            if handlers.is_empty() {
                ext.handlers.shift_remove(&event);
            }
        }))
    }

    pub fn register_tool(&self, tool: ToolDefinition) -> anyhow::Result<()> {
        self.assert_active()?;
        if !tool.parameters.is_object() {
            anyhow::bail!(
                "Tool \"{}\" registered by extension \"{}\" must define an object parameter schema.",
                tool.name,
                self.extension_path
            );
        }
        {
            let mut ext = self.extension.write();
            let source_info = ext.source_info.clone();
            ext.tools.insert(tool.name.clone(), RegisteredTool { definition: tool, source_info });
        }
        self.runtime.actions().refresh_tools()
    }

    pub fn register_command(&self, name: &str, options: CommandOptions) -> anyhow::Result<()> {
        self.assert_active()?;
        if name.is_empty() {
            anyhow::bail!(
                "Command registered by extension \"{}\" must have a non-empty string name. Use pi.registerCommand(\"name\", {{ description, handler }}).",
                self.extension_path
            );
        }
        let mut ext = self.extension.write();
        let source_info = ext.source_info.clone();
        ext.commands.insert(
            name.to_string(),
            RegisteredCommand {
                name: name.to_string(),
                source_info,
                description: options.description,
                get_argument_completions: options.get_argument_completions,
                handler: options.handler,
            },
        );
        Ok(())
    }

    pub fn register_shortcut(&self, shortcut: &str, options: ShortcutOptions) -> anyhow::Result<()> {
        self.assert_active()?;
        self.extension.write().shortcuts.insert(
            shortcut.to_string(),
            ExtensionShortcut {
                shortcut: shortcut.to_string(),
                description: options.description,
                handler: options.handler,
                extension_path: self.extension_path.clone(),
            },
        );
        Ok(())
    }

    pub fn register_flag(&self, name: &str, options: FlagOptions) -> anyhow::Result<()> {
        self.assert_active()?;
        if let Some(default) = &options.default {
            let (expected, got) = match (options.flag_type, default) {
                (FlagType::Boolean, FlagValue::String(_)) => ("boolean", "string"),
                (FlagType::String, FlagValue::Bool(_)) => ("string", "boolean"),
                _ => ("", ""),
            };
            if !expected.is_empty() {
                anyhow::bail!("Invalid default for flag \"{name}\": expected {expected}, got {got}");
            }
        }
        self.extension.write().flags.insert(
            name.to_string(),
            ExtensionFlag {
                name: name.to_string(),
                description: options.description.clone(),
                flag_type: options.flag_type,
                default: options.default.clone(),
                extension_path: self.extension_path.clone(),
            },
        );
        if let Some(default) = options.default
            && !self.runtime.flag_values.lock().contains_key(name)
        {
            if self.is_loading() {
                self.pending_flag_values.lock().entry(name.to_string()).or_insert(default);
            } else {
                self.runtime.flag_values.lock().insert(name.to_string(), default);
            }
        }
        Ok(())
    }

    pub fn register_message_renderer(&self, custom_type: &str, renderer: MessageRenderer) -> anyhow::Result<()> {
        self.assert_active()?;
        self.extension.write().message_renderers.insert(custom_type.to_string(), renderer);
        Ok(())
    }

    pub fn register_markdown_transformer(&self, transformer: MarkdownTransformer) -> anyhow::Result<()> {
        self.assert_active()?;
        self.extension.write().markdown_transformer = Some(transformer);
        Ok(())
    }

    pub fn register_entry_renderer(&self, custom_type: &str, renderer: EntryRenderer) -> anyhow::Result<()> {
        self.assert_active()?;
        self.extension.write().entry_renderers.insert(custom_type.to_string(), renderer);
        Ok(())
    }

    pub fn register_tool_renderer(&self, resolver: ToolRendererResolver) -> anyhow::Result<()> {
        self.assert_active()?;
        self.extension.write().tool_renderers.push(resolver);
        Ok(())
    }

    // Flag access - checks extension registered it, reads from runtime
    pub fn get_flag(&self, name: &str) -> anyhow::Result<Option<FlagValue>> {
        self.assert_active()?;
        if !self.extension.read().flags.contains_key(name) {
            return Ok(None);
        }
        if let Some(v) = self.runtime.flag_values.lock().get(name) {
            return Ok(Some(v.clone()));
        }
        Ok(self.pending_flag_values.lock().get(name).cloned())
    }

    // Action methods - delegate to shared runtime

    pub fn send_message(&self, message: CustomMessageInput, options: SendMessageOptions) -> anyhow::Result<()> {
        self.assert_active()?;
        self.runtime.actions().send_message(message, options)
    }

    pub fn send_user_message(&self, content: UserContent, options: SendUserMessageOptions) -> anyhow::Result<()> {
        self.assert_active()?;
        self.runtime.actions().send_user_message(content, options)
    }

    pub fn append_entry(&self, custom_type: &str, data: Option<Value>) -> anyhow::Result<()> {
        self.assert_active()?;
        self.runtime.actions().append_entry(custom_type, data)
    }

    pub fn set_session_name(&self, name: &str) -> anyhow::Result<()> {
        self.assert_active()?;
        self.runtime.actions().set_session_name(name)
    }

    pub fn get_session_name(&self) -> anyhow::Result<Option<String>> {
        self.assert_active()?;
        self.runtime.actions().get_session_name()
    }

    pub fn set_label(&self, entry_id: &str, label: Option<&str>) -> anyhow::Result<()> {
        self.assert_active()?;
        self.runtime.actions().set_label(entry_id, label)
    }

    pub async fn exec(&self, command: &str, args: &[String], options: ExecOptions) -> anyhow::Result<ExecResult> {
        self.assert_active()?;
        let cwd = options.cwd.clone().unwrap_or_else(|| self.cwd.clone());
        exec_command(command, args, &cwd, Some(&options)).await
    }

    pub fn get_active_tools(&self) -> anyhow::Result<Vec<String>> {
        self.assert_active()?;
        self.runtime.actions().get_active_tools()
    }

    pub fn get_all_tools(&self) -> anyhow::Result<Vec<ToolInfo>> {
        self.assert_active()?;
        self.runtime.actions().get_all_tools()
    }

    pub fn get_settings(&self) -> anyhow::Result<Settings> {
        self.assert_active()?;
        self.runtime.actions().get_settings()
    }

    pub fn set_active_tools(&self, tool_names: Vec<String>) -> anyhow::Result<()> {
        self.assert_active()?;
        self.runtime.actions().set_active_tools(tool_names)
    }

    pub fn get_commands(&self) -> anyhow::Result<Vec<SlashCommandInfo>> {
        self.assert_active()?;
        self.runtime.actions().get_commands()
    }

    pub async fn set_model(&self, model: Model) -> anyhow::Result<bool> {
        self.assert_active()?;
        self.runtime.actions().set_model(model).await
    }

    pub fn get_thinking_level(&self) -> anyhow::Result<ThinkingLevel> {
        self.assert_active()?;
        self.runtime.actions().get_thinking_level()
    }

    pub fn set_thinking_level(&self, level: ThinkingLevel) -> anyhow::Result<()> {
        self.assert_active()?;
        self.runtime.actions().set_thinking_level(level)
    }

    pub fn register_provider(&self, registration: ProviderRegistration) -> anyhow::Result<()> {
        self.assert_active()?;
        let runtime = self.runtime.clone();
        let path = self.extension_path.clone();
        match registration {
            ProviderRegistration::Config { name, config } => self.apply_runtime_change(Box::new(move || {
                runtime.hooks.read().clone().register_provider(&name, config, Some(&path))
            })),
            ProviderRegistration::Native(provider) => self.apply_runtime_change(Box::new(move || {
                runtime.hooks.read().clone().register_native_provider(provider, Some(&path))
            })),
        }
    }

    pub fn unregister_provider(&self, name: &str) -> anyhow::Result<()> {
        self.assert_active()?;
        let runtime = self.runtime.clone();
        let path = self.extension_path.clone();
        let name = name.to_string();
        self.apply_runtime_change(Box::new(move || runtime.hooks.read().clone().unregister_provider(&name, Some(&path))))
    }

    pub fn register_mcp_server(&self, name: &str, config: McpServerConfig) -> anyhow::Result<()> {
        self.assert_active()?;
        let raw = serde_json::to_value(&config)?;
        let validated = validate_mcp_server_config(name, raw).into_result().map_err(|e| {
            anyhow::anyhow!("Invalid MCP server registered by extension \"{}\": {e}", self.extension_path)
        })?;
        if let Some(owner) = self.runtime.mcp_servers.get(name).map(|s| s.extension_path)
            && owner != self.extension_path
        {
            anyhow::bail!("MCP server \"{name}\" is already registered by extension \"{owner}\"");
        }
        // Names that differ only in `-` and `_` would share a namespace.
        if let Some(clash) = self
            .runtime
            .mcp_servers
            .list()
            .into_iter()
            .find(|s| s.name != name && mcp_namespace(&s.name) == mcp_namespace(name))
        {
            anyhow::bail!("MCP server \"{name}\" conflicts with registered server \"{}\"", clash.name);
        }
        let server = RegisteredMcpServer { name: name.to_string(), config: validated, extension_path: self.extension_path.clone() };
        let registry = self.runtime.mcp_servers.clone();
        self.apply_runtime_change(Box::new(move || {
            registry.register(&server);
            Ok(())
        }))
    }

    pub fn unregister_mcp_server(&self, name: &str) -> anyhow::Result<()> {
        self.assert_active()?;
        let registry = self.runtime.mcp_servers.clone();
        let name = name.to_string();
        let path = self.extension_path.clone();
        self.apply_runtime_change(Box::new(move || {
            registry.unregister(&name, &path);
            Ok(())
        }))
    }

    pub fn get_mcp_servers(&self) -> anyhow::Result<Vec<RegisteredMcpServer>> {
        self.assert_active()?;
        Ok(self.runtime.mcp_servers.list())
    }

    pub fn register_virtual_model(&self, model: ExtensionVirtualModel) -> anyhow::Result<()> {
        self.assert_active()?;
        // Routing runs after the runner binds, so the context is created per request. The state
        // comes from the session branch that this router wrote.
        let runtime = Arc::downgrade(&self.runtime);
        let route = model.route.clone();
        let definition = VirtualModelDefinition {
            provider: model.provider,
            id: model.id,
            name: model.name,
            thinking_levels: model.thinking_levels,
            context_window: model.context_window,
            max_tokens: model.max_tokens,
            input: model.input,
            route: Arc::new(move |request| {
                let ctx = match runtime.upgrade() {
                    Some(runtime) => runtime.create_context(),
                    None => Err(anyhow::anyhow!(NOT_INITIALIZED)),
                };
                let route = route.clone();
                Box::pin(async move { route(request, ctx?).await })
            }),
        };
        let runtime = self.runtime.clone();
        let path = self.extension_path.clone();
        self.apply_runtime_change(Box::new(move || {
            runtime.hooks.read().clone().register_virtual_model(definition, Some(&path))
        }))
    }

    pub fn unregister_virtual_model(&self, provider: &str, id: &str) -> anyhow::Result<()> {
        self.assert_active()?;
        let runtime = self.runtime.clone();
        let (provider, id) = (provider.to_string(), id.to_string());
        self.apply_runtime_change(Box::new(move || runtime.hooks.read().clone().unregister_virtual_model(&provider, &id)))
    }
}

fn handler_ptr(handler: &HandlerFn) -> usize {
    match handler {
        HandlerFn::Event(h) => Arc::as_ptr(h) as *const () as usize,
        HandlerFn::ProjectTrust(h) => Arc::as_ptr(h) as *const () as usize,
    }
}

/// Create an Extension object with empty collections.
fn create_extension(extension_path: &str, resolved_path: &str) -> ExtensionRef {
    let source = get_synthetic_path_source(extension_path).unwrap_or_else(|| "local".to_string());
    let base_dir = if is_synthetic_path(extension_path) { None } else { Some(node_dirname(resolved_path)) };
    Arc::new(RwLock::new(Extension {
        path: extension_path.to_string(),
        resolved_path: resolved_path.to_string(),
        hidden: None,
        replaceable: None,
        source_info: create_synthetic_source_info(
            extension_path,
            &CreateSyntheticSourceInfoOptions { source, scope: None, origin: None, base_dir },
        ),
        handlers: IndexMap::new(),
        tools: IndexMap::new(),
        message_renderers: IndexMap::new(),
        tool_renderers: Vec::new(),
        markdown_transformer: None,
        entry_renderers: IndexMap::new(),
        commands: IndexMap::new(),
        flags: IndexMap::new(),
        shortcuts: IndexMap::new(),
    }))
}

async fn initialize_extension(
    factory: &ExtensionFactory,
    extension_path: &str,
    resolved_path: &str,
    cwd: &str,
    event_bus: Arc<dyn EventBus>,
    runtime: Arc<ExtensionRuntime>,
) -> anyhow::Result<ExtensionRef> {
    let extension = create_extension(extension_path, resolved_path);
    let api = LoaderExtensionApi::new(extension.clone(), runtime, cwd, event_bus);
    let api_ref: ExtensionAPIRef = Arc::new(api.clone());
    match factory(api_ref).await.and_then(|()| api.commit()) {
        Ok(()) => {}
        Err(e) => {
            api.discard();
            return Err(e);
        }
    }
    time(&format!("{extension_path} factory"), Some(&TimingLabel::Extensions));
    Ok(extension)
}

async fn load_extension_module(
    extension_path: &str,
    cwd: &str,
    cache_token: Option<&ExtensionCacheToken>,
) -> anyhow::Result<Option<ExtensionFactory>> {
    if is_current_cache_token(cache_token)
        && let Some(factory) = EXTENSION_CACHE.lock().factories.get(extension_path).cloned()
    {
        return Ok(Some(factory));
    }
    let host = JsExtensionHost::shared(cwd).await?;
    let Some(factory) = host.import_factory(extension_path).await? else {
        return Ok(None);
    };
    if is_current_cache_token(cache_token) {
        EXTENSION_CACHE.lock().factories.insert(extension_path.to_string(), factory.clone());
    }
    Ok(Some(factory))
}

async fn load_extension(
    extension_path: &str,
    cwd: &str,
    event_bus: Arc<dyn EventBus>,
    runtime: Arc<ExtensionRuntime>,
    cache_token: Option<&ExtensionCacheToken>,
) -> Result<ExtensionRef, String> {
    let resolved_path =
        resolve_path(extension_path, Some(cwd), Some(&PathInputOptions { normalize_unicode_spaces: Some(true), ..Default::default() }));
    let result = async {
        let factory = load_extension_module(&resolved_path, cwd, cache_token).await?;
        time(&format!("{extension_path} module import"), Some(&TimingLabel::Extensions));
        let Some(factory) = factory else {
            return Ok(Err(format!("Extension does not export a valid factory function: {extension_path}")));
        };
        initialize_extension(&factory, extension_path, &resolved_path, cwd, event_bus, runtime).await.map(Ok)
    }
    .await;
    match result {
        Ok(Ok(extension)) => Ok(extension),
        Ok(Err(message)) => Err(message),
        Err(err) => {
            let (message, _) = crate::core::extensions::runner::error_parts(&err);
            Err(format!("Failed to load extension: {message}"))
        }
    }
}

/// Create an Extension from an inline factory function.
pub async fn load_extension_from_factory(
    factory: &ExtensionFactory,
    cwd: &str,
    event_bus: Arc<dyn EventBus>,
    runtime: Arc<ExtensionRuntime>,
    extension_path: Option<&str>,
) -> anyhow::Result<ExtensionRef> {
    let extension_path = extension_path.unwrap_or("<inline>");
    let resolved_cwd = resolve_path(cwd, None, None);
    initialize_extension(factory, extension_path, extension_path, &resolved_cwd, event_bus, runtime).await
}

/// Load extensions from paths.
async fn load_extensions_internal(
    paths: &[String],
    cwd: &str,
    event_bus: Option<Arc<dyn EventBus>>,
    runtime: Option<Arc<ExtensionRuntime>>,
    use_cache: bool,
) -> LoadExtensionsResult {
    let mut extensions = Vec::new();
    let mut errors = Vec::new();
    let warnings = Vec::new();
    let cache_token = use_cache.then(|| use_extension_cache_cwd(cwd));
    let resolved_cwd = cache_token.as_ref().map(|t| t.cwd.clone()).unwrap_or_else(|| resolve_path(cwd, None, None));
    let event_bus: Arc<dyn EventBus> = match event_bus {
        Some(bus) => bus,
        None => {
            let bus = create_event_bus();
            Arc::new(ControllerBus(bus))
        }
    };
    let runtime = runtime.unwrap_or_else(create_extension_runtime);

    for ext_path in paths {
        match load_extension(ext_path, &resolved_cwd, event_bus.clone(), runtime.clone(), cache_token.as_ref()).await {
            Ok(extension) => extensions.push(extension),
            Err(error) => errors.push(ExtensionLoadError { path: ext_path.clone(), error }),
        }
    }

    LoadExtensionsResult { extensions, errors, warnings: Some(warnings), runtime }
}

/// Adapter from the event bus controller to the plain bus trait object.
struct ControllerBus(Arc<dyn crate::core::event_bus::EventBusController>);

impl EventBus for ControllerBus {
    fn emit(&self, channel: &str, data: Value) {
        self.0.emit(channel, data)
    }
    fn on(&self, channel: &str, handler: Arc<dyn Fn(Value) + Send + Sync>) -> Arc<dyn Fn() + Send + Sync> {
        self.0.on(channel, handler)
    }
}

pub async fn load_extensions(
    paths: &[String],
    cwd: &str,
    event_bus: Option<Arc<dyn EventBus>>,
    runtime: Option<Arc<ExtensionRuntime>>,
) -> anyhow::Result<LoadExtensionsResult> {
    Ok(load_extensions_internal(paths, cwd, event_bus, runtime, false).await)
}

pub async fn load_extensions_cached(
    paths: &[String],
    cwd: &str,
    event_bus: Option<Arc<dyn EventBus>>,
    runtime: Option<Arc<ExtensionRuntime>>,
) -> anyhow::Result<LoadExtensionsResult> {
    Ok(load_extensions_internal(paths, cwd, event_bus, runtime, true).await)
}

fn is_extension_file(name: &str) -> bool {
    name.ends_with(".ts") || name.ends_with(".js")
}

/// Resolve extension entry points from a directory.
///
/// Checks for:
/// 1. package.json with "pi.extensions" field -> returns declared paths
/// 2. index.ts or index.js -> returns the index file
///
/// Returns resolved paths or None if no entry points found.
fn resolve_extension_entries(dir: &str) -> Option<Vec<String>> {
    // Check for package.json with "pi" field first
    let package_json_path = node_join(&[dir, "package.json"]);
    if Path::new(&package_json_path).exists()
        && let Some(manifest) = read_pi_manifest(&package_json_path)
        && let Some(declared) = manifest.extensions.filter(|e| !e.is_empty())
    {
        let entries: Vec<String> = declared
            .iter()
            .map(|p| node_resolve(&[dir, p]))
            .filter(|p| Path::new(p).exists())
            .collect();
        if !entries.is_empty() {
            return Some(entries);
        }
    }

    // Check for index.ts or index.js
    let index_ts = node_join(&[dir, "index.ts"]);
    let index_js = node_join(&[dir, "index.js"]);
    if Path::new(&index_ts).exists() {
        return Some(vec![index_ts]);
    }
    if Path::new(&index_js).exists() {
        return Some(vec![index_js]);
    }
    None
}

/// Discover extensions in a directory.
///
/// Discovery rules:
/// 1. Direct files: `extensions/*.ts` or `*.js` → load
/// 2. Subdirectory with index: `extensions/*/index.ts` or `index.js` → load
/// 3. Subdirectory with package.json: `extensions/*/package.json` with "pi" field → load what it declares
///
/// No recursion beyond one level. Complex packages must use package.json manifest.
fn discover_extensions_in_dir(dir: &str) -> Vec<String> {
    if !Path::new(dir).exists() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut discovered = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { return Vec::new() };
        let name = entry.file_name().to_string_lossy().into_owned();
        let entry_path = node_join(&[dir, &name]);
        let Ok(file_type) = entry.file_type() else { continue };

        // 1. Direct files: *.ts or *.js
        if (file_type.is_file() || file_type.is_symlink()) && is_extension_file(&name) {
            discovered.push(entry_path);
            continue;
        }

        // 2 & 3. Subdirectories
        if (file_type.is_dir() || file_type.is_symlink())
            && let Some(entries) = resolve_extension_entries(&entry_path)
        {
            discovered.extend(entries);
        }
    }
    discovered
}

/// Discover and load extensions from standard locations.
pub async fn discover_and_load_extensions(
    configured_paths: &[String],
    cwd: &str,
    agent_dir: Option<&str>,
    event_bus: Option<Arc<dyn EventBus>>,
) -> anyhow::Result<LoadExtensionsResult> {
    let resolved_cwd = resolve_path(cwd, None, None);
    let agent_dir = agent_dir.map(str::to_string).unwrap_or_else(get_agent_dir);
    let resolved_agent_dir = resolve_path(&agent_dir, None, None);
    let mut all_paths: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut add_paths = |paths: Vec<String>| {
        for p in paths {
            let resolved = node_resolve(&[&p]);
            if seen.insert(resolved) {
                all_paths.push(p);
            }
        }
    };

    // 1. Project-local extensions: cwd/${CONFIG_DIR_NAME}/extensions/
    let local_ext_dir = node_join(&[&resolved_cwd, CONFIG_DIR_NAME, "extensions"]);
    add_paths(discover_extensions_in_dir(&local_ext_dir));

    // 2. Global extensions: agentDir/extensions/
    let global_ext_dir = node_join(&[&resolved_agent_dir, "extensions"]);
    add_paths(discover_extensions_in_dir(&global_ext_dir));

    // 3. Explicitly configured paths
    for p in configured_paths {
        let resolved =
            resolve_path(p, Some(&resolved_cwd), Some(&PathInputOptions { normalize_unicode_spaces: Some(true), ..Default::default() }));
        if Path::new(&resolved).is_dir() {
            // Check for package.json with pi manifest or index.ts
            if let Some(entries) = resolve_extension_entries(&resolved) {
                add_paths(entries);
                continue;
            }
            // No explicit entries - discover individual files in directory
            add_paths(discover_extensions_in_dir(&resolved));
            continue;
        }
        add_paths(vec![resolved]);
    }

    load_extensions(&all_paths, &resolved_cwd, event_bus, None).await
}
