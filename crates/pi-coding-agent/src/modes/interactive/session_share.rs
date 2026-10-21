//! Port of `packages/coding-agent/src/modes/interactive/session-share.ts`.

use crate::cli::auth_command::get_auth_credential;
use crate::config::get_share_viewer_url;
use crate::core::agent_session::{AgentSession, AgentSessionExportToHtmlOptions};
use crate::core::model_runtime::{ModelRuntimeAuthOverrides, ModelRuntimeGetAuthProviderOrModel};
use crate::core::session_export::{TrailingEntries, export_session_to_jsonl};
use crate::core::settings_schema::SessionShareDestinationValue;
use crate::modes::interactive::components::bordered_loader::BorderedLoader;
use crate::modes::interactive::external_editor::make_temp_dir;
use crate::modes::interactive::theme::theme::theme;
use pi_ai::providers::radius_config::DEFAULT_RADIUS_GATEWAY;
use pi_tui::components::editor::EditorComponent;
use pi_tui::terminal_image::hyperlink;
use pi_tui::tui::{ComponentRef, Container, Tui};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone)]
pub struct SessionShareContext {
    pub session: Arc<AgentSession>,
    pub ui: Arc<Tui>,
    pub editor_container: Arc<Container>,
    pub editor: Arc<dyn EditorComponent>,
    pub show_status: Arc<dyn Fn(String) + Send + Sync>,
    pub show_error: Arc<dyn Fn(String) + Send + Sync>,
}

/// Trailing `pi.share` entry carrying the system prompt and tool schemas for the session viewer.
pub fn create_share_trailing_entries(session: Arc<AgentSession>, parent_id: Option<&str>, timestamp: &str) -> Vec<Value> {
    let tools: Vec<Value> = session
        .state()
        .tools
        .iter()
        .map(|tool| json!({ "name": tool.name(), "description": tool.description(), "parameters": tool.parameters() }))
        .collect();
    let id: String = uuid::Uuid::new_v4().to_string().chars().take(8).collect();
    vec![json!({
        "type": "custom",
        "customType": "pi.share",
        "id": id,
        "parentId": parent_id,
        "timestamp": timestamp,
        "data": {
            "systemPrompt": session.system_prompt(),
            "tools": tools,
        },
    })]
}

/// The trailing-entries callback for a session (used by `/share` and `/bug`).
pub fn share_trailing_entries(session: Arc<AgentSession>) -> TrailingEntries {
    Arc::new(move |parent_id: Option<String>, timestamp: String| {
        create_share_trailing_entries(session.clone(), parent_id.as_deref(), &timestamp)
    })
}

/// Export the current branch with presentation metadata for Radius.
pub fn export_session_for_share(file_path: &str, session: Arc<AgentSession>) -> anyhow::Result<()> {
    let trailing = share_trailing_entries(session.clone());
    export_session_to_jsonl(&session.session_manager(), Some(file_path), Some(&trailing))?;
    Ok(())
}

