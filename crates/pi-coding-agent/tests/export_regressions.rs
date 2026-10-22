//! HTML export regressions on a live session: ports of
//! `test/suite/regressions/5596-missing-theme-export.test.ts` and the export case of
//! `test/suite/regressions/10285-mcp-tool-renderers.test.ts` (its renderer case is
//! `extension_renders_calls_to_mcp_tools_that_are_not_registered` in `src/extensions/mcp/tests.rs`).

mod support;

use base64::Engine;
use pi_agent::types::AgentMessage;
use pi_coding_agent::core::session_manager::SessionManager;
use pi_coding_agent::extensions::mcp::config::LoadedMcpConfig;
use pi_coding_agent::extensions::mcp::{McpExtensionOptions, create_mcp_extension};
use serde_json::{Value, json};
use std::sync::Arc;
use support::*;

fn session_data(html: &str) -> Value {
    let re = regex::Regex::new(r#"<script id="session-data" type="application/json">([^<]*)</script>"#).unwrap();
    let encoded = &re.captures(html).expect("session data")[1];
    let decoded = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}

fn message(value: Value) -> AgentMessage {
    serde_json::from_value(value).unwrap()
}

fn persisted_session(dir: &std::path::Path) -> Arc<SessionManager> {
    let cwd = dir.to_string_lossy().into_owned();
    let sessions = dir.join("sessions").to_string_lossy().into_owned();
    SessionManager::create(&cwd, Some(&sessions), None).unwrap()
}

// #5596
#[tokio::test]
async fn exports_with_the_active_fallback_theme_when_the_configured_theme_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let harness = create_harness(HarnessOptions {
        settings: Some(json!({ "theme": "missing-theme" })),
        session_manager: Some(persisted_session(dir.path())),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![faux_text_message("hello").into()]);
    harness.session.prompt("hi", None).await.unwrap();

    let output = dir.path().join("export.html").to_string_lossy().into_owned();
    assert_eq!(harness.session.export_to_html(Some(&output), None).await.unwrap(), output);
    assert!(std::path::Path::new(&output).exists());
    assert_eq!(harness.settings_manager.get_theme().as_deref(), Some("missing-theme"));
    harness.cleanup();
}

// #10285
#[tokio::test]
async fn renders_calls_to_unregistered_mcp_tools_in_html_exports() {
    let dir = tempfile::tempdir().unwrap();
    let session_manager = persisted_session(dir.path());
    let mcp = create_mcp_extension(Some(&McpExtensionOptions {
        load_config: Some(Arc::new(|_| LoadedMcpConfig {
            servers: Vec::new(),
            auto_enable_codemode: None,
            errors: Vec::new(),
            project_config: None,
        })),
        ..Default::default()
    }))
    .unwrap();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![TestExtension::new(mcp)],
        session_manager: Some(session_manager.clone()),
        ..Default::default()
    })
    .await
    .unwrap();

    session_manager.append_message(message(json!({ "role": "user", "content": "search", "timestamp": 1 }))).unwrap();
    session_manager
        .append_message(message(json!({
            "role": "assistant",
            "content": [{ "type": "toolCall", "id": "call-1", "name": "mcp__my_docs__search", "arguments": { "query": "pi" } }],
            "api": "anthropic-messages",
            "provider": "anthropic",
            "model": "test",
            "usage": {
                "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0,
                "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0 }
            },
            "stopReason": "toolUse",
            "timestamp": 2
        })))
        .unwrap();

    let output = dir.path().join("export.html").to_string_lossy().into_owned();
    let html = std::fs::read_to_string(harness.session.export_to_html(Some(&output), None).await.unwrap()).unwrap();
    let data = session_data(&html);
    let call_html = data["renderedTools"]["call-1"]["callHtml"].as_str().unwrap_or_default();
    assert!(call_html.contains("my_docs/search"), "{call_html}");
    harness.cleanup();
}
