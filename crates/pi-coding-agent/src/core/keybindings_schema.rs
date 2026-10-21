//! Port of `packages/coding-agent/src/core/keybindings-schema.ts`.

use serde_json::Value;
use std::sync::LazyLock;

pub use crate::core::key_id_schema::KEYBINDING_VALUE_SCHEMA;

pub static KEYBINDINGS_SCHEMA: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/keybindings-schema.ts:KeybindingsSchema"));
