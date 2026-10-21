//! Port of `packages/coding-agent/src/modes/interactive/bug-report.ts`.

use crate::cli::auth_command::get_auth_credential;
use crate::core::agent_session::{AgentSession, AgentSessionSummarizeForBugReportOptions};
use crate::core::bug_report::{
    BUG_REPORT_CUSTOM_ENTRY_TYPE, BugReportBundle, BugReportSessionEntryData, BugReportSessionEntryDataDelivery,
    CollectBugReportMetadataOptions, CollectBugReportMetadataOptionsExtensionErrors, bug_report_archive_file_name,
    collect_bug_report_diagnostics, collect_bug_report_metadata, write_bug_report_archive,
};
use crate::core::bug_report_upload::{UploadBugReportOptions, upload_bug_report};
use crate::core::crash_log::{clear_crash_log, read_crash_log};
use crate::core::keybindings::KeybindingsManager;
use crate::core::model_runtime::{ModelRuntimeAuthOverrides, ModelRuntimeGetAuthProviderOrModel};
use crate::core::radius::{RADIUS_PROVIDER_ID, get_radius_gateway_url};
use crate::core::session_export::serialize_session_branch;
use crate::modes::interactive::components::bordered_loader::BorderedLoader;
use crate::modes::interactive::components::extension_editor::{ExtensionEditorComponent, ExtensionEditorOptions};
use crate::modes::interactive::components::extension_selector::{
    ExtensionSelectorComponent, ExtensionSelectorOptions,
};
use crate::modes::interactive::session_share::share_trailing_entries;
use crate::modes::interactive::theme::theme::theme;
use parking_lot::Mutex;
use pi_tui::components::editor::EditorComponent;
use pi_tui::tui::{ComponentRef, Container, Tui};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct BugReportContext {
    pub session: Arc<AgentSession>,
    pub ui: Arc<Tui>,
    pub editor_container: Arc<Container>,
    pub editor: Arc<dyn EditorComponent>,
    pub keybindings: Arc<KeybindingsManager>,
    pub show_status: Arc<dyn Fn(String) + Send + Sync>,
    pub show_error: Arc<dyn Fn(String) + Send + Sync>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BugReportOptionsDelivery {
    #[serde(rename = "upload")]
    Upload,
    #[serde(rename = "zip")]
    Zip,
}

impl BugReportOptionsDelivery {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Zip => "zip",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub include_session: bool,
    pub include_summary: bool,
    pub delivery: BugReportOptionsDelivery,
}

const DISCLAIMER: &str = "This report goes to the Pi developers (Earendil) and is not shared publicly. It includes your pi version, operating system, the current model and provider configuration (without API keys), loaded extensions, settings, and provider error diagnostics from this session.";
const TRANSCRIPT_NOTE: &str = "The transcript contains your messages, model output, tool calls and their results, including file contents and command output read during this session.";

/// A component shown in place of the editor, disposed when the editor returns.
struct Overlay {
    component: ComponentRef,
    dispose: Option<Box<dyn Fn() + Send + Sync>>,
}

fn model_name(session: &AgentSession) -> String {
    session.model().map(|m| m.name.clone()).unwrap_or_else(|| "the current model".to_string())
}

/// Run the `/bug` flow: consent, optional summary, then upload or export.
pub async fn report_bug(context: &BugReportContext, initial_hint: Option<&str>) -> anyhow::Result<()> {
    let Some(options) = prompt_for_options(context, initial_hint).await else {
        (context.show_status)("Bug report cancelled".to_string());
        return Ok(());
    };
    if options.delivery == BugReportOptionsDelivery::Upload && std::env::var_os("PI_OFFLINE").is_some_and(|v| !v.is_empty()) {
        (context.show_error)("Uploading bug reports requires online mode. Use Export as Zip instead.".to_string());
        return Ok(());
    }

    let mut summary = None;
    if options.include_summary {
        let (loader, overlay) = show_loader(context, &format!("Writing summary with {}...", model_name(&context.session)));
        let signal = loader.signal();
        let result = context
            .session
            .summarize_for_bug_report(&AgentSessionSummarizeForBugReportOptions {
                hint: options.hint.clone(),
                signal: signal.clone(),
            })
            .await;
        restore_editor(context, &overlay);
        match result {
            Err(error) => {
                if signal.is_cancelled() {
                    (context.show_status)("Bug report cancelled".to_string());
                } else {
                    (context.show_error)(format!("Failed to write bug report summary: {error}"));
                }
                return Ok(());
            }
            Ok(text) => {
                if signal.is_cancelled() {
                    (context.show_status)("Bug report cancelled".to_string());
                    return Ok(());
                }
                summary = Some(text);
            }
        }
    }

    let bundle = match build_bundle(&context.session, &options, summary) {
        Ok(bundle) => bundle,
        Err(error) => {
            (context.show_error)(format!("Failed to build bug report: {error}"));
            return Ok(());
        }
    };

    if options.delivery == BugReportOptionsDelivery::Upload {
        let Some(failure) = upload(context, &bundle).await else { return Ok(()) };
        let fallback = choose(
            context,
            "Upload failed",
            &["Export as Zip", "Cancel"],
            Some(&format!("{failure}\n\nExport the report as a zip archive instead?")),
        )
        .await;
        if fallback.as_deref() != Some("Export as Zip") {
            (context.show_status)("Bug report cancelled".to_string());
            return Ok(());
        }
    }
    export_zip(context, &bundle).await;
    Ok(())
}

async fn prompt_for_options(context: &BugReportContext, initial_hint: Option<&str>) -> Option<BugReportOptions> {
    let hint = input(context, "Report a bug", &format!("{DISCLAIMER}\n\nWhat went wrong? (optional)"), initial_hint).await?;
    let transcript = choose(
        context,
        "Include the session transcript?",
        &["Yes, include the transcript", "No"],
        Some(TRANSCRIPT_NOTE),
    )
    .await?;
    let include_session = transcript != "No";
    let mut include_summary = false;
    if !include_session {
        let model = context.session.model();
        let name = model.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| "the current model".to_string());
        let provider = model.as_ref().map(|m| m.provider.to_string()).unwrap_or_else(|| "your provider".to_string());
        let summary = choose(
            context,
            &format!("Attach a summary written by {name} instead?"),
            &["Yes, generate a summary", "No"],
            Some(&format!(
                "The transcript is sent to {provider} with your credentials and tokens. Only the generated summary is attached; the transcript stays on your machine."
            )),
        )
        .await?;
        include_summary = summary != "No";
    }
    let description = hint.trim().to_string();
    let host = url::Url::parse(&get_radius_gateway_url())
        .ok()
        .map(|u| match (u.host_str(), u.port()) {
            (Some(h), Some(p)) => format!("{h}:{p}"),
            (Some(h), None) => h.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    let delivery = choose(
        context,
        "Bug report",
        &["Upload Report", "Export as Zip", "Cancel"],
        Some(&format!(
            "Description: {}\nTranscript: {}\nSummary: {}\n\nUpload sends the report to {host}. Export writes a zip archive to the current directory instead.",
            if description.is_empty() { "none" } else { &description },
            if include_session { "included" } else { "not included" },
            if include_summary { format!("written by {}", model_name(&context.session)) } else { "none".to_string() },
        )),
    )
    .await?;
    if delivery == "Cancel" {
        return None;
    }
    Some(BugReportOptions {
        hint: if description.is_empty() { None } else { Some(description) },
        include_session,
        include_summary,
        delivery: if delivery == "Upload Report" { BugReportOptionsDelivery::Upload } else { BugReportOptionsDelivery::Zip },
    })
}

fn build_bundle(
    session: &Arc<AgentSession>,
    options: &BugReportOptions,
    summary: Option<String>,
) -> anyhow::Result<BugReportBundle> {
    let extensions = session.resource_loader().get_extensions();
    let session_manager = session.session_manager();
    let metadata = collect_bug_report_metadata(&CollectBugReportMetadataOptions {
        id: None,
        hint: options.hint.clone(),
        session_id: session.session_id(),
        cwd: session_manager.get_cwd(),
        include_session: options.include_session,
        include_summary: summary.is_some(),
        message_count: session.messages().len() as i64,
        model: session.model(),
        model_runtime: session.model_runtime(),
        thinking_level: session.thinking_level(),
        extensions: extensions.extensions.clone(),
        extension_errors: extensions
            .errors
            .iter()
            .map(|e| CollectBugReportMetadataOptionsExtensionErrors { path: e.path.clone(), error: e.error.clone() })
            .collect(),
        global_settings: session.settings_manager().get_global_settings(),
        project_settings: session.settings_manager().get_project_settings(),
    });
    let crashes = read_crash_log(None);
    let diagnostics = collect_bug_report_diagnostics(&session_manager, Some(&crashes));
    let session_jsonl = if options.include_session {
        let trailing = share_trailing_entries(session.clone());
        Some(serialize_session_branch(&session_manager, Some(&trailing)))
    } else {
        None
    };
    Ok(BugReportBundle { metadata, diagnostics, session_jsonl, summary })
}

async fn upload(context: &BugReportContext, bundle: &BugReportBundle) -> Option<String> {
    let (loader, overlay) = show_loader(context, "Uploading bug report...");
    let signal = loader.signal();
    let runtime = context.session.model_runtime();
    let token = if runtime.get_provider(RADIUS_PROVIDER_ID).is_some() {
        let auth = runtime
            .get_auth(
                &ModelRuntimeGetAuthProviderOrModel::String(RADIUS_PROVIDER_ID.to_string()),
                Some(&ModelRuntimeAuthOverrides { min_o_auth_validity_ms: Some(5 * 60_000), ..Default::default() }),
            )
            .await;
        match auth {
            Ok(auth) => get_auth_credential(auth.as_ref()),
            Err(error) => {
                restore_editor(context, &overlay);
                return Some(error.to_string());
            }
        }
    } else {
        None
    };
    let result = upload_bug_report(
        bundle,
        Some(&UploadBugReportOptions { token, signal: Some(signal.clone()), gateway_url: None }),
    )
    .await;
    restore_editor(context, &overlay);
    match result {
        Ok(result) => {
            record_in_session(&context.session, bundle, BugReportSessionEntryDataDelivery::Upload, None);
            (context.show_status)(format!("Bug report uploaded. Report ID: {}", result.id));
            None
        }
        Err(error) => {
            if signal.is_cancelled() {
                (context.show_status)("Bug report cancelled".to_string());
                return None;
            }
            Some(error.to_string())
        }
    }
}

fn metadata_id(bundle: &BugReportBundle) -> String {
    bundle.metadata.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string()
}

async fn export_zip(context: &BugReportContext, bundle: &BugReportBundle) {
    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| ".".to_string());
    let archive_path = std::path::Path::new(&cwd)
        .join(bug_report_archive_file_name(&metadata_id(bundle)))
        .to_string_lossy()
        .into_owned();
    if let Err(error) = write_bug_report_archive(bundle, &archive_path).await {
        (context.show_error)(format!("Failed to write bug report: {error}"));
        return;
    }
    record_in_session(&context.session, bundle, BugReportSessionEntryDataDelivery::Zip, Some(archive_path.clone()));
    (context.show_status)(format!("Bug report exported to: {archive_path}\nReport ID: {}", metadata_id(bundle)));
}

fn record_in_session(
    session: &AgentSession,
    bundle: &BugReportBundle,
    delivery: BugReportSessionEntryDataDelivery,
    path: Option<String>,
) {
    let metadata = &bundle.metadata;
    let data = BugReportSessionEntryData {
        id: metadata_id(bundle),
        created_at: metadata.get("createdAt").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        hint: metadata.get("hint").and_then(|v| v.as_str()).map(str::to_string),
        session_included: metadata.pointer("/session/included").and_then(|v| v.as_bool()).unwrap_or(false),
        summary_included: metadata.pointer("/session/summaryIncluded").and_then(|v| v.as_bool()).unwrap_or(false),
        delivery,
        path,
    };
    let _ = session
        .session_manager()
        .append_custom_entry(BUG_REPORT_CUSTOM_ENTRY_TYPE, serde_json::to_value(&data).ok());
    let has_crashes = bundle.diagnostics.get("crashes").and_then(|c| c.as_array()).is_some_and(|c| !c.is_empty());
    if has_crashes {
        clear_crash_log(None);
    }
}

async fn input(context: &BugReportContext, title: &str, description: &str, initial_value: Option<&str>) -> Option<String> {
    let (tx, rx) = oneshot::channel::<Option<String>>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let overlay_slot: Arc<Mutex<Option<Arc<Overlay>>>> = Arc::new(Mutex::new(None));
    let finish = {
        let ctx = context.clone();
        let tx = tx.clone();
        let slot = overlay_slot.clone();
        Arc::new(move |value: Option<String>| {
            if let Some(overlay) = slot.lock().take() {
                restore_editor(&ctx, &overlay);
            }
            if let Some(tx) = tx.lock().take() {
                let _ = tx.send(value);
            }
        })
    };
    let on_submit = {
        let finish = finish.clone();
        Arc::new(move |value: String| finish(Some(value)))
    };
    let on_cancel = {
        let finish = finish.clone();
        Arc::new(move || finish(None))
    };
    let external = context.session.settings_manager().get_external_editor_command();
    let component = Arc::new(ExtensionEditorComponent::new(
        context.ui.clone(),
        context.keybindings.clone(),
        title,
        initial_value,
        on_submit,
        on_cancel,
        Some(&ExtensionEditorOptions {
            padding_x: None,
            autocomplete_max_visible: None,
            description: Some(description.to_string()),
        }),
        Some(&external),
    ));
    let overlay = Arc::new(Overlay { component: component as ComponentRef, dispose: None });
    *overlay_slot.lock() = Some(overlay.clone());
    show_overlay(context, &overlay);
    rx.await.ok().flatten()
}

async fn choose(context: &BugReportContext, title: &str, options: &[&str], description: Option<&str>) -> Option<String> {
    let (tx, rx) = oneshot::channel::<Option<String>>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let overlay_slot: Arc<Mutex<Option<Arc<Overlay>>>> = Arc::new(Mutex::new(None));
    let finish = {
        let ctx = context.clone();
        let tx = tx.clone();
        let slot = overlay_slot.clone();
        Arc::new(move |value: Option<String>| {
            if let Some(overlay) = slot.lock().take() {
                restore_editor(&ctx, &overlay);
            }
            if let Some(tx) = tx.lock().take() {
                let _ = tx.send(value);
            }
        })
    };
    let on_select = {
        let finish = finish.clone();
        Arc::new(move |value: String| finish(Some(value)))
    };
    let on_cancel = {
        let finish = finish.clone();
        Arc::new(move || finish(None))
    };
    let options: Vec<String> = options.iter().map(|s| s.to_string()).collect();
    let component = Arc::new(ExtensionSelectorComponent::new(
        title,
        &options,
        on_select,
        on_cancel,
        Some(&ExtensionSelectorOptions {
            tui: Some(context.ui.clone()),
            timeout: None,
            on_toggle_tools_expanded: None,
            description: description.map(str::to_string),
        }),
    ));
    let dispose_target = component.clone();
    let overlay = Arc::new(Overlay {
        component: component as ComponentRef,
        dispose: Some(Box::new(move || dispose_target.dispose())),
    });
    *overlay_slot.lock() = Some(overlay.clone());
    show_overlay(context, &overlay);
    rx.await.ok().flatten()
}

fn show_loader(context: &BugReportContext, message: &str) -> (Arc<BorderedLoader>, Arc<Overlay>) {
    let loader = Arc::new(BorderedLoader::new(context.ui.clone(), theme(), message, None));
    let dispose_target = loader.clone();
    let overlay = Arc::new(Overlay {
        component: loader.clone() as ComponentRef,
        dispose: Some(Box::new(move || dispose_target.dispose())),
    });
    show_overlay(context, &overlay);
    (loader, overlay)
}

fn show_overlay(context: &BugReportContext, overlay: &Overlay) {
    context.editor_container.clear();
    context.editor_container.add_child(overlay.component.clone());
    context.ui.set_focus(Some(overlay.component.clone()));
    context.ui.request_render(false);
}

fn restore_editor(context: &BugReportContext, overlay: &Overlay) {
    if let Some(dispose) = &overlay.dispose {
        dispose();
    }
    context.editor_container.clear();
    let editor: ComponentRef = context.editor.clone();
    context.editor_container.add_child(editor.clone());
    context.ui.set_focus(Some(editor));
    context.ui.request_render(false);
}
