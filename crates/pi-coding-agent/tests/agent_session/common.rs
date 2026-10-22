//! Helpers shared by the session suites (on top of `support`).
#![allow(dead_code)]

use std::future::Future;
use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::{AgentToolRef, AgentToolResult, FnAgentTool};
use pi_ai::providers::faux::FauxResponseStep;
use pi_ai::types::{AssistantContent, AssistantMessage, Message, StopReason, TranscriptContext, UserContent};
use pi_coding_agent::core::agent_session::{AgentSessionEvent, AgentSessionEventListener};
use pi_coding_agent::core::extensions::types::{ExtensionAPIRef, ExtensionFactory};
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::support::*;

pub fn text(text: &str) -> FauxResponseStep {
    faux_text_message(text).into()
}

pub fn tool_call(name: &str, args: Value) -> AssistantContent {
    faux_tool_call(name, args.as_object().cloned().unwrap_or_default(), None)
}

pub fn message(content: Vec<AssistantContent>, stop_reason: Option<StopReason>) -> AssistantMessage {
    faux_assistant_message(
        content,
        FauxMessageOptions {
            stop_reason,
            ..Default::default()
        },
    )
}

/// `fauxAssistantMessage(fauxToolCall(name, args), { stopReason: "toolUse" })`.
pub fn tool_use(name: &str, args: Value) -> FauxResponseStep {
    message(vec![tool_call(name, args)], Some(StopReason::ToolUse)).into()
}

/// A response computed from the request context.
pub fn respond_with(
    f: impl Fn(&TranscriptContext) -> AssistantMessage + Send + Sync + 'static,
) -> FauxResponseStep {
    FauxResponseStep::factory(move |context, _options, _state, _model| f(context))
}

/// Text of an LLM message (text blocks joined by `\n`).
pub fn llm_text(message: &Message) -> String {
    match message {
        Message::User(m) => match &m.content {
            UserContent::Text(t) => t.clone(),
            UserContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| b.as_text().map(|t| t.text.clone()))
                .collect::<Vec<_>>()
                .join("\n"),
        },
        Message::Assistant(m) => m
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("\n"),
        Message::ToolResult(m) => m
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub fn user_llm_texts(context: &TranscriptContext) -> Vec<String> {
    context
        .messages
        .iter()
        .filter(|m| matches!(m, Message::User(_)))
        .map(llm_text)
        .collect()
}

pub fn factory<F, Fut>(f: F) -> TestExtension
where
    F: Fn(ExtensionAPIRef) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let f: ExtensionFactory = Arc::new(move |pi| Box::pin(f(pi)));
    TestExtension::new(f)
}

/// An extension factory from a synchronous setup closure.
pub fn ext(f: impl Fn(ExtensionAPIRef) -> anyhow::Result<()> + Send + Sync + 'static) -> TestExtension {
    let f = Arc::new(f);
    factory(move |pi| {
        let f = f.clone();
        async move { f(pi) }
    })
}

/// A tool that blocks until released.
pub fn wait_tool(release: Arc<Notify>) -> AgentToolRef {
    FnAgentTool::new(
        "wait",
        "Wait",
        "Wait for release",
        json!({"type": "object", "properties": {}}),
        move |_id, _params, _signal, _update| {
            let release = release.clone();
            async move {
                release.notified().await;
                Ok(AgentToolResult::text("released", Some(json!({}))))
            }
        },
    )
    .into_ref()
}

/// A tool returning `text`.
pub fn simple_tool(name: &str, text: &'static str) -> AgentToolRef {
    FnAgentTool::new(
        name,
        name,
        format!("{name} tool"),
        json!({"type": "object", "properties": {}}),
        move |_id, _params, _signal, _update| async move {
            Ok(AgentToolResult::text(text, Some(json!({}))))
        },
    )
    .into_ref()
}

/// Resolves once the session emits `tool_execution_start` for `tool_name`.
pub fn wait_for_tool_start(harness: &Harness, tool_name: &str) -> tokio::sync::oneshot::Receiver<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Mutex::new(Some(tx));
    let tool_name = tool_name.to_string();
    let listener: AgentSessionEventListener = Arc::new(move |event| {
        if let AgentSessionEvent::ToolExecutionStart { tool_name: name, .. } = &event
            && *name == tool_name
            && let Some(tx) = tx.lock().take()
        {
            let _ = tx.send(());
        }
    });
    harness.session.subscribe(&listener);
    rx
}

/// Resolves on the first session event matching `predicate`.
pub fn wait_for_event(
    harness: &Harness,
    predicate: impl Fn(&AgentSessionEvent) -> bool + Send + Sync + 'static,
) -> tokio::sync::oneshot::Receiver<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Mutex::new(Some(tx));
    let listener: AgentSessionEventListener = Arc::new(move |event| {
        if predicate(&event)
            && let Some(tx) = tx.lock().take()
        {
            let _ = tx.send(());
        }
    });
    harness.session.subscribe(&listener);
    rx
}

/// TS `createWaitingHarness`: a session with a `wait` tool, prompted with "start" in the background.
pub struct Waiting {
    pub harness: Arc<Harness>,
    release: Arc<Notify>,
    pub run: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
    pub tool_start: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl Waiting {
    pub fn release(&self) {
        self.release.notify_one();
    }

    pub async fn wait_for_tool_start(&mut self) {
        self.tool_start.take().unwrap().await.unwrap();
    }

    /// Start the prompt (TS creates the promise inside the helper, after the responses are queued
    /// by the caller in the same tick).
    pub fn start(&mut self) {
        let session = self.harness.session.clone();
        self.run = Some(tokio::spawn(async move { session.prompt("start", None).await }));
    }

    pub async fn finish(&mut self) -> anyhow::Result<()> {
        self.run.take().unwrap().await.unwrap()
    }
}

pub async fn create_waiting_harness(mut options: HarnessOptions) -> Waiting {
    let release = Arc::new(Notify::new());
    let mut tools = vec![wait_tool(release.clone())];
    tools.extend(options.tools.take().unwrap_or_default());
    options.tools = Some(tools);
    let harness = Arc::new(create_harness(options).await.unwrap());
    let tool_start = wait_for_tool_start(&harness, "wait");
    Waiting {
        harness,
        release,
        run: None,
        tool_start: Some(tool_start),
    }
}

/// Yield to other tasks a few times (TS `await new Promise(r => setTimeout(r, 0))`).
pub async fn tick() {
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
}
