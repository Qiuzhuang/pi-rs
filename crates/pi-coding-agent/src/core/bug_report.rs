//! Port of `packages/coding-agent/src/core/bug-report.ts`.

use crate::config::VERSION;
use crate::core::compaction::compaction::{
    complete_summarization, estimate_tokens, get_summarization_failure,
};
use crate::core::compaction::utils::serialize_conversation;
use crate::core::crash_log::CrashRecord;
use crate::core::extensions::types::ExtensionRef;
use crate::core::messages::convert_to_llm;
use crate::core::model_runtime::ModelRuntime;
use crate::core::session_manager::{ReadonlySessionManager, SessionEntry};
use crate::core::settings_schema::Settings;
use crate::utils::pi_user_agent::get_pi_user_agent;
use crate::utils::zip::{ZipEntry, ZipEntryData, write_zip_archive};
use indexmap::IndexMap;
use pi_agent::types::{AgentMessage, StreamFn, ThinkingLevel};
use pi_ai::models::Provider;
use pi_ai::types::{AssistantContent, Context, Message, Model, StopReason, StreamOptions, UserMessage};
use pi_ai::utils::retry::RetryPolicy;
use pi_ai::utils::text::assistant_content_text;
use pi_ai::utils::transcript::normalize_context;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::sync::{Arc, LazyLock};
use tokio_util::sync::CancellationToken;

pub const BUG_REPORT_CUSTOM_ENTRY_TYPE: &str = "pi.bug-report";
const BUG_REPORT_SCHEMA_VERSION: u64 = 1;
const REDACTED: &str = "<redacted>";

static SENSITIVE_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:^|[-_])(api[-_]?key|secret|token|password|passwd|credential|authorization|cookie)(?:$|[-_])",
    )
    .expect("valid regex")
});
static CAMEL_BOUNDARY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([a-z0-9])([A-Z])").expect("valid regex"));
static NESTED_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)^([a-z][a-z0-9+.-]*:)([a-z][a-z0-9+.-]*://.*)$").expect("valid regex")
});

fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEY.is_match(&CAMEL_BOUNDARY.replace_all(key, "${1}_${2}"))
}

/// Strip credentials and secret-looking query parameters from a URL.
pub fn redact_url(value: &str) -> String {
    if let Some(captures) = NESTED_URL.captures(value) {
        return format!("{}{}", &captures[1], redact_url(&captures[2]));
    }
    let Ok(mut url) = url::Url::parse(value) else {
        return value.to_string();
    };
    let mut changed = false;
    if !url.username().is_empty() || url.password().is_some() {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        changed = true;
    }
    if url.query().is_some() {
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        if pairs.iter().any(|(key, _)| is_sensitive_key(key)) {
            // URLSearchParams.set: the first entry of a name gets the value, later ones are removed.
            let mut seen: Vec<String> = Vec::new();
            let mut next: Vec<(String, String)> = Vec::new();
            for (key, value) in pairs {
                if is_sensitive_key(&key) {
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.push(key.clone());
                    next.push((key, REDACTED.to_string()));
                } else {
                    next.push((key, value));
                }
            }
            url.query_pairs_mut().clear().extend_pairs(next);
            changed = true;
        }
    }
    if changed { url.to_string() } else { value.to_string() }
}

