//! Keybinding registry. Port of `packages/tui/src/keybindings.ts`.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use crate::keys::matches_key;

/// A keybinding identifier such as `"tui.editor.cursorUp"`.
pub type Keybinding = String;

/// One or several keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySpec {
    One(String),
    Many(Vec<String>),
}

impl KeySpec {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            KeySpec::One(k) => vec![k.clone()],
            KeySpec::Many(v) => v.clone(),
        }
    }
}

impl From<&str> for KeySpec {
    fn from(v: &str) -> Self {
        KeySpec::One(v.to_string())
    }
}
impl From<Vec<&str>> for KeySpec {
    fn from(v: Vec<&str>) -> Self {
        KeySpec::Many(v.into_iter().map(String::from).collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindingDefinition {
    pub default_keys: KeySpec,
    pub description: Option<String>,
}

/// Ordered keybinding definitions (insertion order matters, like `Object.entries`).
pub type KeybindingDefinitions = Vec<(String, KeybindingDefinition)>;
/// Ordered user overrides; `None` means "undefined" (use defaults).
pub type KeybindingsConfig = Vec<(String, Option<KeySpec>)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindingConflict {
    pub key: String,
    pub keybindings: Vec<String>,
}

macro_rules! defs {
    ($( $id:literal => [$($k:literal),*] , $desc:literal );* $(;)?) => {
        vec![$( ($id.to_string(), KeybindingDefinition {
            default_keys: KeySpec::Many(vec![$($k.to_string()),*]),
            description: Some($desc.to_string()),
        }) ),*]
    };
}

fn single(spec: KeybindingDefinitions) -> KeybindingDefinitions {
    // Definitions declared with exactly one key use the single-key form, like the TS table.
    spec.into_iter()
        .map(|(id, mut d)| {
            if let KeySpec::Many(v) = &d.default_keys
                && v.len() == 1
            {
                d.default_keys = KeySpec::One(v[0].clone());
            }
            (id, d)
        })
        .collect()
}

/// The built-in TUI keybindings.
pub static TUI_KEYBINDINGS: LazyLock<KeybindingDefinitions> = LazyLock::new(|| {
    single(defs! {
        "tui.editor.cursorUp" => ["up"], "Move cursor up";
        "tui.editor.cursorDown" => ["down"], "Move cursor down";
        "tui.editor.historyPrevious" => [], "Select previous prompt history entry";
        "tui.editor.historyNext" => [], "Select next prompt history entry";
        "tui.editor.cursorLeft" => ["left", "ctrl+b"], "Move cursor left";
        "tui.editor.cursorRight" => ["right", "ctrl+f"], "Move cursor right";
        "tui.editor.cursorWordLeft" => ["alt+left", "ctrl+left", "alt+b"], "Move cursor word left";
        "tui.editor.cursorWordRight" => ["alt+right", "ctrl+right", "alt+f"], "Move cursor word right";
        "tui.editor.cursorLineStart" => ["home", "ctrl+a"], "Move to line start";
        "tui.editor.cursorLineEnd" => ["end", "ctrl+e"], "Move to line end";
        "tui.editor.jumpForward" => ["ctrl+]"], "Jump forward to character";
        "tui.editor.jumpBackward" => ["ctrl+alt+]"], "Jump backward to character";
        "tui.editor.pageUp" => ["pageUp", "ctrl+pageUp"], "Page up";
        "tui.editor.pageDown" => ["pageDown", "ctrl+pageDown"], "Page down";
        "tui.editor.deleteCharBackward" => ["backspace"], "Delete character backward";
        "tui.editor.deleteCharForward" => ["delete", "ctrl+d"], "Delete character forward";
        "tui.editor.deleteWordBackward" => ["ctrl+w", "alt+backspace"], "Delete word backward";
        "tui.editor.deleteWordForward" => ["alt+d", "alt+delete"], "Delete word forward";
        "tui.editor.deleteToLineStart" => ["ctrl+u"], "Delete to line start";
        "tui.editor.deleteToLineEnd" => ["ctrl+k"], "Delete to line end";
        "tui.editor.yank" => ["ctrl+y"], "Yank";
        "tui.editor.yankPop" => ["alt+y"], "Yank pop";
        "tui.editor.undo" => ["ctrl+-"], "Undo";
        "tui.input.newLine" => ["shift+enter", "ctrl+j"], "Insert newline";
        "tui.input.submit" => ["enter"], "Submit input";
        "tui.input.tab" => ["tab"], "Tab / autocomplete";
        "tui.input.copy" => ["ctrl+c"], "Copy selection";
        "tui.select.up" => ["up"], "Move selection up";
        "tui.select.down" => ["down"], "Move selection down";
        "tui.select.pageUp" => ["pageUp"], "Selection page up";
        "tui.select.pageDown" => ["pageDown"], "Selection page down";
        "tui.select.confirm" => ["enter"], "Confirm selection";
        "tui.select.cancel" => ["escape", "ctrl+c"], "Cancel selection";
        "tui.altScreen.pageUp" => ["pageUp"], "Scroll viewport up one page";
        "tui.altScreen.pageDown" => ["pageDown"], "Scroll viewport down one page";
        "tui.altScreen.halfPageUp" => [], "Scroll viewport up half a page";
        "tui.altScreen.halfPageDown" => [], "Scroll viewport down half a page";
        "tui.altScreen.lineUp" => [], "Scroll viewport up one line";
        "tui.altScreen.lineDown" => [], "Scroll viewport down one line";
        "tui.altScreen.previousPrompt" => ["ctrl+shift+up", "ctrl+up"], "Jump to previous semantic prompt";
        "tui.altScreen.nextPrompt" => ["ctrl+shift+down", "ctrl+down"], "Jump to next semantic prompt";
        "tui.altScreen.search" => ["ctrl+shift+f"], "Search the primary scroll view";
        "tui.altScreen.searchNext" => ["enter", "ctrl+g"], "Select the next search match";
        "tui.altScreen.searchPrevious" => ["shift+enter", "ctrl+shift+g"], "Select the previous search match";
        "tui.altScreen.searchClose" => ["escape"], "Close transcript search";
        "tui.altScreen.top" => ["ctrl+home"], "Scroll viewport to top";
        "tui.altScreen.bottom" => ["ctrl+end"], "Scroll viewport to bottom";
    })
});

fn normalize_keys(keys: Option<&KeySpec>) -> Vec<String> {
    let Some(keys) = keys else { return Vec::new() };
    let mut result: Vec<String> = Vec::new();
    for key in keys.to_vec() {
        if !result.contains(&key) {
            result.push(key);
        }
    }
    result
}

#[derive(Debug, Default)]
struct State {
    definitions: KeybindingDefinitions,
    user_bindings: KeybindingsConfig,
    keys_by_id: HashMap<String, Vec<String>>,
    conflicts: Vec<KeybindingConflict>,
}

impl State {
    fn rebuild(&mut self) {
        self.keys_by_id.clear();
        self.conflicts.clear();
        let mut claims: Vec<(String, Vec<String>)> = Vec::new();
        for (kb, keys) in &self.user_bindings {
            if !self.definitions.iter().any(|(id, _)| id == kb) {
                continue;
            }
            for key in normalize_keys(keys.as_ref()) {
                match claims.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, set)) => {
                        if !set.contains(kb) {
                            set.push(kb.clone());
                        }
                    }
                    None => claims.push((key, vec![kb.clone()])),
                }
            }
        }
        for (key, kbs) in claims {
            if kbs.len() > 1 {
                self.conflicts.push(KeybindingConflict { key, keybindings: kbs });
            }
        }
        for (id, def) in &self.definitions {
            // Last occurrence wins, like object property assignment.
            let user = self.user_bindings.iter().rev().find(|(k, _)| k == id).and_then(|(_, v)| v.as_ref());
            let keys = match user {
                None => normalize_keys(Some(&def.default_keys)),
                Some(u) => normalize_keys(Some(u)),
            };
            self.keys_by_id.insert(id.clone(), keys);
        }
    }
}

