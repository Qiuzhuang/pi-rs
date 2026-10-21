//! Port of `packages/coding-agent/src/core/keybindings.ts`.

use crate::config::get_agent_dir;
use crate::core::key_id_schema::is_valid_keybinding_value;
use crate::utils::paths::node_join;
use crate::utils::text::strip_bom;
use indexmap::IndexMap;
use pi_tui::keybindings::{
    KeySpec, KeybindingDefinition, KeybindingDefinitions, KeybindingsConfig,
    KeybindingsManager as TuiKeybindingsManager, TUI_KEYBINDINGS,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, LazyLock};

/// `process.platform` of the running binary.
pub fn node_platform() -> &'static str {
    if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "freebsd") {
        "freebsd"
    } else {
        std::env::consts::OS
    }
}

/// Whether Windows-friendly default keys apply: native Windows, or Linux under WSL.
/// `platform` defaults to the current platform and `env` to the process environment.
pub fn use_windows_keybindings(
    platform: Option<&str>,
    env: Option<&HashMap<String, String>>,
) -> bool {
    let platform: &str = match platform {
        Some(p) => p,
        None => node_platform(),
    };
    let has = |name: &str| match env {
        Some(env) => env.get(name).is_some_and(|v| !v.is_empty()),
        None => std::env::var(name).is_ok_and(|v| !v.is_empty()),
    };
    platform == "win32" || (platform == "linux" && (has("WSL_DISTRO_NAME") || has("WSL_INTEROP")))
}

pub type AppKeybinding = String;

pub type AppKeybindings = Value;

fn def(keys: KeySpec, description: &str) -> KeybindingDefinition {
    KeybindingDefinition {
        default_keys: keys,
        description: Some(description.to_string()),
    }
}

fn one(key: &str) -> KeySpec {
    KeySpec::One(key.to_string())
}

fn many(keys: &[&str]) -> KeySpec {
    KeySpec::Many(keys.iter().map(|k| k.to_string()).collect())
}

fn app_keybindings(windows: bool) -> KeybindingDefinitions {
    let win32 = cfg!(windows);
    let darwin = cfg!(target_os = "macos");
    let entries: Vec<(&str, KeySpec, &str)> = vec![
        ("app.interrupt", one("escape"), "Cancel or abort"),
        ("app.clear", one("ctrl+c"), "Clear editor"),
        ("app.exit", one("ctrl+d"), "Exit when editor is empty"),
        (
            "app.suspend",
            if win32 { many(&[]) } else { one("ctrl+z") },
            "Suspend to background",
        ),
        ("app.thinking.cycle", one("shift+tab"), "Cycle thinking level"),
        ("app.thinking.save", one("ctrl+s"), "Save thinking level"),
        ("app.model.cycleForward", one("ctrl+p"), "Cycle to next model"),
        (
            "app.model.cycleBackward",
            one(if windows { "alt+p" } else { "shift+ctrl+p" }),
            "Cycle to previous model",
        ),
        ("app.model.select", one("ctrl+l"), "Open model selector"),
        ("app.tools.expand", one("ctrl+o"), "Toggle tool output"),
        ("app.thinking.toggle", one("ctrl+t"), "Toggle thinking blocks"),
        (
            "app.session.toggleNamedFilter",
            one("ctrl+n"),
            "Toggle named session filter",
        ),
        ("app.editor.external", one("ctrl+g"), "Open external editor"),
        (
            "app.message.copy",
            one("ctrl+x"),
            "Copy selection or last assistant message",
        ),
        (
            "app.message.followUp",
            one(if windows { "ctrl+q" } else { "alt+enter" }),
            "Queue follow-up message",
        ),
        (
            "app.message.dequeue",
            one(if windows { "alt+q" } else { "alt+up" }),
            "Restore queued messages",
        ),
        (
            "app.clipboard.pasteImage",
            one(if windows { "alt+v" } else { "ctrl+v" }),
            "Paste files on macOS, images, or text from clipboard",
        ),
        ("app.session.new", many(&[]), "Start a new session"),
        ("app.session.tree", many(&[]), "Open session tree"),
        ("app.session.fork", many(&[]), "Fork current session"),
        ("app.session.resume", many(&[]), "Resume a session"),
        (
            "app.tree.foldOrUp",
            if darwin {
                many(&["alt+left", "ctrl+left"])
            } else {
                many(&["ctrl+left", "alt+left"])
            },
            "Fold tree branch or move up",
        ),
        (
            "app.tree.unfoldOrDown",
            if darwin {
                many(&["alt+right", "ctrl+right"])
            } else {
                many(&["ctrl+right", "alt+right"])
            },
            "Unfold tree branch or move down",
        ),
        ("app.tree.editLabel", one("shift+l"), "Edit tree label"),
        (
            "app.tree.toggleLabelTimestamp",
            one("shift+t"),
            "Toggle tree label timestamps",
        ),
        (
            "app.session.togglePath",
            one("ctrl+p"),
            "Toggle session path display",
        ),
        ("app.session.toggleSort", one("ctrl+s"), "Toggle session sort mode"),
        ("app.session.rename", one("ctrl+r"), "Rename session"),
        ("app.session.delete", one("ctrl+d"), "Delete session"),
        (
            "app.session.deleteNoninvasive",
            one("ctrl+backspace"),
            "Delete session when query is empty",
        ),
        ("app.models.save", one("ctrl+s"), "Save model selection"),
        ("app.models.enableAll", one("ctrl+a"), "Enable all models"),
        ("app.models.clearAll", one("ctrl+x"), "Clear all models"),
        (
            "app.models.toggleProvider",
            one("ctrl+p"),
            "Toggle all models for provider",
        ),
        ("app.models.reorderUp", one("alt+up"), "Move model up in order"),
        ("app.models.reorderDown", one("alt+down"), "Move model down in order"),
        ("app.tree.filter.default", one("ctrl+d"), "Tree filter: default view"),
        (
            "app.tree.filter.noTools",
            one("ctrl+t"),
            "Tree filter: hide tool results",
        ),
        (
            "app.tree.filter.userOnly",
            one("ctrl+u"),
            "Tree filter: user messages only",
        ),
        (
            "app.tree.filter.labeledOnly",
            one("ctrl+l"),
            "Tree filter: labeled entries only",
        ),
        ("app.tree.filter.all", one("ctrl+a"), "Tree filter: show all entries"),
        (
            "app.tree.filter.cycleForward",
            one("ctrl+o"),
            "Tree filter: cycle forward",
        ),
        (
            "app.tree.filter.cycleBackward",
            one("shift+ctrl+o"),
            "Tree filter: cycle backward",
        ),
    ];
    entries
        .into_iter()
        .map(|(id, keys, description)| (id.to_string(), def(keys, description)))
        .collect()
}

