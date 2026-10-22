# pi-rs

A Rust port of [pi](https://github.com/earendil-works/pi), the minimal, extensible coding agent
harness. It builds a single native `pi` binary with the same CLI, terminal UI, on-disk formats
(sessions, settings, `auth.json`, `models.json`), RPC protocol and extension API as the
TypeScript version. No Node.js or Bun is needed at runtime: TypeScript extensions run on an
embedded JavaScript engine.

The user documentation of pi applies unchanged; it ships with the crate in
[`crates/pi-coding-agent/README.md`](crates/pi-coding-agent/README.md) and
[`crates/pi-coding-agent/docs/`](crates/pi-coding-agent/docs/).

## Build and install

Requires Rust 1.88 or newer (edition 2024 with let chains; developed with 1.97).

```bash
cargo build --release          # builds target/release/pi
./target/release/pi --version
./target/release/pi --help

cargo install --path crates/pi-coding-agent   # installs `pi` into ~/.cargo/bin
```

Only the `pi` binary is built from `crates/pi-coding-agent`. Everything else is a library crate.
HTTP goes through `reqwest` with rustls, so there is no OpenSSL dependency.

## Workspace

| Crate | Ports | Contents |
|---|---|---|
| `crates/pi-ai` | `packages/ai` | Providers (Anthropic, OpenAI completions/responses/Codex, Azure, Google Gemini/Vertex, Bedrock, Mistral, Cloudflare, OpenRouter, ...), model catalog, streaming, OAuth logins, AWS/Google credential chains |
| `crates/pi-agent` | `packages/agent` | The agent loop, `Agent`, proxy stream |
| `crates/pi-tui` | `packages/tui` | Differential terminal renderer, components (editor, markdown, select lists, images, ...), keybindings |
| `crates/pi-mcp` | `packages/mcp` | Model Context Protocol client: stdio and streamable HTTP transports, OAuth |
| `crates/pi-codemode` | `packages/codemode` | Sandboxed JavaScript execution for code mode |
| `crates/pi-telemetry` | `packages/telemetry` | Telemetry contracts |
| `crates/pi-json` | (new) | `JSON.parse` with V8's error messages and `JSON.stringify` with JavaScript formatting, shared by the crates above |
| `crates/pi-jsrt` | (new) | Embedded JavaScript runtime for extensions: Boa engine, oxc TypeScript stripping, ES module loader, Node API shims (`fs`, `path`, `child_process`, `events`, streams, `fetch`, ...) |
| `crates/pi-coding-agent` | `packages/coding-agent` | The `pi` binary and library: sessions, tools, settings, resources, packages, interactive/print/JSON/RPC modes, built-in extensions |

Modules mirror the TypeScript file layout (`core/agent-session.ts` is
`pi_coding_agent::core::agent_session`). [`PORTING.md`](PORTING.md) describes the conventions and
[`crates/pi-coding-agent/ARCHITECTURE.md`](crates/pi-coding-agent/ARCHITECTURE.md) the design of
the main crate.

## Parity with TypeScript pi

The port aims at behavioral parity, including error messages, defaults, environment variables and
byte-compatible JSON output:

- **CLI and modes**: interactive TUI, print mode (`-p`), JSON event mode, RPC mode
  (`--mode rpc`), session management, `pi install/remove/update/list/config`, login/logout flows.
- **Formats**: session JSONL files, `settings.json`, `auth.json` and `models.json` are read and
  written exactly like the TypeScript version (key order, number formatting, migrations), so both
  implementations can share an agent directory (`~/.pi/agent`).
- **Rendering**: UI components are checked line by line against renders of the TypeScript
  reference (golden tests, see below).
- **Providers**: requests carry the same bodies and headers as the official SDKs the TypeScript
  version uses (OpenAI, Anthropic, Google GenAI, AWS SDK user agents and retry behavior, ...).

Known differences and the status of every area are tracked in
[`PORTING-STATUS.md`](PORTING-STATUS.md). The main ones:

- No Bun-specific plumbing, `rpc-entry.ts`, or Node module-resolution experiments (use
  `pi --mode rpc`).
- Rust strings cannot hold unpaired UTF-16 surrogates; where JavaScript would keep one (for
  example `JSON.parse("\"\\uD800\"")`), the port substitutes U+FFFD.
- Extension code runs on Boa rather than V8, so engine-level details (stack trace text,
  performance characteristics, a few rarely used language features) differ.

## Extensions

pi extensions are TypeScript or JavaScript modules, and the port loads them unchanged:

- `.ts`/`.js`/`.mjs` extensions run inside `pi-jsrt`, an embedded [Boa](https://boajs.dev) engine
  on a dedicated thread. TypeScript is stripped with [oxc](https://oxc.rs) when loaded.
- Imports of `@earendil-works/pi-coding-agent`, `@earendil-works/pi-tui`,
  `@earendil-works/pi-ai`, `@earendil-works/pi-agent-core`, `typebox` (the real library, vendored)
  and `node:*` built-ins resolve to modules provided by the runtime, backed by the Rust
  implementations. npm dependencies of extension packages are loaded from their `node_modules`.
- The full extension API is available: events, tools, commands, shortcuts, flags, providers, tool
  renderers, custom UI components, and session control.
- The built-in extensions (MCP, code mode, tool search, llama.cpp) are implemented in Rust against
  the same API.

[`crates/pi-coding-agent/js/EXPORTS.md`](crates/pi-coding-agent/js/EXPORTS.md) lists every export
of the extension-facing packages and how it is provided.

## Tests

```bash
cargo test --workspace                    # all tests
cargo test -p pi-coding-agent --test rpc_mode   # one test target
cargo clippy --workspace --all-targets
```

Most tests are ports of the TypeScript test suites (see
[`crates/pi-coding-agent/TEST-MAP.md`](crates/pi-coding-agent/TEST-MAP.md)) and run offline against
mock servers and the faux provider. A few suites use the TypeScript reference checkout when it is
available and skip otherwise:

- the example-extension tests (`extensions_examples`, `extensions_interactive`) load the examples
  from `PI_TS_SOURCE` (a checkout of the TypeScript repository);
- live provider tests in `crates/pi-ai/tests/ts_ported/live` need real API keys and are ignored by
  default.

### Golden fixtures

Rendering and configuration tests compare against fixtures generated from the TypeScript
reference (`crates/pi-coding-agent/tests/fixtures/golden/`). To regenerate them, install the
reference's dependencies (`npm ci --ignore-scripts` in the checkout; Node 22.6 or newer) and run:

```bash
PI_TS_REF=/path/to/pi scripts/golden/run.sh                 # all generators
PI_TS_REF=/path/to/pi scripts/golden/run.sh scripts/golden/gen-tui.mjs   # one generator
```

The generators render with a fixed home directory, clock, terminal and theme setup that the Rust
side reproduces. The vendored JavaScript libraries for extensions are regenerated with
`scripts/vendor-typebox.mjs` and `scripts/vendor-marked.mjs`.

## License

MIT, like pi.
