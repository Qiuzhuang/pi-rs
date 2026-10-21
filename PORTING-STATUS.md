# Porting status

Tracks what is ported and what is outstanding. Update when finishing a module.

| Area | Status |
|------|--------|
| workspace skeleton | done |
| pi-ai: core types (`types.ts`, `model-schema.ts`, `compat-schema.ts`), event stream, identity, diagnostics | done |
| pi-tui: core utils (width/wrap/truncate/slice), colors, oklab, terminal-colors, keys, stdin-buffer, keybindings, kill-ring, undo-stack, word-navigation, fuzzy | done |
| pi-telemetry: contracts, noop, in-memory recorder/store, metrics, schemas, typed starters, render, conformance suite (all TS tests ported) | done. Notes: TS compile-time schema typing (exact attribute types) has no Rust equivalent; `TypedSpanStarter`/`TypedTelemetryMetrics` bind schemas at runtime without validation, as in TS. Callbacks are futures: `start_span`/`try_start_span` (an `Err` sets error status); dropping an unfinished span future settles it with an `AbortError` status. Clocks are injectable (`ManualTelemetryClock`) instead of mocking `Date.now` |
| pi-mcp: jsonrpc/types/content, client (handshake, paginated tools/resources/templates, read, call, progress, timeouts, cancellation, server requests, list_changed), stdio (process groups, SIGTERM/SIGKILL shutdown, exit hook), streamable HTTP (JSON/SSE, sessions, GET stream, resumption, reconnect, auth retry), in-memory, auth provider, OAuth (discovery, PKCE, DCR, CIMD, refresh, step-up, provider/store, callback server); all TS tests ported | done. Notes: errors are one cloneable `pi_mcp::Error` enum (`name()` gives the TS class); `AbortSignal` wraps `CancellationToken` and keeps the reason; `McpFetch` is a closure over `HttpRequest`/`HttpResponse` (reqwest default); request methods dispatch when called and dropping the returned future cancels the request. Rust additions: `list_prompts`/`get_prompt`. Windows cross-spawn shim handling (`.cmd` via cmd.exe, PATHEXT) is implemented but untested here. Stdio test fixtures are POSIX sh scripts |
| pi-agent: types, agent-loop, agent, proxy, stream-fn; all TS tests ported (agent-loop, agent, proxy, e2e via faux provider) | done. Notes: custom messages are `AgentMessage::Custom` (raw JSON fields); in-place `args` mutation in `beforeToolCall` is `BeforeToolCallResult.args`; `Agent.continue()` is `continue_run()` |