/// Build the keybinding table for a Windows-keybindings decision (see [`use_windows_keybindings`]).
pub fn build_keybindings(windows: bool) -> KeybindingDefinitions {
    let win32 = cfg!(windows);
    let mut definitions: KeybindingDefinitions = TUI_KEYBINDINGS.clone();
    let mut set_default = |id: &str, keys: KeySpec| {
        if let Some((_, definition)) = definitions.iter_mut().find(|(k, _)| k == id) {
            definition.default_keys = keys;
        }
    };
    set_default(
        "tui.editor.undo",
        one(if win32 {
            "ctrl+z"
        } else if windows {
            "alt+z"
        } else {
            "ctrl+-"
        }),
    );
    set_default(
        "tui.altScreen.previousPrompt",
        if windows {
            one("ctrl+up")
        } else {
            many(&["ctrl+shift+up", "ctrl+up"])
        },
    );
    set_default(
        "tui.altScreen.nextPrompt",
        if windows {
            one("ctrl+down")
        } else {
            many(&["ctrl+shift+down", "ctrl+down"])
        },
    );
    set_default(
        "tui.altScreen.search",
        one(if windows { "ctrl+f" } else { "ctrl+shift+f" }),
    );
    for (id, definition) in app_keybindings(windows) {
        match definitions.iter_mut().find(|(k, _)| *k == id) {
            Some((_, existing)) => *existing = definition,
            None => definitions.push((id, definition)),
        }
    }
    definitions
}

/// All keybindings: the pi-tui table with app overrides, then the app keybindings.
pub static KEYBINDINGS: LazyLock<KeybindingDefinitions> =
    LazyLock::new(|| build_keybindings(use_windows_keybindings(None, None)));

/// Look up a keybinding definition by id.
pub fn get_keybinding_definition(id: &str) -> Option<&'static KeybindingDefinition> {
    KEYBINDINGS.iter().find(|(k, _)| k == id).map(|(_, d)| d)
}