fn redact_value(key: &str, value: &Value) -> Value {
    if !value.is_null() && is_sensitive_key(key) {
        return Value::String(REDACTED.to_string());
    }
    match value {
        Value::String(text) => Value::String(redact_url(text)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| redact_value(&index.to_string(), item))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), redact_value(k, v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Copy a JSON value while removing values that may contain credentials.
pub fn redact_json_value(value: Value) -> Value {
    redact_value("", &value)
}

fn redact_settings(settings: &Settings) -> Value {
    let mut value = serde_json::to_value(settings).unwrap_or(Value::Object(Map::new()));
    if let Value::Object(map) = &mut value {
        map.shift_remove("trackingId");
        map.shift_remove("deviceId");
    }
    redact_json_value(value)
}

fn command_output(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Node's `process.platform`.
fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// Node's `process.arch`.
fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        "powerpc64" => "ppc64",
        other => other,
    }
}

fn collect_environment() -> Value {
    let env = |name: &str| -> Value {
        std::env::var(name)
            .ok()
            .filter(|v| !v.is_empty())
            .map(Value::String)
            .unwrap_or(Value::Null)
    };
    let flag = |name: &str| std::env::var(name).is_ok_and(|v| !v.is_empty());
    let (os_release, os_version) = if cfg!(unix) {
        (command_output("uname", &["-r"]), command_output("uname", &["-v"]))
    } else {
        (String::new(), String::new())
    };
    let shell = std::env::var("SHELL")
        .ok()
        .and_then(|shell| shell.split(['/', '\\']).next_back().map(str::to_string))
        .filter(|s| !s.is_empty())
        .map(Value::String)
        .unwrap_or(Value::Null);
    let mut pi_variables: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| name.starts_with("PI_"))
        .collect();
    pi_variables.sort();
    json!({
        "version": VERSION,
        "userAgent": get_pi_user_agent(VERSION),
        // The TS version reports `node/<version>` or `bun/<version>`; this build is native.
        "runtime": "rust",
        "platform": node_platform(),
        "arch": node_arch(),
        "osRelease": os_release,
        "osVersion": os_version,
        "shell": shell,
        "terminal": {
            "term": env("TERM"),
            "program": env("TERM_PROGRAM"),
            "programVersion": env("TERM_PROGRAM_VERSION"),
            "colorterm": env("COLORTERM"),
            "tmux": flag("TMUX"),
            "ssh": flag("SSH_CONNECTION") || flag("SSH_CLIENT") || flag("SSH_TTY"),
            "ci": flag("CI"),
        },
        // Names help diagnose configuration; values never leave the machine.
        "piEnvironmentVariables": pi_variables,
    })
}

fn sorted_keys<'a>(keys: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut keys: Vec<String> = keys.cloned().collect();
    keys.sort();
    keys
}

fn describe_model(model: &Model) -> Value {
    json!({
        "provider": model.provider,
        "id": model.id,
        "name": model.name,
        "api": model.api,
        "baseUrl": redact_url(&model.base_url),
        "reasoning": model.reasoning,
        "input": model.input,
        "contextWindow": model.context_window,
        "maxTokens": model.max_tokens,
        "samplingParams": model.sampling_params.as_ref().map(|p| redact_json_value(Value::Object(p.clone()))).unwrap_or(Value::Null),
        "compat": model.compat.as_ref().map(|c| redact_json_value(serde_json::to_value(c).unwrap_or(Value::Null))).unwrap_or(Value::Null),
        "thinkingLevelMap": model.thinking_level_map.as_ref().map(|m| serde_json::to_value(m).unwrap_or(Value::Null)).unwrap_or(Value::Null),
        "headerNames": sorted_keys(model.headers.iter().flat_map(|h| h.keys())),
    })
}

fn describe_provider(model_runtime: &ModelRuntime, provider: &Arc<dyn Provider>) -> Value {
    let mut auth_types = Vec::new();
    if provider.auth().api_key.is_some() {
        auth_types.push("api_key");
    }
    if provider.auth().oauth.is_some() {
        auth_types.push("oauth");
    }
    json!({
        "id": provider.id(),
        "name": provider.name(),
        "baseUrl": provider.base_url().map(redact_url),
        "headerNames": sorted_keys(provider.headers().into_iter().flat_map(|h| h.keys())),
        "authTypes": auth_types,
        "authStatus": model_runtime.get_provider_auth_status(provider.id()),
        "usingOAuth": model_runtime.is_using_o_auth(provider.id()),
        "registeredByExtension": model_runtime.get_registered_provider_ids().iter().any(|id| id == provider.id()),
    })
}

