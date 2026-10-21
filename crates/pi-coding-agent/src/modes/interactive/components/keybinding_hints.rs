//! Port of `packages/coding-agent/src/modes/interactive/components/keybinding-hints.ts`.
//!
//! Utilities for formatting keybinding hints in the UI.

use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use pi_tui::keybindings::{Keybinding, get_keybindings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyTextFormatOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capitalize: Option<bool>,
}

fn format_key_part(part: &str, options: &KeyTextFormatOptions) -> String {
    let display_part = if cfg!(target_os = "macos") && part.to_lowercase() == "alt" { "option" } else { part };
    if options.capitalize.unwrap_or(false) {
        let mut chars = display_part.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        }
    } else {
        display_part.to_string()
    }
}

pub fn format_key_text(key: &str, options: Option<&KeyTextFormatOptions>) -> String {
    let default_options = KeyTextFormatOptions::default();
    let options = options.unwrap_or(&default_options);
    key.split('/')
        .map(|k| k.split('+').map(|part| format_key_part(part, options)).collect::<Vec<_>>().join("+"))
        .collect::<Vec<_>>()
        .join("/")
}

fn format_keys(keys: &[String], options: Option<&KeyTextFormatOptions>) -> String {
    if keys.is_empty() {
        return String::new();
    }
    format_key_text(&keys.join("/"), options)
}

pub fn key_text(keybinding: Keybinding) -> String {
    format_keys(&get_keybindings().get_keys(&keybinding), None)
}

pub fn key_display_text(keybinding: Keybinding) -> String {
    format_keys(&get_keybindings().get_keys(&keybinding), Some(&KeyTextFormatOptions { capitalize: Some(true) }))
}

pub fn key_hint(keybinding: Keybinding, description: &str) -> String {
    let t = theme();
    t.fg(&ThemeColor::Dim, &key_text(keybinding)) + &t.fg(&ThemeColor::Muted, &format!(" {description}"))
}

pub fn raw_key_hint(key: &str, description: &str) -> String {
    let t = theme();
    t.fg(&ThemeColor::Dim, &format_key_text(key, None)) + &t.fg(&ThemeColor::Muted, &format!(" {description}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_key_text_parts() {
        assert_eq!(format_key_text("ctrl+x/escape", None), "ctrl+x/escape");
        assert_eq!(
            format_key_text("ctrl+x/escape", Some(&KeyTextFormatOptions { capitalize: Some(true) })),
            "Ctrl+X/Escape"
        );
    }
}