const KEYBINDING_NAME_MIGRATIONS: &[(&str, &str)] = &[
    ("cursorUp", "tui.editor.cursorUp"),
    ("cursorDown", "tui.editor.cursorDown"),
    ("cursorLeft", "tui.editor.cursorLeft"),
    ("cursorRight", "tui.editor.cursorRight"),
    ("cursorWordLeft", "tui.editor.cursorWordLeft"),
    ("cursorWordRight", "tui.editor.cursorWordRight"),
    ("cursorLineStart", "tui.editor.cursorLineStart"),
    ("cursorLineEnd", "tui.editor.cursorLineEnd"),
    ("jumpForward", "tui.editor.jumpForward"),
    ("jumpBackward", "tui.editor.jumpBackward"),
    ("pageUp", "tui.editor.pageUp"),
    ("pageDown", "tui.editor.pageDown"),
    ("deleteCharBackward", "tui.editor.deleteCharBackward"),
    ("deleteCharForward", "tui.editor.deleteCharForward"),
    ("deleteWordBackward", "tui.editor.deleteWordBackward"),
    ("deleteWordForward", "tui.editor.deleteWordForward"),
    ("deleteToLineStart", "tui.editor.deleteToLineStart"),
    ("deleteToLineEnd", "tui.editor.deleteToLineEnd"),
    ("yank", "tui.editor.yank"),
    ("yankPop", "tui.editor.yankPop"),
    ("undo", "tui.editor.undo"),
    ("newLine", "tui.input.newLine"),
    ("submit", "tui.input.submit"),
    ("tab", "tui.input.tab"),
    ("copy", "tui.input.copy"),
    ("selectUp", "tui.select.up"),
    ("selectDown", "tui.select.down"),
    ("selectPageUp", "tui.select.pageUp"),
    ("selectPageDown", "tui.select.pageDown"),
    ("selectConfirm", "tui.select.confirm"),
    ("selectCancel", "tui.select.cancel"),
    ("interrupt", "app.interrupt"),
    ("clear", "app.clear"),
    ("exit", "app.exit"),
    ("suspend", "app.suspend"),
    ("cycleThinkingLevel", "app.thinking.cycle"),
    ("cycleModelForward", "app.model.cycleForward"),
    ("cycleModelBackward", "app.model.cycleBackward"),
    ("selectModel", "app.model.select"),
    ("expandTools", "app.tools.expand"),
    ("toggleThinking", "app.thinking.toggle"),
    ("toggleSessionNamedFilter", "app.session.toggleNamedFilter"),
    ("externalEditor", "app.editor.external"),
    ("followUp", "app.message.followUp"),
    ("dequeue", "app.message.dequeue"),
    ("pasteImage", "app.clipboard.pasteImage"),
    ("newSession", "app.session.new"),
    ("tree", "app.session.tree"),
    ("fork", "app.session.fork"),
    ("resume", "app.session.resume"),
    ("treeFoldOrUp", "app.tree.foldOrUp"),
    ("treeUnfoldOrDown", "app.tree.unfoldOrDown"),
    ("treeEditLabel", "app.tree.editLabel"),
    ("treeToggleLabelTimestamp", "app.tree.toggleLabelTimestamp"),
    ("toggleSessionPath", "app.session.togglePath"),
    ("toggleSessionSort", "app.session.toggleSort"),
    ("renameSession", "app.session.rename"),
    ("deleteSession", "app.session.delete"),
    ("deleteSessionNoninvasive", "app.session.deleteNoninvasive"),
];

fn migrated_keybinding_name(key: &str) -> Option<&'static str> {
    KEYBINDING_NAME_MIGRATIONS
        .iter()
        .find(|(old, _)| *old == key)
        .map(|(_, new)| *new)
}

fn key_spec_from_value(value: &Value) -> Option<KeySpec> {
    match value {
        Value::String(s) => Some(KeySpec::One(s.clone())),
        Value::Array(items) => Some(KeySpec::Many(
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
        )),
        _ => None,
    }
}

fn key_spec_to_value(spec: &KeySpec) -> Value {
    match spec {
        KeySpec::One(key) => Value::String(key.clone()),
        KeySpec::Many(keys) => Value::Array(keys.iter().cloned().map(Value::String).collect()),
    }
}

fn to_keybindings_config(value: &IndexMap<String, Value>) -> KeybindingsConfig {
    value
        .iter()
        .filter(|(key, binding)| key.as_str() != "$schema" && is_valid_keybinding_value(binding))
        .filter_map(|(key, binding)| key_spec_from_value(binding).map(|spec| (key.clone(), Some(spec))))
        .collect()
}

/// Convert a keybindings config to its JSON object form (`undefined` entries are omitted).
pub fn keybindings_config_to_json(config: &KeybindingsConfig) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    for (key, spec) in config {
        if let Some(spec) = spec {
            map.insert(key.clone(), key_spec_to_value(spec));
        }
    }
    map
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateKeybindingsConfigResult {
    pub config: IndexMap<String, Value>,
    pub migrated: bool,
}

