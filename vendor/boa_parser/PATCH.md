# Local patch to boa_parser 0.22.0

This is the crates.io release `boa_parser` 0.22.0 (Boa git `337a3668`, `core/parser`) with one
fix, wired into the workspace with `[patch.crates-io]` in the root `Cargo.toml`. Remove the
patch once an upstream release contains an equivalent fix.

## `export { ... }` as the last statement of a module

`src/parser/statement/declaration/export.rs` peeked the token after a `NamedExports` clause with
`.or_abrupt()` to check for `from`. When the clause is the last statement and has no semicolon
(automatic semicolon insertion at the end of input), there is no next token, and the module
failed to parse with `SyntaxError: abrupt end`. Real packages ship such files (for example
`fetch-blob/from.js`, a dependency of `node-fetch` 3).

The peek now accepts the end of input; `expect_semicolon` already allows ASI there. The changed
lines are marked `PATCH(pi-rs)`. Regression test:
`crates/pi-jsrt/tests/boa_patches.rs::export_list_at_end_of_module_without_semicolon`.
