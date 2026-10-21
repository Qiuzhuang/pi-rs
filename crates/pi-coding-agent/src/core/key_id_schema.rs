//! Port of `packages/coding-agent/src/core/key-id-schema.ts`.

use serde_json::Value;
use std::sync::LazyLock;

pub static KEY_ID_SCHEMA: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/key-id-schema.ts:KeyIdSchema"));

pub fn keybinding_value_schema(options: Option<Value>) -> Value {
    todo!("port core/key-id-schema.ts:keybindingValueSchema")
}

pub static KEYBINDING_VALUE_SCHEMA: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/key-id-schema.ts:KeybindingValueSchema"));