pub fn migrate_keybindings_config(
    raw_config: IndexMap<String, Value>,
) -> MigrateKeybindingsConfigResult {
    let mut config: IndexMap<String, Value> = IndexMap::new();
    let mut migrated = false;

    for (key, value) in &raw_config {
        let next_key = migrated_keybinding_name(key).unwrap_or(key.as_str());
        if next_key != key {
            migrated = true;
        }
        if key != next_key && raw_config.contains_key(next_key) {
            migrated = true;
            continue;
        }
        config.insert(next_key.to_string(), value.clone());
    }

    MigrateKeybindingsConfigResult {
        config: order_keybindings_config(config),
        migrated,
    }
}

fn order_keybindings_config(config: IndexMap<String, Value>) -> IndexMap<String, Value> {
    let mut ordered: IndexMap<String, Value> = IndexMap::new();
    for (keybinding, _) in KEYBINDINGS.iter() {
        if let Some(value) = config.get(keybinding) {
            ordered.insert(keybinding.clone(), value.clone());
        }
    }
    let mut extras: Vec<&String> = config
        .keys()
        .filter(|key| !ordered.contains_key(*key))
        .collect();
    extras.sort_by(|a, b| crate::core::trust_manager::js_compare(a, b));
    for key in extras {
        ordered.insert(key.clone(), config[key].clone());
    }
    ordered
}

fn load_raw_config(path: &str) -> Option<IndexMap<String, Value>> {
    let content = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Value>(&strip_bom(&content)).ok()? {
        Value::Object(map) => Some(map.into_iter().collect()),
        // Arrays are objects in JS; their entries are index-keyed.
        Value::Array(items) => Some(
            items
                .into_iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v))
                .collect(),
        ),
        _ => None,
    }
}

/// TS: `extends TuiKeybindingsManager`. Dereferences to the pi-tui manager; [`Self::tui_manager`]
/// gives the shared handle to install with `pi_tui::keybindings::set_keybindings`.
pub struct KeybindingsManager {
    inner: Arc<TuiKeybindingsManager>,
    config_path: Option<String>,
}

impl KeybindingsManager {
    pub fn new(user_bindings: Option<KeybindingsConfig>, config_path: Option<&str>) -> Self {
        Self {
            inner: Arc::new(TuiKeybindingsManager::new(
                KEYBINDINGS.clone(),
                user_bindings.unwrap_or_default(),
            )),
            config_path: config_path.map(str::to_string),
        }
    }

    pub fn create(agent_dir: Option<&str>) -> Arc<KeybindingsManager> {
        let agent_dir = agent_dir.map(str::to_string).unwrap_or_else(get_agent_dir);
        let config_path = node_join(&[&agent_dir, "keybindings.json"]);
        let user_bindings = Self::load_from_file(&config_path);
        Arc::new(Self::new(Some(user_bindings), Some(&config_path)))
    }

    pub fn reload(&self) {
        let Some(config_path) = &self.config_path else {
            return;
        };
        self.inner
            .set_user_bindings(Self::load_from_file(config_path));
    }

    pub fn get_effective_config(&self) -> KeybindingsConfig {
        self.inner
            .get_resolved_bindings()
            .into_iter()
            .map(|(id, spec)| (id, Some(spec)))
            .collect()
    }

    /// The underlying pi-tui keybindings manager.
    pub fn tui_manager(&self) -> Arc<TuiKeybindingsManager> {
        self.inner.clone()
    }

    fn load_from_file(path: &str) -> KeybindingsConfig {
        let Some(raw_config) = load_raw_config(path) else {
            return Vec::new();
        };
        to_keybindings_config(&migrate_keybindings_config(raw_config).config)
    }
}

impl Deref for KeybindingsManager {
    type Target = TuiKeybindingsManager;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn default_keys(id: &str) -> KeySpec {
        get_keybinding_definition(id).unwrap().default_keys.clone()
    }

    #[test]
    fn windows_keybinding_detection() {
        assert!(use_windows_keybindings(Some("win32"), Some(&env(&[]))));
        assert!(use_windows_keybindings(
            Some("linux"),
            Some(&env(&[("WSL_DISTRO_NAME", "Ubuntu")]))
        ));
        assert!(use_windows_keybindings(
            Some("linux"),
            Some(&env(&[("WSL_INTEROP", "/run/WSL/123_interop")]))
        ));
        assert!(!use_windows_keybindings(
            Some("linux"),
            Some(&env(&[("WT_SESSION", "session")]))
        ));
        assert!(!use_windows_keybindings(Some("linux"), Some(&env(&[]))));
        assert!(!use_windows_keybindings(Some("darwin"), Some(&env(&[]))));
    }

