//! Port of `packages/coding-agent/src/core/settings-defaults.ts`.

use serde_json::Value;
use std::sync::LazyLock;

/// Runtime defaults shared by settings accessors and the published JSON Schema.
pub static SETTINGS_DEFAULTS: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/settings-defaults.ts:SETTINGS_DEFAULTS"));