fn describe_extension(extension: &ExtensionRef) -> Value {
    let extension = extension.read();
    json!({
        "path": extension.path,
        "source": redact_url(&extension.source_info.source),
        "scope": extension.source_info.scope,
        "origin": extension.source_info.origin,
        "hidden": extension.hidden == Some(true),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectBugReportMetadataOptionsExtensionErrors {
    pub path: String,
    pub error: String,
}

#[derive(Clone)]
pub struct CollectBugReportMetadataOptions {
    pub id: Option<String>,
    pub hint: Option<String>,
    pub session_id: String,
    pub cwd: String,
    pub include_session: bool,
    pub include_summary: bool,
    pub message_count: i64,
    pub model: Option<Model>,
    pub model_runtime: Arc<ModelRuntime>,
    pub thinking_level: ThinkingLevel,
    pub extensions: Vec<ExtensionRef>,
    pub extension_errors: Vec<CollectBugReportMetadataOptionsExtensionErrors>,
    pub global_settings: Settings,
    pub project_settings: Settings,
}

fn iso_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn collect_bug_report_metadata(options: &CollectBugReportMetadataOptions) -> BugReportMetadata {
    let hint = options
        .hint
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty());
    let provider = options
        .model
        .as_ref()
        .and_then(|model| options.model_runtime.get_provider(&model.provider));
    let mut session = json!({
        "id": options.session_id,
        "included": options.include_session,
        "summaryIncluded": options.include_summary,
        "messageCount": options.message_count,
    });
    if options.include_session {
        session["cwd"] = Value::String(options.cwd.clone());
    }
    json!({
        "schemaVersion": BUG_REPORT_SCHEMA_VERSION,
        "id": options.id.clone().unwrap_or_else(pi_ai::uuidv7),
        "createdAt": iso_now(),
        "hint": hint,
        "environment": collect_environment(),
        "session": session,
        "model": options.model.as_ref().map(describe_model),
        "provider": provider.as_ref().map(|p| describe_provider(&options.model_runtime, p)),
        "thinkingLevel": options.thinking_level,
        "extensions": options.extensions.iter().map(describe_extension).collect::<Vec<_>>(),
        "extensionErrors": options.extension_errors.iter().map(|e| json!({ "path": e.path, "error": e.error })).collect::<Vec<_>>(),
        "settings": {
            "global": redact_settings(&options.global_settings),
            "project": redact_settings(&options.project_settings),
        },
    })
}

/// Collect failed assistant turns without collecting conversation content.
pub fn collect_bug_report_diagnostics(
    session_manager: &ReadonlySessionManager,
    crashes: Option<&[CrashRecord]>,
) -> BugReportDiagnostics {
    let entries = session_manager.get_entries();
    let mut assistant = Vec::new();
    let mut assistant_message_count = 0;
    for entry in &entries {
        let SessionEntry::Message(entry) = entry else { continue };
        let AgentMessage::Assistant(message) = &entry.message else { continue };
        assistant_message_count += 1;
        let diagnostics = message.diagnostics.clone().unwrap_or_default();
        if diagnostics.is_empty()
            && message.stop_reason != StopReason::Error
            && message.stop_reason != StopReason::Aborted
            && message.error_message.as_deref().is_none_or(str::is_empty)
        {
            continue;
        }
        let mut item = json!({
            "entryId": entry.id,
            "timestamp": entry.timestamp,
            "provider": message.provider,
            "model": message.model,
            "api": message.api,
            "stopReason": message.stop_reason,
        });
        if let Some(raw) = &message.raw_stop_reason {
            item["rawStopReason"] = Value::String(raw.clone());
        }
        if let Some(error) = &message.error_message {
            item["errorMessage"] = Value::String(error.clone());
        }
        item["diagnostics"] = serde_json::to_value(&diagnostics).unwrap_or(Value::Array(Vec::new()));
        assistant.push(item);
    }
    let crashes: Vec<Value> = crashes
        .unwrap_or_default()
        .iter()
        .map(|record| {
            let mut value = serde_json::to_value(record).unwrap_or(Value::Null);
            if let Value::Object(map) = &mut value {
                map.shift_remove("notified");
            }
            value
        })
        .collect();
    json!({
        "schemaVersion": BUG_REPORT_SCHEMA_VERSION,
        "sessionId": session_manager.get_session_id(),
        "entryCount": entries.len(),
        "assistantMessageCount": assistant_message_count,
        "assistant": assistant,
        "crashes": crashes,
    })
}

pub type BugReportMetadata = Value;

pub type BugReportDiagnostics = Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportBundle {
    pub metadata: BugReportMetadata,
    pub diagnostics: BugReportDiagnostics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_jsonl: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BugReportSessionEntryDataDelivery {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "upload")]
    Upload,
}

impl BugReportSessionEntryDataDelivery {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Upload => "upload",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportSessionEntryData {
    pub id: String,
    pub created_at: String,
    #[serde(default)]
    pub hint: Option<String>,
    pub session_included: bool,
    pub summary_included: bool,
    pub delivery: BugReportSessionEntryDataDelivery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportFile {
    pub name: String,
    pub content_type: String,
    pub data: String,
}

fn pretty_json(value: &Value) -> String {
    format!("{}\n", serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_string()))
}

/// Files shared by upload and zip export.
pub fn bug_report_files(bundle: &BugReportBundle) -> Vec<BugReportFile> {
    let mut files = vec![
        BugReportFile {
            name: "report.json".to_string(),
            content_type: "application/json".to_string(),
            data: pretty_json(&bundle.metadata),
        },
        BugReportFile {
            name: "diagnostics.json".to_string(),
            content_type: "application/json".to_string(),
            data: pretty_json(&bundle.diagnostics),
        },
    ];
    if let Some(session) = &bundle.session_jsonl {
        files.push(BugReportFile {
            name: "session.jsonl".to_string(),
            content_type: "application/x-ndjson".to_string(),
            data: session.clone(),
        });
    }
    if let Some(summary) = &bundle.summary {
        files.push(BugReportFile {
            name: "summary.md".to_string(),
            content_type: "text/markdown".to_string(),
            data: if summary.ends_with('\n') { summary.clone() } else { format!("{summary}\n") },
        });
    }
    files
}

pub async fn write_bug_report_archive(bundle: &BugReportBundle, file_path: &str) -> anyhow::Result<()> {
    let entries: Vec<ZipEntry> = bug_report_files(bundle)
        .into_iter()
        .map(|file| ZipEntry { name: file.name, data: ZipEntryData::String(file.data) })
        .collect();
    write_zip_archive(file_path, &entries).await
}

pub fn bug_report_archive_file_name(id: &str) -> String {
    format!("pi-bug-report-{id}.zip")
}

const BUG_SUMMARY_SYSTEM_PROMPT: &str = "You are helping a user file a bug report about pi, the coding agent they are talking to. You will be shown the conversation transcript. Write a report for the pi developers describing what the user was doing and what went wrong.

Do NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the report.";

const BUG_SUMMARY_INSTRUCTIONS: &str = "Write the bug report in Markdown with these sections:

## What the user was doing
One short paragraph.

## What went wrong
Concrete description of the failure: wrong output, errors, hangs, tool failures, unexpected behavior. Quote error messages and tool output verbatim where they exist.

## Steps to reproduce
Numbered list, as specific as the transcript allows.

## Relevant details
Tool calls involved, files touched, model behavior, anything else that helps a developer reproduce or locate the problem.

Do not include file contents, secrets, or credentials from the transcript; refer to files by path only. Keep the report factual and concise.";

fn select_messages(messages: &[AgentMessage], token_budget: u64) -> Vec<AgentMessage> {
    let mut selected: Vec<AgentMessage> = Vec::new();
    let mut tokens = 0;
    for message in messages.iter().rev() {
        let next = estimate_tokens(message);
        if !selected.is_empty() && tokens + next > token_budget {
            break;
        }
        selected.push(message.clone());
        tokens += next;
    }
    selected.reverse();
    selected
}

#[derive(Clone)]
pub struct GenerateBugReportSummaryOptions {
    pub messages: Vec<AgentMessage>,
    pub hint: Option<String>,
    pub model: Model,
    pub api_key: Option<String>,
    pub headers: Option<IndexMap<String, String>>,
    pub env: Option<IndexMap<String, String>>,
    pub signal: CancellationToken,
    pub thinking_level: Option<ThinkingLevel>,
    pub stream_fn: Option<StreamFn>,
    pub retry: Option<RetryPolicy>,
    pub session_id: Option<String>,
}

/// Ask the session model for a report when the user does not share the transcript.
pub async fn generate_bug_report_summary(options: &GenerateBugReportSummaryOptions) -> anyhow::Result<String> {
    let model = &options.model;
    let context_window = if model.context_window > 0 { model.context_window } else { 128_000 };
    let messages = select_messages(&options.messages, (context_window as f64 * 0.6).floor() as u64);
    let hint = options.hint.as_deref().map(str::trim).filter(|h| !h.is_empty());
    let mut parts: Vec<String> = Vec::new();
    if messages.len() < options.messages.len() {
        parts.push(format!(
            "Note: only the last {} of {} messages are shown.",
            messages.len(),
            options.messages.len()
        ));
    }
    parts.push(format!(
        "<conversation>\n{}\n</conversation>",
        serialize_conversation(&convert_to_llm(&messages))
    ));
    if let Some(hint) = hint {
        parts.push(format!("<user-report>\n{hint}\n</user-report>"));
    }
    parts.push(BUG_SUMMARY_INSTRUCTIONS.to_string());
    let prompt = parts.join("\n\n");

    let request_options = StreamOptions {
        max_tokens: Some(if model.max_tokens > 0 { model.max_tokens.min(4096) } else { 4096 }),
        signal: Some(options.signal.clone()),
        api_key: options.api_key.clone(),
        headers: options
            .headers
            .as_ref()
            .map(|headers| headers.iter().map(|(k, v)| (k.clone(), Some(v.clone()))).collect()),
        env: options
            .env
            .as_ref()
            .map(|env| env.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        session_id: options.session_id.clone(),
        reasoning: if model.reasoning {
            options.thinking_level.and_then(|level| level.thinking())
        } else {
            None
        },
        ..Default::default()
    };
    let context = normalize_context(&Context {
        system_prompt: Some(BUG_SUMMARY_SYSTEM_PROMPT.to_string()),
        messages: vec![Message::User(UserMessage::new(prompt, pi_ai::utils::now_ms()))],
        tools: None,
    });
    let response = complete_summarization(
        model,
        context,
        request_options,
        options.stream_fn.clone(),
        options.retry.as_ref(),
        None,
    )
    .await?;
    if response.stop_reason == StopReason::Aborted {
        anyhow::bail!("Bug report summary was cancelled");
    }
    if let Some(failure) = get_summarization_failure(&response, "Bug report summary") {
        anyhow::bail!(failure);
    }
    if response
        .content
        .iter()
        .any(|block| matches!(block, AssistantContent::ToolCall(_)))
    {
        anyhow::bail!("Bug report summary attempted to call a tool");
    }
    let text = assistant_content_text(&response.content, "\n").trim().to_string();
    if text.is_empty() {
        anyhow::bail!("Bug report summary was empty");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_urls() {
        assert_eq!(
            redact_url("https://user:pass@example.com/path?api_key=abc&x=1"),
            "https://example.com/path?api_key=%3Credacted%3E&x=1"
        );
        assert_eq!(redact_url("https://example.com/path?x=1"), "https://example.com/path?x=1");
        assert_eq!(redact_url("not a url"), "not a url");
        assert_eq!(
            redact_url("git+https://token@github.com/a/b"),
            "git+https://github.com/a/b"
        );
        assert_eq!(
            redact_url("https://example.com/?accessToken=1"),
            "https://example.com/?accessToken=%3Credacted%3E"
        );
    }

    #[test]
    fn redacts_json_values() {
        let value = json!({
            "apiKey": "secret",
            "nested": { "authorization": "Bearer x", "plain": "https://u:p@h.example/" },
            "list": ["https://u:p@h.example/x"],
            "tokenCount": null,
            "maxTokens": 5,
        });
        assert_eq!(
            redact_json_value(value),
            json!({
                "apiKey": "<redacted>",
                "nested": { "authorization": "<redacted>", "plain": "https://h.example/" },
                "list": ["https://h.example/x"],
                "tokenCount": null,
                "maxTokens": 5,
            })
        );
    }

    #[test]
    fn sensitive_keys() {
        assert!(is_sensitive_key("apiKey"));
        assert!(is_sensitive_key("x-api-key"));
        assert!(is_sensitive_key("refresh_token"));
        assert!(is_sensitive_key("Cookie"));
        assert!(!is_sensitive_key("maxTokens"));
        assert!(!is_sensitive_key("tokenizer"));
    }

    #[test]
    fn builds_archive_files() {
        let bundle = BugReportBundle {
            metadata: json!({ "id": "x" }),
            diagnostics: json!({}),
            session_jsonl: Some("{}\n".to_string()),
            summary: Some("summary".to_string()),
        };
        let files = bug_report_files(&bundle);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["report.json", "diagnostics.json", "session.jsonl", "summary.md"]);
        assert_eq!(files[0].data, "{\n  \"id\": \"x\"\n}\n");
        assert_eq!(files[3].data, "summary\n");
        assert_eq!(bug_report_archive_file_name("abc"), "pi-bug-report-abc.zip");
    }
}