    #[test]
    fn applies_the_detected_defaults_consistently() {
        let windows = use_windows_keybindings(None, None);
        let native = cfg!(windows);
        assert_eq!(
            default_keys("app.clipboard.pasteImage"),
            one(if windows { "alt+v" } else { "ctrl+v" })
        );
        assert_eq!(
            default_keys("tui.altScreen.search"),
            one(if windows { "ctrl+f" } else { "ctrl+shift+f" })
        );
        assert_eq!(
            default_keys("app.message.followUp"),
            one(if windows { "ctrl+q" } else { "alt+enter" })
        );
        assert_eq!(
            default_keys("app.model.cycleBackward"),
            one(if windows { "alt+p" } else { "shift+ctrl+p" })
        );
        assert_eq!(
            default_keys("tui.editor.undo"),
            one(if native {
                "ctrl+z"
            } else if windows {
                "alt+z"
            } else {
                "ctrl+-"
            })
        );
        assert_eq!(
            default_keys("tui.altScreen.previousPrompt"),
            if windows {
                one("ctrl+up")
            } else {
                many(&["ctrl+shift+up", "ctrl+up"])
            }
        );
        assert_eq!(
            default_keys("app.message.dequeue"),
            one(if windows { "alt+q" } else { "alt+up" })
        );
        // Overridden TUI ids keep their position; app ids follow the TUI ids.
        let ids: Vec<&str> = KEYBINDINGS.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(ids[0], "tui.editor.cursorUp");
        assert!(
            ids.iter().position(|k| *k == "tui.editor.undo").unwrap()
                < ids.iter().position(|k| *k == "app.interrupt").unwrap()
        );
    }

    #[test]
    fn migrates_old_key_names() {
        let raw: IndexMap<String, Value> = [
            ("cursorUp".to_string(), json!(["up", "ctrl+p"])),
            ("expandTools".to_string(), json!("ctrl+x")),
        ]
        .into();
        let result = migrate_keybindings_config(raw);
        assert!(result.migrated);
        assert_eq!(
            Value::Object(result.config.into_iter().collect()),
            json!({"tui.editor.cursorUp": ["up", "ctrl+p"], "app.tools.expand": "ctrl+x"})
        );

        let raw: IndexMap<String, Value> = [
            ("expandTools".to_string(), json!("ctrl+x")),
            ("app.tools.expand".to_string(), json!("ctrl+y")),
        ]
        .into();
        let result = migrate_keybindings_config(raw);
        assert_eq!(
            Value::Object(result.config.into_iter().collect()),
            json!({"app.tools.expand": "ctrl+y"})
        );

        let raw: IndexMap<String, Value> = [
            ("z.custom".to_string(), json!("ctrl+1")),
            ("a.custom".to_string(), json!("ctrl+2")),
            ("app.interrupt".to_string(), json!("escape")),
        ]
        .into();
        let result = migrate_keybindings_config(raw);
        assert!(!result.migrated);
        assert_eq!(
            result.config.keys().collect::<Vec<_>>(),
            vec!["app.interrupt", "a.custom", "z.custom"]
        );
    }

    #[test]
    fn loads_old_key_names_in_memory_and_ignores_schema_and_invalid_values() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("keybindings.json"),
            serde_json::to_string_pretty(&json!({
                "$schema": "https://pi.dev/schemas/keybindings.schema.json",
                "selectConfirm": "enter",
                "interrupt": "ctrl+x",
                "extension.invalid": "ctrl+not-a-key"
            }))
            .unwrap(),
        )
        .unwrap();
        let keybindings = KeybindingsManager::create(Some(temp.path().to_str().unwrap()));
        assert_eq!(
            keybindings_config_to_json(&keybindings.get_user_bindings()),
            *json!({"tui.select.confirm": "enter", "app.interrupt": "ctrl+x"})
                .as_object()
                .unwrap()
        );
        let effective = keybindings_config_to_json(&keybindings.get_effective_config());
        assert_eq!(effective["tui.select.confirm"], "enter");
        assert_eq!(effective["app.interrupt"], "ctrl+x");
        assert_eq!(keybindings.get_keys("app.interrupt"), vec!["ctrl+x"]);

        std::fs::write(
            temp.path().join("keybindings.json"),
            "{\"app.interrupt\": \"ctrl+q\"}",
        )
        .unwrap();
        keybindings.reload();
        assert_eq!(keybindings.get_keys("app.interrupt"), vec!["ctrl+q"]);
    }
}
