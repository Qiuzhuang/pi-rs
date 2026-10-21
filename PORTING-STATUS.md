# Porting status

Tracks what is ported and what is outstanding. Update when finishing a module.

| Area | Status |
|------|--------|
| workspace skeleton | done |
| pi-ai: core types (`types.ts`, `model-schema.ts`, `compat-schema.ts`), event stream, identity, diagnostics | done |
| pi-tui: core utils (width/wrap/truncate/slice), colors, oklab, terminal-colors, keys, stdin-buffer, keybindings, kill-ring, undo-stack, word-navigation, fuzzy | done |
| pi-telemetry: contracts, noop, in-memory recorder/store, metrics, schemas, typed starters, render, conformance suite (all TS tests ported) | done |
| pi-mcp | in progress |
| pi-agent: types, agent-loop, agent, proxy, stream-fn; all TS tests ported (agent-loop, agent, proxy, e2e via faux provider) | done. Notes: custom messages are `AgentMessage::Custom` (raw JSON fields); in-place `args` mutation in `beforeToolCall` is `BeforeToolCallResult.args`; `Agent.continue()` is `continue_run()` |
