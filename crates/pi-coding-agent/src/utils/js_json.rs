//! JavaScript-compatible JSON helpers.
//!
//! `JSON.parse` with V8's error messages lives in the `pi-json` crate (re-exported by pi-ai as
//! `pi_ai::utils::js_json`) so pi-ai, pi-agent and pi-mcp share it, together with `JSON.stringify`
//! (JavaScript number formatting and property order); it is re-exported here for the callers in
//! this crate.

pub use pi_ai::utils::js_json::{js_json_parse, js_json_parse_as, js_json_stringify_pretty, v8_json_syntax_error};
