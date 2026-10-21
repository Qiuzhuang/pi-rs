//! Port of `packages/coding-agent/src/modes/interactive/components/keybinding-hints.ts`.

use pi_tui::keybindings::Keybinding;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyTextFormatOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capitalize: Option<bool>,
}

pub fn format_key_text(key: &str, options: Option<&KeyTextFormatOptions>) -> String {
    todo!("port modes/interactive/components/keybinding-hints.ts:formatKeyText")
}

pub fn key_text(keybinding: Keybinding) -> String {
    todo!("port modes/interactive/components/keybinding-hints.ts:keyText")
}

pub fn key_display_text(keybinding: Keybinding) -> String {
    todo!("port modes/interactive/components/keybinding-hints.ts:keyDisplayText")
}

pub fn key_hint(keybinding: Keybinding, description: &str) -> String {
    todo!("port modes/interactive/components/keybinding-hints.ts:keyHint")
}

pub fn raw_key_hint(key: &str, description: &str) -> String {
    todo!("port modes/interactive/components/keybinding-hints.ts:rawKeyHint")
}
