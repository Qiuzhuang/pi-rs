//! Builders for `sessions.json` (scripts/golden/gen-sessions.mjs).

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use pi_coding_agent::core::keybindings::KeybindingsManager;
use pi_coding_agent::core::session_manager::{SessionInfo, SessionTreeNode};
use pi_coding_agent::modes::interactive::components::session_selector::{
    SessionSelectorComponent, SessionSelectorComponentOptions, SessionsLoader,
};
use pi_coding_agent::modes::interactive::components::tree_selector::{FilterMode, TreeSelectorComponent};
use pi_tui::tui::Component;
use serde_json::Value;

use super::harness::*;
use super::selectors::feed;

const HOUR_MS: i64 = 3_600_000;

fn session_info(spec: &Value, now: chrono::DateTime<chrono::Utc>) -> SessionInfo {
    let dir = "/home/golden/.pi/agent/sessions/--home-golden-project--";
    let ago = chrono::Duration::milliseconds(spec["ago"].as_i64().unwrap());
    let first = s(spec, "first").to_string();
    SessionInfo {
        path: format!("{dir}/{}.jsonl", s(spec, "id")),
        id: s(spec, "id").into(),
        cwd: opt_s(spec, "cwd").unwrap_or("/home/golden/project").into(),
        name: opt_s(spec, "name").map(str::to_string),
        parent_session_path: opt_s(spec, "parent").map(|p| format!("{dir}/{p}.jsonl")),
        created: now - ago - chrono::Duration::milliseconds(HOUR_MS),
        modified: now - ago,
        message_count: spec["messages"].as_i64().unwrap(),
        first_message: first.clone(),
        all_messages_text: first,
    }
}

fn loader(sessions: Vec<SessionInfo>) -> SessionsLoader {
    Arc::new(move |_, _| {
        let sessions = sessions.clone();
        async move { Ok(sessions) }.boxed()
    })
}

fn settle() {
    std::thread::sleep(Duration::from_millis(30));
}

pub fn build(kind: &str, input: &Value) -> Option<Built> {
    Some(match kind {
        "tree_selector" => {
            let tree: Vec<SessionTreeNode> = from(input, "tree");
            let filter: Option<FilterMode> = opt_from(input, "filter");
            let c = TreeSelectorComponent::new(
                &tree,
                opt_s(input, "leaf"),
                opt_i(input, "height").unwrap_or(24),
                Arc::new(|_| {}),
                Arc::new(|| {}),
                Some(Arc::new(|_, _| {})),
                opt_s(input, "selected"),
                filter.as_ref(),
            );
            if b(input, "focused") {
                c.set_focused(true);
            }
            feed(&c, input);
            component(c)
        }
        "session_selector" => {
            let now = chrono::Utc::now();
            let specs = input["sessions"].as_array().unwrap();
            let current: Vec<SessionInfo> =
                specs.iter().filter(|s| s.get("cwd").is_none()).map(|s| session_info(s, now)).collect();
            let all: Vec<SessionInfo> = specs.iter().map(|s| session_info(s, now)).collect();
            let rename: Option<Arc<dyn Fn(String, Option<String>) -> futures::future::BoxFuture<'static, anyhow::Result<()>> + Send + Sync>> =
                b(input, "rename").then(|| {
                    Arc::new(|_: String, _: Option<String>| async { Ok(()) }.boxed())
                        as Arc<dyn Fn(String, Option<String>) -> futures::future::BoxFuture<'static, anyhow::Result<()>> + Send + Sync>
                });
            let c = SessionSelectorComponent::new(
                &loader(current),
                &loader(all),
                Arc::new(|_| {}),
                Arc::new(|| {}),
                Arc::new(|| {}),
                Arc::new(|| {}),
                Some(&SessionSelectorComponentOptions {
                    rename_session: rename,
                    show_rename_hint: Some(input.get("renameHint").and_then(Value::as_bool).unwrap_or(true)),
                    keybindings: Some(Arc::new(KeybindingsManager::new(None, None))),
                }),
                opt_s(input, "currentPath"),
            );
            settle();
            if b(input, "focused") {
                c.set_focused(true);
            }
            for key in input.get("keys").and_then(Value::as_array).into_iter().flatten() {
                c.handle_input(key.as_str().unwrap());
                settle();
            }
            component(c)
        }
        _ => return None,
    })
}
