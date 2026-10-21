# pi-coding-agent architecture (Rust port)

This crate is the Rust port of `packages/coding-agent` (the `pi` CLI and library). The reference
TypeScript lives in the clone named in `PORTING.md`; read the TS file in full before porting a
module. This document describes how the skeleton is laid out, the ownership and concurrency
model every module follows, the extension API design, and the conventions implementation agents
must keep.

## Status of the skeleton

Every TS module has a Rust module with:

- all exported (and module-local) **types** ported: structs, enums, type aliases, traits, with
  serde attributes that reproduce the TS JSON shapes;
- every exported **function, class constructor, method, getter/setter** declared with its Rust
  signature and a doc comment copied from the TS source;
- non-trivial bodies are `todo!("port <ts-file>:<symbol>")`. These markers are the work items.
  `scripts/port-todo.sh` (repo root) lists the remaining markers per file.

Hand-written (complete) modules: `core/extensions/types.rs` (the extension API contract),
`compat.rs`, `config.rs`, `core/messages.rs`, `modes/rpc/rpc_types.rs`, `core/source_info.rs`,
`core/defaults.rs`, `core/settings_defaults.rs`, `core/slash_commands.rs`, `core/radius.rs`,
`core/experimental.rs`, `core/pi_manifest.rs`, `utils/text.rs`, `utils/paths.rs` (normalize helpers),
`modes/interactive/theme/theme_tokens.rs`, `modes/interactive/theme/theme_schema.rs` (types and
schema; validation is a work item), and `AgentSessionEvent` in `core/agent_session.rs`.

The rest of the skeleton was produced mechanically from the TS AST and then reviewed. Generated
types are faithful in shape but some choices deserve a second look while porting each module:

- Numbers: `number` became `u64` for token counts, `i64` for integer-looking names (counts,
  indices, timestamps, ms, sizes, lines, ...), and `f64` otherwise. `f64` fields use
  `pi_ai::serde_util::js_f64` so whole numbers serialize like JS. Fix any field whose type is wrong.
- Members of a union discriminated by `type` (or another literal property) do not contain that
  field; the enum supplies it (`#[serde(tag = "type")]`). Standalone structs that keep a literal
  field have it as `String` with a doc line `Always "..."`; set it in constructors.
- Inline object/union types became named auxiliary types `<Owner><Member><Param>` (e.g.
  `AgentSessionSendCustomMessageOptions`). Rename freely inside a module, but keep public names
  that other modules already use, or update all users in the same change.
- `T | undefined` / `x?: T` is `Option<T>` with `skip_serializing_if`; `T | null` is `Option<T>`
  serialized as `null`.
- Parameters: `&str` for strings, `&[T]` for arrays, `&T` for crate structs, owned values for
  `Arc`s, closures, `Value` and `Copy` types. Optional parameters are `Option<...>`. Adjust a
  signature when the port needs it (e.g. `&mut`), and update callers.
- Functions that `throw` (and all async functions) return `anyhow::Result`.
- `Promise<T>` in a field became `Shared<BoxFuture<Result<T, Arc<anyhow::Error>>>>`; prefer a
  better-fitting primitive (`tokio::sync::watch`, `Notify`) when porting.
- Types that other crates do not export yet are placeholders in `src/compat.rs`. Replace them
  with the real types as the crates catch up (see "Needed from other crates" below).

## Module map

TS `src/<dir>/<file>.ts` maps to `src/<dir>/<file>.rs` with `-` replaced by `_`
(`core/agent-session.ts` → `core::agent_session`). `index.ts` files become the directory's
`mod.rs` (they hold the re-exports, as in TS). Exceptions:

| TS | Rust | Why |
|---|---|---|
| `src/index.ts` | `src/lib.rs` | public API re-exports |
| `src/main.ts` | `src/pi_main.rs` (`pi_coding_agent::main`) | `src/main.rs` is the binary |
| `src/cli.ts` | `src/main.rs` | binary entry point |
| `src/rpc-entry.ts` | not ported | use `pi --mode rpc` (a `pi-rpc` binary can be added later) |
| `src/bun/*` | not ported | Bun runtime plumbing |
| `src/experimental/source-resolver.ts` | not ported | Node module-resolution experiment |
| `core/extensions/jiti-*.ts`, `virtual-modules.ts` | not ported | JS loading is done by `pi-jsrt` |
| `*.lazy.ts`, `extensions/codemode/worker.ts`, `utils/image-resize-worker.ts` | not ported | Node lazy-import / worker shims; call the real module directly |
| `src/compat.rs` | (new) | placeholders for types other crates do not export yet |