/// Resolves keybinding identifiers to keys, applying user overrides.
#[derive(Debug, Default)]
pub struct KeybindingsManager {
    state: RwLock<State>,
}

impl KeybindingsManager {
    pub fn new(definitions: KeybindingDefinitions, user_bindings: KeybindingsConfig) -> Self {
        let mut state = State { definitions, user_bindings, ..Default::default() };
        state.rebuild();
        Self { state: RwLock::new(state) }
    }

    /// Check whether input data matches any key bound to `keybinding`.
    pub fn matches(&self, data: &str, keybinding: &str) -> bool {
        let state = self.state.read().unwrap();
        state.keys_by_id.get(keybinding).is_some_and(|keys| keys.iter().any(|k| matches_key(data, k)))
    }

    pub fn get_keys(&self, keybinding: &str) -> Vec<String> {
        self.state.read().unwrap().keys_by_id.get(keybinding).cloned().unwrap_or_default()
    }

    pub fn get_definition(&self, keybinding: &str) -> Option<KeybindingDefinition> {
        self.state.read().unwrap().definitions.iter().find(|(id, _)| id == keybinding).map(|(_, d)| d.clone())
    }

    pub fn get_definitions(&self) -> KeybindingDefinitions {
        self.state.read().unwrap().definitions.clone()
    }

    pub fn get_conflicts(&self) -> Vec<KeybindingConflict> {
        self.state.read().unwrap().conflicts.clone()
    }

    pub fn set_user_bindings(&self, user_bindings: KeybindingsConfig) {
        let mut state = self.state.write().unwrap();
        state.user_bindings = user_bindings;
        state.rebuild();
    }

    pub fn get_user_bindings(&self) -> KeybindingsConfig {
        self.state.read().unwrap().user_bindings.clone()
    }

    /// Resolved bindings for every definition; single keys use [`KeySpec::One`].
    pub fn get_resolved_bindings(&self) -> Vec<(String, KeySpec)> {
        let state = self.state.read().unwrap();
        state
            .definitions
            .iter()
            .map(|(id, _)| {
                let keys = state.keys_by_id.get(id).cloned().unwrap_or_default();
                let spec = if keys.len() == 1 { KeySpec::One(keys[0].clone()) } else { KeySpec::Many(keys) };
                (id.clone(), spec)
            })
            .collect()
    }
}

static GLOBAL_KEYBINDINGS: RwLock<Option<Arc<KeybindingsManager>>> = RwLock::new(None);

/// Replace the global keybindings manager.
pub fn set_keybindings(keybindings: Arc<KeybindingsManager>) {
    *GLOBAL_KEYBINDINGS.write().unwrap() = Some(keybindings);
}

/// The global keybindings manager (created with [`TUI_KEYBINDINGS`] on first use).
pub fn get_keybindings() -> Arc<KeybindingsManager> {
    if let Some(k) = GLOBAL_KEYBINDINGS.read().unwrap().as_ref() {
        return k.clone();
    }
    let mut guard = GLOBAL_KEYBINDINGS.write().unwrap();
    guard.get_or_insert_with(|| Arc::new(KeybindingsManager::new(TUI_KEYBINDINGS.clone(), Vec::new()))).clone()
}
