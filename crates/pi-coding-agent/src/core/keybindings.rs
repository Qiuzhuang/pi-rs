//! Port of `packages/coding-agent/src/core/keybindings.ts`.

use indexmap::IndexMap;
use pi_tui::keybindings::KeybindingsConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

pub fn use_windows_keybindings(platform: Option<Value>, env: Option<Value>) -> bool {
    todo!("port core/keybindings.ts:useWindowsKeybindings")
}

pub type AppKeybinding = String;

pub type AppKeybindings = Value;

pub static KEYBINDINGS: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/keybindings.ts:KEYBINDINGS"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateKeybindingsConfigMigrateKeybindingsConfigResult {
    pub config: IndexMap<String, Value>,
    pub migrated: bool,
}

pub fn migrate_keybindings_config(
    raw_config: IndexMap<String, Value>,
) -> MigrateKeybindingsConfigMigrateKeybindingsConfigResult {
    todo!("port core/keybindings.ts:migrateKeybindingsConfig")
}

/// TS: `extends TuiKeybindingsManager`
pub struct KeybindingsManager {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl KeybindingsManager {
    pub fn new(user_bindings: Option<KeybindingsConfig>, config_path: Option<&str>) -> Self {
        todo!("port core/keybindings.ts:KeybindingsManager.constructor")
    }

    pub fn create(agent_dir: Option<&str>) -> Arc<KeybindingsManager> {
        todo!("port core/keybindings.ts:KeybindingsManager.create")
    }

    pub fn reload(&self) {
        todo!("port core/keybindings.ts:KeybindingsManager.reload")
    }

    pub fn get_effective_config(&self) -> KeybindingsConfig {
        todo!("port core/keybindings.ts:KeybindingsManager.getEffectiveConfig")
    }
}

// TODO(port): impl pi-tui::KeybindingsManager for KeybindingsManager