Non-TS assets (`core/export-html/template.*`, `vendor/*`, `modes/interactive/theme/*.json`,
`modes/interactive/assets/*`) are not copied yet. The export-html and theme owners embed them with
`include_str!`/`include_bytes!` (put them next to the module, e.g.
`src/core/export_html/template.html`).

The full file-by-file map is at the end of this document.

## Ownership and concurrency model

- **Async runtime**: tokio (multi-threaded). Anything that returns a `Promise` in TS is `async`
  in Rust. Cancellation (`AbortSignal`/`AbortController`) is `tokio_util::sync::CancellationToken`.
- **Classes are shared handles.** A TS class becomes a `struct` used as `Arc<T>`; all methods take
  `&self`, and mutable state lives behind interior mutability (`parking_lot::Mutex`/`RwLock` for
  short critical sections, `tokio::sync::Mutex` only when a lock must be held across `.await`).
  Never hold a lock while calling callbacks, listeners, extension handlers, or other components
  (same rule as pi-tui components). Static constructors that return the class
  (`SessionManager::create`, `SettingsManager::in_memory`, ...) return `Arc<Self>`.
- **Interfaces with methods are traits** (`Arc<dyn Trait>`), e.g. `ResourceLoader`,
  `AuthStorageBackend`, `SettingsStorage`, `EventBus`, `PackageManager`, `ExtensionUIContext`.
  Interfaces that are mostly data with a few callbacks are structs with `Arc<dyn Fn>` fields
  (e.g. `ToolDefinition`, `VirtualModelDefinition`, `ProviderConfig`).
- **Plain data** (`interface` without methods, unions) is `Clone` + serde.
- **Agent**: `pi_agent::Agent` is already a cheap-clone handle; `AgentSession` owns one.
- **AgentSession** (`core::agent_session`) is the central object: `Arc<AgentSession>` owns the
  agent, `Arc<SessionManager>`, `Arc<SettingsManager>`, `Arc<ModelRuntime>`, the resource loader
  and the `Arc<ExtensionRunner>`. Events go out through `subscribe(listener)` as
  `AgentSessionEvent` (serde-tagged like the TS union, so RPC/JSON modes can write them directly).
- **AgentSessionRuntime** (`core::agent_session_runtime`) owns the current `Arc<AgentSession>` and
  replaces it on `/new`, `/resume`, `/fork` and reload. Modes hold `Arc<AgentSessionRuntime>` and
  re-read `runtime.session()` after replacement instead of caching the session.
- **Extensions see the session through traits, never through `Arc<AgentSession>`**: the session
  implements `ExtensionContextActions`, `ExtensionActions` and (interactive/RPC)
  `ExtensionCommandContextActions` with small adapter structs that hold a `Weak<AgentSession>`
  (avoids cycles: session → runner → extension closures → session). `runner.bind_core()` installs
  them, and `ExtensionRunner` must implement `ExtensionContextHost` (the backend of every
  `ExtensionContext` it creates).
- **UI context per mode**: each mode implements `ExtensionUIContext` and passes it via
  `ExtensionBindings { ui_context, mode, command_context_actions, ... }` to
  `AgentSession::bind_extensions`. Interactive mode drives pi-tui components; RPC mode turns
  dialogs into `extension_ui_request` lines and awaits `extension_ui_response`; print/JSON modes
  use the runner's `NoOpUIContext` (`has_ui = false`).
- **TUI**: components implement `pi_tui::tui::Component` (`render(&self, width)`, interior
  mutability). Generated component structs already have an `impl Component` with `todo!()`s.
  TS `dispose()` maps to `Drop` (or an explicit `dispose(&self)` when the TS code calls it).
- **Global state** in TS modules (`let foo` at module level) becomes `static` with
  `LazyLock<Mutex<..>>`/`OnceLock`. Keep it rare and document it.

## Extension API design (`core/extensions/types.rs`)

The contract is complete and documented in the module docs. Summary:

- `ExtensionEvent`: one enum, `#[serde(tag = "type")]`, a variant per TS event, so events
  round-trip to the exact TS JSON. Non-JSON fields (`signal`) are `#[serde(skip)]`.
- `ExtensionHandler = Arc<dyn Fn(ExtensionEvent, ExtensionContext) -> BoxFuture<'static,
  anyhow::Result<Option<ExtensionEventResult>>> + Send + Sync>`. `ExtensionEventResult` has a typed
  variant per result type plus `Json(Value)` for the JS bridge; `EventResultType::from_result`
  converts either form into the typed result the runner expects. `on_event::<E, _>(pi, handler)`
  and `typed_handler` give Rust extensions typed registration via `ExtensionEventPayload`.
- In-place mutation in TS becomes returned replacements: `ToolCallEventResult::input`,
  `BeforeProviderHeadersEventResult::headers`, `BeforeAgentStartEventResult::system_prompt_options`.
  The runner applies them in handler order so later handlers observe earlier changes.