/// Share the current session through Radius, falling back to a private gist.
pub async fn share_session(context: &SessionShareContext) -> anyhow::Result<()> {
    let temp_dir = make_temp_dir("pi-share-")?;
    let result = share_in_dir(context, &temp_dir).await;
    // Ignore cleanup errors
    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

async fn share_in_dir(context: &SessionShareContext, temp_dir: &std::path::Path) -> anyhow::Result<()> {
    let jsonl_file = temp_dir.join("session.jsonl").to_string_lossy().into_owned();
    let html_file = temp_dir.join("session.html").to_string_lossy().into_owned();

    if let Err(error) = export_session_for_share(&jsonl_file, context.session.clone()) {
        (context.show_error)(format!("Failed to export session: {error}"));
        return Ok(());
    }
    let destination = context.session.settings_manager().get_session_share_destination();
    if destination != SessionShareDestinationValue::Gist {
        if try_share_via_radius(&jsonl_file, context).await {
            return Ok(());
        }
        if destination == SessionShareDestinationValue::Radius {
            (context.show_error)("Radius authentication is not configured. Run '/login radius' first.".to_string());
            return Ok(());
        }
    }

    match tokio::process::Command::new("gh").args(["auth", "status"]).output().await {
        Ok(output) if output.status.success() => {}
        Ok(_) => {
            (context.show_error)("GitHub CLI is not logged in. Run 'gh auth login' first.".to_string());
            return Ok(());
        }
        Err(_) => {
            (context.show_error)("GitHub CLI (gh) is not installed. Install it from https://cli.github.com/".to_string());
            return Ok(());
        }
    }

    let options = AgentSessionExportToHtmlOptions { theme_name: theme().name() };
    if let Err(error) = context.session.export_to_html(Some(&html_file), Some(&options)).await {
        (context.show_error)(format!("Failed to export session: {error}"));
        return Ok(());
    }
    share_via_gist(&html_file, context).await;
    Ok(())
}

fn show_loader(context: &SessionShareContext, message: &str) -> Arc<BorderedLoader> {
    let loader = Arc::new(BorderedLoader::new(context.ui.clone(), theme(), message, None));
    context.editor_container.clear();
    context.editor_container.add_child(loader.clone() as ComponentRef);
    context.ui.set_focus(Some(loader.clone() as ComponentRef));
    context.ui.request_render(false);
    loader
}

async fn try_share_via_radius(tmp_file: &str, context: &SessionShareContext) -> bool {
    let runtime = context.session.model_runtime();
    if runtime.get_provider("radius").is_none() {
        return false;
    }
    let auth = runtime
        .get_auth(
            &ModelRuntimeGetAuthProviderOrModel::String("radius".to_string()),
            Some(&ModelRuntimeAuthOverrides { min_o_auth_validity_ms: Some(5 * 60_000), ..Default::default() }),
        )
        .await
        .ok()
        .flatten();
    let Some(token) = get_auth_credential(auth.as_ref()) else { return false };

    let loader = show_loader(context, "Uploading to Radius...");
    {
        let ctx = context.clone();
        let weak = Arc::downgrade(&loader);
        loader.set_on_abort(Some(Arc::new(move || {
            if let Some(loader) = weak.upgrade() {
                restore_editor(&loader, &ctx);
            }
            (ctx.show_status)("Share cancelled".to_string());
        })));
    }
    let signal = loader.signal();

    let upload = async {
        let body = std::fs::read(tmp_file)?;
        let mut url = url::Url::parse(DEFAULT_RADIUS_GATEWAY)?.join("/v1/artifacts")?;
        url.query_pairs_mut().append_pair("visibility", "organization").append_pair("title", "Pi session");
        let response = reqwest::Client::new()
            .post(url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/x-ndjson")
            .header("Content-Length", body.len().to_string())
            .body(body)
            .send()
            .await?;
        let status = response.status();
        let json: Option<Value> = response.json().await.ok();
        anyhow::Ok((status, json))
    };
    let result = tokio::select! {
        result = upload => Some(result),
        _ = signal.cancelled() => None,
    };
    let Some(result) = result else { return true };
    if signal.is_cancelled() {
        return true;
    }
    match result {
        Ok((status, json)) => {
            restore_editor(&loader, context);
            let artifact_url = json
                .as_ref()
                .and_then(|j| j.get("artifact"))
                .and_then(|a| a.get("canonical_url"))
                .and_then(Value::as_str)
                .map(str::to_string);
            match artifact_url {
                Some(share_url) if status.is_success() => {
                    (context.show_status)(format!("Share URL: {}", hyperlink(&share_url, &share_url)));
                }
                _ => {
                    let error = json
                        .as_ref()
                        .and_then(|j| j.get("error"))
                        .and_then(Value::as_str)
                        .filter(|e| !e.is_empty())
                        .map(str::to_string)
                        .or_else(|| status.canonical_reason().map(str::to_string))
                        .unwrap_or_else(|| status.as_u16().to_string());
                    (context.show_error)(format!("Failed to upload Radius artifact: {error}"));
                }
            }
        }
        Err(error) => {
            restore_editor(&loader, context);
            (context.show_error)(format!("Failed to upload Radius artifact: {error}"));
        }
    }
    true
}

async fn share_via_gist(tmp_file: &str, context: &SessionShareContext) {
    let loader = show_loader(context, "Creating gist...");
    let signal = loader.signal();
    {
        let ctx = context.clone();
        let weak = Arc::downgrade(&loader);
        loader.set_on_abort(Some(Arc::new(move || {
            if let Some(loader) = weak.upgrade() {
                restore_editor(&loader, &ctx);
            }
            (ctx.show_status)("Share cancelled".to_string());
        })));
    }

    let child = tokio::process::Command::new("gh")
        .args(["gist", "create", "--public=false", tmp_file])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let output = match child {
        Ok(child) => {
            tokio::select! {
                output = child.wait_with_output() => Some(output),
                // Dropping the child kills it (TS: proc.kill()).
                _ = signal.cancelled() => None,
            }
        }
        Err(error) => Some(Err(error)),
    };
    let Some(output) = output else { return };
    if signal.is_cancelled() {
        return;
    }
    restore_editor(&loader, context);
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            (context.show_error)(format!("Failed to create gist: {error}"));
            return;
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        (context.show_error)(format!(
            "Failed to create gist: {}",
            if stderr.is_empty() { "Unknown error".to_string() } else { stderr }
        ));
        return;
    }
    let gist_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let gist_id = gist_url.rsplit('/').next().unwrap_or_default().to_string();
    if gist_id.is_empty() {
        (context.show_error)("Failed to parse gist ID from gh output".to_string());
        return;
    }
    let preview_url = get_share_viewer_url(&gist_id);
    (context.show_status)(format!(
        "Share URL: {}\nGist: {}",
        hyperlink(&preview_url, &preview_url),
        hyperlink(&gist_url, &gist_url)
    ));
}

fn restore_editor(loader: &BorderedLoader, context: &SessionShareContext) {
    loader.dispose();
    context.editor_container.clear();
    let editor: ComponentRef = context.editor.clone();
    context.editor_container.add_child(editor.clone());
    context.ui.set_focus(Some(editor));
}