- `project_trust` handlers have their own type (`ProjectTrustHandler`, `on_project_trust`).
- `ExtensionAPI` (the `pi` object) is a trait implemented in `loader.rs`; every method returns
  `anyhow::Result` because TS throws for stale extension instances.
- `ExtensionContext` is a cloneable handle over `ExtensionContextHost` (the runner); all
  accessors check staleness. `ExtensionToolContext`, `ExtensionCommandContext` and
  `ReplacedSessionContext` wrap it and `Deref` to it.
- `ToolDefinition` is a struct with closures (`execute`, `render_call`, `render_result`,
  `prepare_arguments`, `prepare_loadout`, `reaches_tools`); renderers return
  `pi_tui::tui::ComponentRef`; per-row renderer state is `ToolRenderState`
  (`Arc<Mutex<Option<Box<dyn Any + Send + Sync>>>>`).
- `Extension` (loaded extension) is shared as `ExtensionRef = Arc<RwLock<Extension>>`;
  `ExtensionRuntime` is `Arc` with interior mutability; its `hooks` (`ExtensionRuntimeHooks`) queue
  provider/virtual-model registrations until `runner.bind_core()` swaps in the live implementation.
- JS extensions (pi-jsrt bridge, future work in `core/extensions/loader.rs`): the bridge builds an
  `ExtensionAPIRef`-backed JS `pi` object, wraps JS handlers into `ExtensionHandler`s that
  serialize the event to JSON, call into the JS thread, and return `ExtensionEventResult::Json`
  (after reading back mutated `input`/`headers`/`systemPromptOptions`). JS tools become
  `ToolDefinition`s whose closures call into JS; JS components become `Component` impls holding a
  JS handle.

## Conventions for implementation agents

1. Work only in the modules of your group; if you need a signature change in another module,
   change the signature and all its callers in one commit, and keep `cargo check -p pi-coding-agent`
   green. Do not rename public items other groups use without updating them.
2. Replace each `todo!("port ...")` with a faithful port (behavior, error messages, defaults, env
   vars). Do not add new `todo!`s. If something cannot be ported yet, record it in
   `PORTING-STATUS.md`.
3. Add private fields to the generated `struct`s (they are empty with a
   `// TODO(port): private state` marker) and private helper functions as needed.
4. Keep JSON byte-compatible: field order is struct declaration order; check optional vs null.
5. Port the TS tests of your modules (`packages/coding-agent/test/...`) into `#[cfg(test)]` or
   `tests/`.
6. Remove `#![allow(dead_code, unused_variables)]` from `lib.rs` once all groups are done.
7. Commit only your paths (`git add crates/pi-coding-agent/src/<your files>`).

## Suggested work groups

Balanced by TS line count, with the coupling noted. Dependencies point to groups whose types the
group calls (all types exist already, so groups can start in parallel; behavior tests need the
dependency's implementation).

| # | Group | Modules | TS lines |
|---|---|---|---|
| 1 | Session engine | `core::{agent_session, agent_session_runtime, agent_session_services, sdk, nested_tool_calls, cache_warmer, cache_stats, usage_totals, system_prompt, bash_executor, exec, event_bus, output_guard, crash_log, timings, telemetry}` | ~8.0k |
| 2 | Extension runtime | `core::extensions::{runner, loader, mod (index), wrapper}`, `core::mcp_servers`, JS bridge to pi-jsrt | ~3.0k + bridge |
| 3 | Sessions, compaction, export | `core::{session_manager, session_cwd, session_export}`, `core::compaction::*`, `core::export_html::*`, `migrations` | ~5.1k |
| 4 | Settings and resources | `core::{settings_manager, settings_diagnostics, resource_loader, skills, prompt_templates, keybindings, keybindings_schema, key_id_schema, footer_data_provider, trust_manager, project_trust, resolve_config_value}` | ~5.3k |
| 5 | Models and auth | `core::{model_registry, model_runtime, model_resolver, model_config, models_store, provider_composer, remote_catalog_provider, virtual_models, auth_storage, auth_guidance, runtime_credentials, http_dispatcher, client_identity, bug_report, bug_report_upload}` | ~5.3k |
| 6 | Built-in tools | `core::tools::**` (incl. `renderers`), `utils::{shell, child_process, tools_manager, mime, image_*, exif_orientation, photon, tool_result_images, git}` | ~6.5k |
| 7 | Interactive mode core | `modes::interactive::{interactive_mode, chat_viewport, tui_renderer, external_editor, program_status_reporter, model_catalog_refresh, model_search, session_share, bug_report}`, `modes::interactive::theme::{theme, theme_controller, system_theme}` | ~10k |
| 8 | Message and tool components | `modes::interactive::components::{assistant_message, user_message, tool_execution, bash_execution, custom_message, custom_entry, branch_summary_message, compaction_summary_message, skill_invocation_message, diff, footer, markdown_transform, mermaid, status_indicator, visual_truncate, keybinding_hints, dynamic_border, bordered_loader, countdown_timer, themed_text, custom_editor, extension_editor, extension_input, extension_selector, auth_url}` | ~3.6k |
| 9 | Selectors and dialogs | `modes::interactive::components::{config_selector, model_selector, oauth_selector, login_dialog, scoped_models_selector, session_selector, session_selector_search, settings_selector, settings_submenu, theme_selector, thinking_selector, tree_selector, trust_selector, user_message_selector, first_time_setup, radius_login_selector, show_images_selector, armin, earendil_announcement, easter_egg_3d, pi_logo}` | ~9.0k |
| 10 | CLI, print/JSON/RPC modes | `pi_main`, `cli::*`, `modes::{print_mode, json_event}`, `modes::rpc::{rpc_mode, rpc_client, jsonl}`, `package_manager_cli`, `core::package_manager`, remaining `utils::*` (clipboard, changelog, version_check, windows_self_update, zip, html, ansi, syntax_highlight, ...) | ~8.5k |
| 11 | MCP extension | `extensions::mcp::*` (on `pi-mcp`) | ~4.3k |
| 12 | Codemode, tool search, llama | `extensions::{codemode, tool_search, llama}`, `extensions::mod` (on `pi-codemode`/`pi-jsrt`) | ~3.3k |

Group 10 is large but mostly independent leaf code; split `core::package_manager` +
`package_manager_cli` (~3.9k) into its own group if more agents are available.

## Needed from other crates

Placeholders live in `src/compat.rs`; swap each for the real type when it exists.

- **pi-ai**: `AuthOperationOptions`, `OAuthLoginCallbacks`, `OAuthDeviceCodeInfo`,
  `OAuthAuthInfo`, `OAuthSelectPrompt`/`OAuthSelectOption`, `OAuthPrompt` (extension OAuth compat
  types from `compat/extension-oauth-types.ts`). `Models*Options` map to
  `pi_ai::models::ModelsRequestOptions`.
- **pi-tui**: `MarkdownTheme`, `SettingsListTheme`, `SettingsList`, `Input`, `Editor`, `Markdown`,
  `Text` components (exported from `components`), `WheelScrollLines`, `RenderCache`,
  `ImageTranscoder`. Consider a `dispose()` default method on `Component` if TS code relies on
  explicit disposal order.
- **pi-codemode**: `CodemodeJsonSchema` and the rest of the codemode API (crate is empty).
- **pi-jsrt**: extension loading entry points used by `core::extensions::loader` (the crate is not
  a dependency yet because it is mid-port; add `pi-jsrt.workspace = true` when it compiles).

## File-by-file module map

**`cli/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `cli/args.ts` | `cli::args` | 479 |
| `cli/auth-check.ts` | `cli::auth_check` | 73 |
| `cli/auth-command.ts` | `cli::auth_command` | 126 |
| `cli/config-selector.ts` | `cli::config_selector` | 61 |
| `cli/credential-print.ts` | `cli::credential_print` | 90 |
| `cli/file-processor.ts` | `cli::file_processor` | 88 |
| `cli/initial-message.ts` | `cli::initial_message` | 43 |
| `cli/list-models.ts` | `cli::list_models` | 115 |
| `cli/project-trust.ts` | `cli::project_trust` | 62 |
| `cli/session-picker.ts` | `cli::session_picker` | 55 |
| `cli/setup.ts` | `cli::setup` | 13 |
| `cli/startup-ui.ts` | `cli::startup_ui` | 253 |

**`./`**

| TS file | Rust module | TS lines |
|---|---|---|
| `config.ts` | `config` | 656 |

**`core/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `core/agent-session-runtime.ts` | `core::agent_session_runtime` | 447 |
| `core/agent-session-services.ts` | `core::agent_session_services` | 233 |
| `core/agent-session.ts` | `core::agent_session` | 4449 |
| `core/auth-guidance.ts` | `core::auth_guidance` | 25 |
| `core/auth-storage.ts` | `core::auth_storage` | 507 |
| `core/bash-executor.ts` | `core::bash_executor` | 146 |
| `core/bug-report-upload.ts` | `core::bug_report_upload` | 37 |
| `core/bug-report.ts` | `core::bug_report` | 375 |
| `core/cache-stats.ts` | `core::cache_stats` | 174 |
| `core/cache-warmer.ts` | `core::cache_warmer` | 453 |
| `core/client-identity.ts` | `core::client_identity` | 16 |

**`core/compaction/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `core/compaction/branch-summarization.ts` | `core::compaction::branch_summarization` | 380 |
| `core/compaction/compaction.ts` | `core::compaction::compaction` | 1104 |
| `core/compaction/index.ts` | `core::compaction` | 7 |
| `core/compaction/utils.ts` | `core::compaction::utils` | 163 |

**`core/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `core/crash-log.ts` | `core::crash_log` | 170 |
| `core/defaults.ts` | `core::defaults` | 12 |
| `core/diagnostics.ts` | `core::diagnostics` | 15 |
| `core/event-bus.ts` | `core::event_bus` | 33 |
| `core/exec.ts` | `core::exec` | 107 |
| `core/experimental.ts` | `core::experimental` | 3 |

**`core/export-html/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `core/export-html/ansi-to-html.ts` | `core::export_html::ansi_to_html` | 258 |
| `core/export-html/index.ts` | `core::export_html` | 318 |
| `core/export-html/tool-renderer.ts` | `core::export_html::tool_renderer` | 174 |

**`core/extensions/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `core/extensions/index.ts` | `core::extensions` | 225 |
| `core/extensions/loader.ts` | `core::extensions::loader` | 879 |
| `core/extensions/runner.ts` | `core::extensions::runner` | 1565 |
| `core/extensions/types.ts` | `core::extensions::types` | 2304 |
| `core/extensions/wrapper.ts` | `core::extensions::wrapper` | 29 |

**`core/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `core/footer-data-provider.ts` | `core::footer_data_provider` | 388 |
| `core/http-dispatcher.ts` | `core::http_dispatcher` | 113 |
| `core/index.ts` | `core` | 97 |
| `core/key-id-schema.ts` | `core::key_id_schema` | 21 |
| `core/keybindings-schema.ts` | `core::keybindings_schema` | 21 |
| `core/keybindings.ts` | `core::keybindings` | 359 |
| `core/mcp-servers.ts` | `core::mcp_servers` | 346 |
| `core/messages.ts` | `core::messages` | 196 |
| `core/model-config.ts` | `core::model_config` | 219 |
| `core/model-registry.ts` | `core::model_registry` | 244 |
| `core/model-resolver.ts` | `core::model_resolver` | 684 |
| `core/model-runtime.ts` | `core::model_runtime` | 1049 |
| `core/models-store.ts` | `core::models_store` | 147 |
| `core/nested-tool-calls.ts` | `core::nested_tool_calls` | 263 |
| `core/output-guard.ts` | `core::output_guard` | 108 |
| `core/package-manager.ts` | `core::package_manager` | 2766 |
| `core/pi-manifest.ts` | `core::pi_manifest` | 35 |
| `core/project-trust.ts` | `core::project_trust` | 97 |
| `core/prompt-templates.ts` | `core::prompt_templates` | 325 |
| `core/provider-composer.ts` | `core::provider_composer` | 732 |
| `core/radius.ts` | `core::radius` | 11 |
| `core/remote-catalog-provider.ts` | `core::remote_catalog_provider` | 240 |
| `core/resolve-config-value.ts` | `core::resolve_config_value` | 287 |
| `core/resource-loader.ts` | `core::resource_loader` | 1287 |
| `core/runtime-credentials.ts` | `core::runtime_credentials` | 52 |
| `core/sdk.ts` | `core::sdk` | 479 |
| `core/session-cwd.ts` | `core::session_cwd` | 59 |
| `core/session-export.ts` | `core::session_export` | 45 |
| `core/session-manager.ts` | `core::session_manager` | 2013 |
| `core/settings-defaults.ts` | `core::settings_defaults` | 67 |
| `core/settings-diagnostics.ts` | `core::settings_diagnostics` | 25 |
| `core/settings-manager.ts` | `core::settings_manager` | 1443 |
| `core/settings-schema.ts` | `core::settings_schema` | 482 |
| `core/skills.ts` | `core::skills` | 514 |
| `core/slash-commands.ts` | `core::slash_commands` | 44 |
| `core/source-info.ts` | `core::source_info` | 57 |
| `core/system-prompt.ts` | `core::system_prompt` | 229 |
| `core/telemetry.ts` | `core::telemetry` | 13 |
| `core/timings.ts` | `core::timings` | 50 |

**`core/tools/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `core/tools/bash.ts` | `core::tools::bash` | 442 |
| `core/tools/edit-diff.ts` | `core::tools::edit_diff` | 556 |
| `core/tools/edit.ts` | `core::tools::edit` | 220 |
| `core/tools/file-mutation-queue.ts` | `core::tools::file_mutation_queue` | 61 |
| `core/tools/find.ts` | `core::tools::find` | 318 |
| `core/tools/grep.ts` | `core::tools::grep` | 323 |
| `core/tools/index.ts` | `core::tools` | 224 |
| `core/tools/ls.ts` | `core::tools::ls` | 175 |
| `core/tools/output-accumulator.ts` | `core::tools::output_accumulator` | 257 |
| `core/tools/path-utils.ts` | `core::tools::path_utils` | 118 |
| `core/tools/powershell.ts` | `core::tools::powershell` | 67 |
| `core/tools/read.ts` | `core::tools::read` | 224 |
| `core/tools/render-utils.ts` | `core::tools::render_utils` | 185 |

**`core/tools/renderers/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `core/tools/renderers/bash.ts` | `core::tools::renderers::bash` | 109 |
| `core/tools/renderers/edit.ts` | `core::tools::renderers::edit` | 252 |
| `core/tools/renderers/find.ts` | `core::tools::renderers::find` | 77 |
| `core/tools/renderers/grep.ts` | `core::tools::renderers::grep` | 82 |
| `core/tools/renderers/index.ts` | `core::tools::renderers` | 63 |
| `core/tools/renderers/ls.ts` | `core::tools::renderers::ls` | 70 |
| `core/tools/renderers/read.ts` | `core::tools::renderers::read` | 173 |
| `core/tools/renderers/write.ts` | `core::tools::renderers::write` | 181 |

**`core/tools/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `core/tools/tool-definition-wrapper.ts` | `core::tools::tool_definition_wrapper` | 58 |
| `core/tools/truncate.ts` | `core::tools::truncate` | 314 |
| `core/tools/write.ts` | `core::tools::write` | 97 |

**`core/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `core/trust-manager.ts` | `core::trust_manager` | 246 |
| `core/usage-totals.ts` | `core::usage_totals` | 97 |
| `core/virtual-models.ts` | `core::virtual_models` | 238 |

**`extensions/codemode/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `extensions/codemode/execute.ts` | `extensions::codemode::execute` | 850 |
| `extensions/codemode/index.ts` | `extensions::codemode` | 49 |
| `extensions/codemode/renderer.ts` | `extensions::codemode::renderer` | 156 |
| `extensions/codemode/tool.ts` | `extensions::codemode::tool` | 411 |

**`extensions/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `extensions/index.ts` | `extensions` | 14 |

**`extensions/llama/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `extensions/llama/client.ts` | `extensions::llama::client` | 348 |
| `extensions/llama/huggingface.ts` | `extensions::llama::huggingface` | 158 |
| `extensions/llama/index.ts` | `extensions::llama` | 230 |
| `extensions/llama/provider.ts` | `extensions::llama::provider` | 299 |
| `extensions/llama/ui.ts` | `extensions::llama::ui` | 542 |

**`extensions/mcp/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `extensions/mcp/cli.ts` | `extensions::mcp::cli` | 594 |
| `extensions/mcp/config.ts` | `extensions::mcp::config` | 243 |
| `extensions/mcp/index.ts` | `extensions::mcp` | 1347 |
| `extensions/mcp/log.ts` | `extensions::mcp::log` | 69 |
| `extensions/mcp/oauth.ts` | `extensions::mcp::oauth` | 558 |
| `extensions/mcp/resources.ts` | `extensions::mcp::resources` | 339 |
| `extensions/mcp/runtime.ts` | `extensions::mcp::runtime` | 454 |
| `extensions/mcp/tools.ts` | `extensions::mcp::tools` | 391 |
| `extensions/mcp/ui.ts` | `extensions::mcp::ui` | 259 |

**`extensions/tool-search/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `extensions/tool-search/index.ts` | `extensions::tool_search` | 18 |
| `extensions/tool-search/tool.ts` | `extensions::tool_search::tool` | 244 |

**`./`**

| TS file | Rust module | TS lines |
|---|---|---|
| `index.ts` | `crate` | 497 |
| `main.ts` | `pi_main` | 1000 |
| `migrations.ts` | `migrations` | 315 |

**`modes/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `modes/index.ts` | `modes` | 16 |

**`modes/interactive/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `modes/interactive/bug-report.ts` | `modes::interactive::bug_report` | 298 |
| `modes/interactive/chat-viewport.ts` | `modes::interactive::chat_viewport` | 46 |

**`modes/interactive/components/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `modes/interactive/components/armin.ts` | `modes::interactive::components::armin` | 384 |
| `modes/interactive/components/assistant-message.ts` | `modes::interactive::components::assistant_message` | 202 |
| `modes/interactive/components/auth-url.ts` | `modes::interactive::components::auth_url` | 39 |
| `modes/interactive/components/bash-execution.ts` | `modes::interactive::components::bash_execution` | 223 |
| `modes/interactive/components/bordered-loader.ts` | `modes::interactive::components::bordered_loader` | 68 |
| `modes/interactive/components/branch-summary-message.ts` | `modes::interactive::components::branch_summary_message` | 71 |
| `modes/interactive/components/compaction-summary-message.ts` | `modes::interactive::components::compaction_summary_message` | 72 |
| `modes/interactive/components/config-selector.ts` | `modes::interactive::components::config_selector` | 951 |
| `modes/interactive/components/countdown-timer.ts` | `modes::interactive::components::countdown_timer` | 39 |
| `modes/interactive/components/custom-editor.ts` | `modes::interactive::components::custom_editor` | 148 |
| `modes/interactive/components/custom-entry.ts` | `modes::interactive::components::custom_entry` | 69 |
| `modes/interactive/components/custom-message.ts` | `modes::interactive::components::custom_message` | 114 |
| `modes/interactive/components/diff.ts` | `modes::interactive::components::diff` | 147 |
| `modes/interactive/components/dynamic-border.ts` | `modes::interactive::components::dynamic_border` | 25 |
| `modes/interactive/components/earendil-announcement.ts` | `modes::interactive::components::earendil_announcement` | 53 |
| `modes/interactive/components/easter-egg-3d.ts` | `modes::interactive::components::easter_egg_3d` | 1292 |
| `modes/interactive/components/extension-editor.ts` | `modes::interactive::components::extension_editor` | 141 |
| `modes/interactive/components/extension-input.ts` | `modes::interactive::components::extension_input` | 94 |
| `modes/interactive/components/extension-selector.ts` | `modes::interactive::components::extension_selector` | 117 |
| `modes/interactive/components/first-time-setup.ts` | `modes::interactive::components::first_time_setup` | 148 |
| `modes/interactive/components/footer.ts` | `modes::interactive::components::footer` | 302 |
| `modes/interactive/components/index.ts` | `modes::interactive::components` | 37 |
| `modes/interactive/components/keybinding-hints.ts` | `modes::interactive::components::keybinding_hints` | 48 |
| `modes/interactive/components/login-dialog.ts` | `modes::interactive::components::login_dialog` | 238 |
| `modes/interactive/components/markdown-transform.ts` | `modes::interactive::components::markdown_transform` | 29 |
| `modes/interactive/components/mermaid.ts` | `modes::interactive::components::mermaid` | 89 |
| `modes/interactive/components/model-selector.ts` | `modes::interactive::components::model_selector` | 421 |
| `modes/interactive/components/oauth-selector.ts` | `modes::interactive::components::oauth_selector` | 224 |
| `modes/interactive/components/pi-logo.ts` | `modes::interactive::components::pi_logo` | 40 |
| `modes/interactive/components/radius-login-selector.ts` | `modes::interactive::components::radius_login_selector` | 114 |
| `modes/interactive/components/scoped-models-selector.ts` | `modes::interactive::components::scoped_models_selector` | 401 |
| `modes/interactive/components/session-selector-search.ts` | `modes::interactive::components::session_selector_search` | 194 |
| `modes/interactive/components/session-selector.ts` | `modes::interactive::components::session_selector` | 1045 |
| `modes/interactive/components/settings-selector.ts` | `modes::interactive::components::settings_selector` | 1009 |
| `modes/interactive/components/settings-submenu.ts` | `modes::interactive::components::settings_submenu` | 258 |
| `modes/interactive/components/show-images-selector.ts` | `modes::interactive::components::show_images_selector` | 50 |
| `modes/interactive/components/skill-invocation-message.ts` | `modes::interactive::components::skill_invocation_message` | 68 |
| `modes/interactive/components/status-indicator.ts` | `modes::interactive::components::status_indicator` | 123 |
| `modes/interactive/components/theme-selector.ts` | `modes::interactive::components::theme_selector` | 67 |
| `modes/interactive/components/themed-text.ts` | `modes::interactive::components::themed_text` | 32 |
| `modes/interactive/components/thinking-selector.ts` | `modes::interactive::components::thinking_selector` | 154 |
| `modes/interactive/components/tool-execution.ts` | `modes::interactive::components::tool_execution` | 408 |
| `modes/interactive/components/tree-selector.ts` | `modes::interactive::components::tree_selector` | 1428 |
| `modes/interactive/components/trust-selector.ts` | `modes::interactive::components::trust_selector` | 134 |
| `modes/interactive/components/user-message-selector.ts` | `modes::interactive::components::user_message_selector` | 155 |
| `modes/interactive/components/user-message.ts` | `modes::interactive::components::user_message` | 71 |
| `modes/interactive/components/visual-truncate.ts` | `modes::interactive::components::visual_truncate` | 98 |

**`modes/interactive/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `modes/interactive/external-editor.ts` | `modes::interactive::external_editor` | 46 |
| `modes/interactive/interactive-mode.ts` | `modes::interactive::interactive_mode` | 7082 |
| `modes/interactive/model-catalog-refresh.ts` | `modes::interactive::model_catalog_refresh` | 51 |
| `modes/interactive/model-search.ts` | `modes::interactive::model_search` | 21 |
| `modes/interactive/program-status-reporter.ts` | `modes::interactive::program_status_reporter` | 111 |
| `modes/interactive/session-share.ts` | `modes::interactive::session_share` | 224 |

**`modes/interactive/theme/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `modes/interactive/theme/system-theme.ts` | `modes::interactive::theme::system_theme` | 655 |
| `modes/interactive/theme/theme-controller.ts` | `modes::interactive::theme::theme_controller` | 253 |
| `modes/interactive/theme/theme-schema.ts` | `modes::interactive::theme::theme_schema` | 146 |
| `modes/interactive/theme/theme-tokens.ts` | `modes::interactive::theme::theme_tokens` | 121 |
| `modes/interactive/theme/theme.ts` | `modes::interactive::theme::theme` | 1086 |

**`modes/interactive/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `modes/interactive/tui-renderer.ts` | `modes::interactive::tui_renderer` | 81 |

**`modes/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `modes/json-event.ts` | `modes::json_event` | 61 |
| `modes/print-mode.ts` | `modes::print_mode` | 173 |

**`modes/rpc/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `modes/rpc/jsonl.ts` | `modes::rpc::jsonl` | 58 |
| `modes/rpc/rpc-client.ts` | `modes::rpc::rpc_client` | 617 |
| `modes/rpc/rpc-mode.ts` | `modes::rpc::rpc_mode` | 819 |
| `modes/rpc/rpc-types.ts` | `modes::rpc::rpc_types` | 303 |

**`./`**

| TS file | Rust module | TS lines |
|---|---|---|
| `package-manager-cli.ts` | `package_manager_cli` | 1142 |

**`utils/`**

| TS file | Rust module | TS lines |
|---|---|---|
| `utils/abort.ts` | `utils::abort` | 48 |
| `utils/ansi.ts` | `utils::ansi` | 82 |
| `utils/changelog.ts` | `utils::changelog` | 196 |
| `utils/child-process.ts` | `utils::child_process` | 137 |
| `utils/clipboard-command.ts` | `utils::clipboard_command` | 47 |
| `utils/clipboard-image.ts` | `utils::clipboard_image` | 240 |
| `utils/clipboard.ts` | `utils::clipboard` | 150 |
| `utils/deprecation.ts` | `utils::deprecation` | 14 |
| `utils/exif-orientation.ts` | `utils::exif_orientation` | 182 |
| `utils/frontmatter.ts` | `utils::frontmatter` | 40 |
| `utils/fs-watch.ts` | `utils::fs_watch` | 30 |
| `utils/git.ts` | `utils::git` | 226 |
| `utils/html.ts` | `utils::html` | 51 |
| `utils/image-convert.ts` | `utils::image_convert` | 89 |
| `utils/image-process.ts` | `utils::image_process` | 119 |
| `utils/image-resize-core.ts` | `utils::image_resize_core` | 174 |
| `utils/image-resize.ts` | `utils::image_resize` | 122 |
| `utils/json.ts` | `utils::json` | 6 |
| `utils/management-http.ts` | `utils::management_http` | 78 |
| `utils/mime.ts` | `utils::mime` | 116 |
| `utils/open-browser.ts` | `utils::open_browser` | 24 |
| `utils/output-files.ts` | `utils::output_files` | 34 |
| `utils/paths.ts` | `utils::paths` | 140 |
| `utils/photon.ts` | `utils::photon` | 139 |
| `utils/pi-user-agent.ts` | `utils::pi_user_agent` | 6 |
| `utils/shell.ts` | `utils::shell` | 251 |
| `utils/sleep.ts` | `utils::sleep` | 18 |
| `utils/syntax-highlight.ts` | `utils::syntax_highlight` | 219 |
| `utils/text.ts` | `utils::text` | 9 |
| `utils/tool-result-images.ts` | `utils::tool_result_images` | 67 |
| `utils/tools-manager.ts` | `utils::tools_manager` | 401 |
| `utils/version-check.ts` | `utils::version_check` | 109 |
| `utils/windows-self-update.ts` | `utils::windows_self_update` | 84 |
| `utils/wsl.ts` | `utils::wsl` | 15 |
| `utils/zip.ts` | `utils::zip` | 72 |
