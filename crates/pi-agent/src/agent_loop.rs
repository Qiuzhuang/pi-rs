//! Agent loop that works with `AgentMessage` throughout (port of `agent-loop.ts`).
//! Transforms to `Message[]` only at the LLM call boundary.

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use futures::FutureExt;
use futures::future::BoxFuture;
use pi_ai::utils::now_ms;
use pi_ai::utils::transcript::{
    ToolStateChanges, get_current_tools, get_tool_state_changes, transcript_from_messages,
};
use pi_ai::{
    AssistantMessage, AssistantMessageEvent, ClientIdentity, EventSender, EventStream, Message,
    StopReason, SystemContent, SystemMessage, ToolResultMessage, merge_client_identity,
    pi_attribution,
};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::AgentError;
use crate::stream_fn::get_default_stream_fn;
use crate::types::{
    AfterToolCallContext, AfterToolCallFn, AgentContext, AgentEvent, AgentLoopConfig, AgentMessage,
    AgentToolCall, AgentToolCallOutcome, AgentToolRef, AgentToolResult, AgentToolUpdateCallback,
    AgentTurnContext, AgentTurnDecision, BeforeToolCallContext, BeforeToolCallFn,
    PrepareRequestContext, StreamFn, ThinkingLevel, ToolExecutionMode, to_tool_declaration,
};

/// Receives loop events. Awaited before the loop continues.
pub type AgentEventSink = Arc<dyn Fn(AgentEvent) -> BoxFuture<'static, ()> + Send + Sync>;

/// Receives partial tool results.
pub type ToolUpdateSink = Arc<dyn Fn(AgentToolResult) -> BoxFuture<'static, ()> + Send + Sync>;

/// Build an [`AgentEventSink`] from a synchronous closure.
pub fn event_sink(f: impl Fn(AgentEvent) + Send + Sync + 'static) -> AgentEventSink {
    Arc::new(move |event| {
        f(event);
        Box::pin(async {})
    })
}

/// Request identity of agent loops: pi-ai's defaults with provider attribution to pi.
/// `AgentLoopConfig.options.identity` overrides it field by field.
pub fn default_agent_identity() -> ClientIdentity {
    ClientIdentity {
        attribution: Some(Some(pi_attribution())),
        ..Default::default()
    }
}

/// The event stream returned by [`agent_loop`] and [`agent_loop_continue`].
pub type AgentEventStream = EventStream<AgentEvent, Vec<AgentMessage>>;

fn create_agent_stream() -> (EventSender<AgentEvent, Vec<AgentMessage>>, AgentEventStream) {
    EventStream::new(
        |event: &AgentEvent| matches!(event, AgentEvent::AgentEnd { .. }),
        |event: &AgentEvent| match event {
            AgentEvent::AgentEnd { messages } => messages.clone(),
            _ => Vec::new(),
        },
    )
}

fn sender_sink(sender: EventSender<AgentEvent, Vec<AgentMessage>>) -> AgentEventSink {
    Arc::new(move |event| {
        sender.push(event);
        Box::pin(async {})
    })
}

/// Start an agent loop with new prompt messages. The prompts are added to the context and
/// events are emitted for them. Must be called within a tokio runtime.
pub fn agent_loop(
    prompts: Vec<AgentMessage>,
    context: AgentContext,
    config: AgentLoopConfig,
    signal: Option<CancellationToken>,
    stream_fn: Option<StreamFn>,
) -> AgentEventStream {
    let (sender, stream) = create_agent_stream();
    let sink = sender_sink(sender.clone());
    tokio::spawn(async move {
        if let Ok(messages) =
            run_agent_loop(prompts, context, config, sink, signal, stream_fn).await
        {
            sender.end(Some(messages));
        } else {
            sender.end(None);
        }
    });
    stream
}

fn validate_continue_context(context: &AgentContext) -> Result<(), AgentError> {
    match context.messages.last() {
        None => Err(AgentError::new("Cannot continue: no messages in context")),
        Some(AgentMessage::Assistant(_)) => Err(AgentError::new(
            "Cannot continue from message role: assistant",
        )),
        Some(_) => Ok(()),
    }
}

/// Continue an agent loop from the current context without adding a new message.
///
/// The last message in context must convert to a `user` or `toolResult` message via
/// `convert_to_llm`.
pub fn agent_loop_continue(
    context: AgentContext,
    config: AgentLoopConfig,
    signal: Option<CancellationToken>,
    stream_fn: Option<StreamFn>,
) -> Result<AgentEventStream, AgentError> {
    validate_continue_context(&context)?;
    let (sender, stream) = create_agent_stream();
    let sink = sender_sink(sender.clone());
    tokio::spawn(async move {
        if let Ok(messages) =
            run_agent_loop_continue(context, config, sink, signal, stream_fn).await
        {
            sender.end(Some(messages));
        } else {
            sender.end(None);
        }
    });
    Ok(stream)
}

/// Run an agent loop with new prompt messages, emitting events into `emit`.
/// Returns the messages added by this run.
pub async fn run_agent_loop(
    prompts: Vec<AgentMessage>,
    context: AgentContext,
    config: AgentLoopConfig,
    emit: AgentEventSink,
    signal: Option<CancellationToken>,
    stream_fn: Option<StreamFn>,
) -> anyhow::Result<Vec<AgentMessage>> {
    let initial_messages = declare_tool_changes(&context, prompts);
    let mut new_messages = initial_messages.clone();
    let mut current_context = context;
    current_context
        .messages
        .extend(initial_messages.iter().cloned());

    emit(AgentEvent::AgentStart).await;
    emit(AgentEvent::TurnStart).await;
    for message in initial_messages {
        emit(AgentEvent::MessageStart {
            message: message.clone(),
        })
        .await;
        emit(AgentEvent::MessageEnd { message }).await;
    }

    let stream_fn = match stream_fn {
        Some(stream_fn) => stream_fn,
        None => get_default_stream_fn()?,
    };
    run_loop(
        current_context,
        &mut new_messages,
        config,
        signal.unwrap_or_default(),
        &emit,
        &stream_fn,
    )
    .await?;
    Ok(new_messages)
}

/// Continue an agent loop from the current context, emitting events into `emit`.
pub async fn run_agent_loop_continue(
    context: AgentContext,
    config: AgentLoopConfig,
    emit: AgentEventSink,
    signal: Option<CancellationToken>,
    stream_fn: Option<StreamFn>,
) -> anyhow::Result<Vec<AgentMessage>> {
    validate_continue_context(&context)?;
    let mut new_messages = Vec::new();

    emit(AgentEvent::AgentStart).await;
    emit(AgentEvent::TurnStart).await;

    let stream_fn = match stream_fn {
        Some(stream_fn) => stream_fn,
        None => get_default_stream_fn()?,
    };
    run_loop(
        context,
        &mut new_messages,
        config,
        signal.unwrap_or_default(),
        &emit,
        &stream_fn,
    )
    .await?;
    Ok(new_messages)
}

async fn poll_messages(source: &Option<crate::types::GetMessagesFn>) -> Vec<AgentMessage> {
    match source {
        Some(source) => source().await,
        None => Vec::new(),
    }
}

fn thinking_level_of(config: &AgentLoopConfig) -> ThinkingLevel {
    config
        .options
        .reasoning
        .map(ThinkingLevel::from)
        .unwrap_or(ThinkingLevel::Off)
}

fn apply_thinking_level(config: &mut AgentLoopConfig, level: Option<ThinkingLevel>) {
    if let Some(level) = level {
        config.options.reasoning = level.thinking();
    }
}

/// Main loop logic shared by `run_agent_loop` and `run_agent_loop_continue`.
async fn run_loop(
    initial_context: AgentContext,
    new_messages: &mut Vec<AgentMessage>,
    initial_config: AgentLoopConfig,
    signal: CancellationToken,
    emit: &AgentEventSink,
    stream_fn: &StreamFn,
) -> anyhow::Result<()> {
    let mut current_context = initial_context;
    let mut config = initial_config;
    let mut last_completed_turn: Option<AgentTurnContext> = None;
    let mut explicit_continuation = false;
    // Check for steering messages at start (user may have typed while waiting).
    let mut pending_messages = poll_messages(&config.get_steering_messages).await;

    // Outer loop: continues when queued follow-up messages arrive after the agent would stop.
    loop {
        let mut has_more_tool_calls = true;

        // Inner loop: process tool calls and steering messages.
        while has_more_tool_calls || !pending_messages.is_empty() {
            let mut prepared_messages: Vec<AgentMessage> = Vec::new();
            if let Some(last_turn) = &last_completed_turn {
                let update = match &config.prepare_next_turn {
                    Some(prepare_next_turn) => prepare_next_turn(last_turn.clone()).await,
                    None => None,
                };
                if let Some(update) = update {
                    if let Some(context) = update.context {
                        current_context = context;
                    }
                    prepared_messages = update.messages.unwrap_or_default();
                    if let Some(model) = update.model {
                        config.model = model;
                    }
                    apply_thinking_level(&mut config, update.thinking_level);
                }
                // Preparation can be long-running (for example, compaction). Pick up steering
                // queued while it ran. Only poll again if the earlier poll returned nothing;
                // otherwise one-at-a-time mode would deliver two messages in this turn.
                if pending_messages.is_empty() {
                    pending_messages = poll_messages(&config.get_steering_messages).await;
                }
                emit(AgentEvent::TurnStart).await;
            }

            // Process prepared and queued messages before the next assistant response.
            let mut queued = prepared_messages;
            queued.append(&mut pending_messages);
            for message in declare_tool_changes(&current_context, queued) {
                emit(AgentEvent::MessageStart {
                    message: message.clone(),
                })
                .await;
                emit(AgentEvent::MessageEnd {
                    message: message.clone(),
                })
                .await;
                current_context.messages.push(message.clone());
                new_messages.push(message);
            }

            if let Some(prepare_request) = &config.prepare_request {
                let request = PrepareRequestContext {
                    context: current_context.clone(),
                    model: config.model.clone(),
                    thinking_level: thinking_level_of(&config),
                };
                if let Some(update) = prepare_request(request, signal.clone()).await? {
                    if let Some(context) = update.context {
                        current_context = context;
                    }
                    if let Some(model) = update.model {
                        config.model = model;
                    }
                    apply_thinking_level(&mut config, update.thinking_level);
                }
            }

            // Stream assistant response.
            let message =
                stream_assistant_response(&mut current_context, &config, &signal, emit, stream_fn)
                    .await?;
            new_messages.push(AgentMessage::Assistant(message.clone()));

            if matches!(message.stop_reason, StopReason::Error | StopReason::Aborted) {
                let turn = AgentTurnContext {
                    message: message.clone(),
                    tool_results: Vec::new(),
                    context: current_context.clone(),
                    new_messages: new_messages.clone(),
                };
                if let Some(finish_turn) = &config.finish_turn {
                    finish_turn(turn, signal.clone()).await;
                }
                emit(AgentEvent::TurnEnd {
                    message: AgentMessage::Assistant(message),
                    tool_results: Vec::new(),
                })
                .await;
                emit(AgentEvent::AgentEnd {
                    messages: new_messages.clone(),
                })
                .await;
                return Ok(());
            }

            // Check for tool calls.
            let tool_calls: Vec<AgentToolCall> = message.tool_calls().cloned().collect();
            let mut tool_results: Vec<ToolResultMessage> = Vec::new();
            has_more_tool_calls = false;
            if !tool_calls.is_empty() {
                // A "length" stop means the output was cut off by the token limit, so every
                // tool call in the message may carry truncated arguments. Fail them all
                // instead of executing potentially broken calls.
                let batch = if message.stop_reason == StopReason::Length {
                    fail_tool_calls_from_truncated_message(&tool_calls, emit).await
                } else {
                    execute_tool_calls(&current_context, &message, &config, &signal, emit).await
                };
                tool_results.extend(batch.messages);
                has_more_tool_calls = !batch.terminate;

                for result in &tool_results {
                    current_context
                        .messages
                        .push(AgentMessage::ToolResult(result.clone()));
                    new_messages.push(AgentMessage::ToolResult(result.clone()));
                }
            }

            let turn = AgentTurnContext {
                message: message.clone(),
                tool_results: tool_results.clone(),
                context: current_context.clone(),
                new_messages: new_messages.clone(),
            };
            let decision = match &config.finish_turn {
                Some(finish_turn) => finish_turn(turn.clone(), signal.clone()).await,
                None => None,
            };
            last_completed_turn = Some(turn);
            emit(AgentEvent::TurnEnd {
                message: AgentMessage::Assistant(message),
                tool_results,
            })
            .await;

            if decision == Some(AgentTurnDecision::End) {
                emit(AgentEvent::AgentEnd {
                    messages: new_messages.clone(),
                })
                .await;
                return Ok(());
            }

            explicit_continuation = decision == Some(AgentTurnDecision::Continue);
            pending_messages = poll_messages(&config.get_steering_messages).await;
            if has_more_tool_calls || !pending_messages.is_empty() {
                explicit_continuation = false;
            }
        }

        // Agent would stop here. Check for follow-up messages.
        let follow_up_messages = poll_messages(&config.get_follow_up_messages).await;
        if !follow_up_messages.is_empty() {
            // Set as pending so the inner loop processes them.
            explicit_continuation = false;
            pending_messages = follow_up_messages;
            continue;
        }

        // No natural request was selected, so fulfill the continuation decision with one
        // context-only turn.
        if explicit_continuation {
            explicit_continuation = false;
            continue;
        }

        // No more messages, exit.
        break;
    }

    emit(AgentEvent::AgentEnd {
        messages: new_messages.clone(),
    })
    .await;
    Ok(())
}

/// Declare tool loadout changes to the model.
///
/// `context.tools` is what the runtime can execute; the transcript's system messages declare
/// what the model may call. Before each request the difference becomes `toolsAdded` and
/// `toolsRemoved` on a system message. When a pending system message exists, its tool fields
/// are treated as intent and replaced with the delta between the committed transcript and the
/// executable set. Otherwise a new system message is inserted before the first non-system
/// pending message.
pub fn declare_tool_changes(
    context: &AgentContext,
    pending_messages: Vec<AgentMessage>,
) -> Vec<AgentMessage> {
    let system_index = pending_messages
        .iter()
        .rposition(|m| matches!(m, AgentMessage::System(_)));
    let pending: Option<SystemMessage> =
        system_index.and_then(|i| pending_messages[i].as_system().cloned());
    let no_changes = ToolStateChanges::default();
    let baseline: Vec<AgentMessage> = match (&pending, system_index) {
        (Some(pending), Some(index)) => {
            let mut baseline = pending_messages.clone();
            baseline[index] = AgentMessage::System(with_tool_changes(pending, &no_changes));
            baseline
        }
        _ => pending_messages.clone(),
    };
    let committed: Vec<Message> = context
        .messages
        .iter()
        .chain(baseline.iter())
        .filter_map(|m| m.as_system().cloned().map(Message::System))
        .collect();
    let executable: Vec<pi_ai::Tool> = context
        .tools()
        .iter()
        .map(|t| to_tool_declaration(t.as_ref()))
        .collect();
    let changes = get_tool_state_changes(&get_current_tools(&committed), &executable);
    let unchanged = changes.tools_added.is_empty() && changes.tools_removed.is_empty();

    if let (Some(pending), Some(index)) = (pending, system_index) {
        // Keep the caller's message when it already declares no tool changes.
        let declares_nothing = pending.tools_added.as_ref().is_none_or(Vec::is_empty)
            && pending.tools_removed.as_ref().is_none_or(Vec::is_empty);
        if unchanged && declares_nothing {
            return pending_messages;
        }
        let mut result = baseline;
        result[index] = AgentMessage::System(with_tool_changes(&pending, &changes));
        return result;
    }
    if unchanged {
        return pending_messages;
    }
    let update = with_tool_changes(
        &SystemMessage {
            content: SystemContent::Text(String::new()),
            timestamp: now_ms(),
            ..Default::default()
        },
        &changes,
    );
    let insert_index = pending_messages
        .iter()
        .position(|m| !matches!(m, AgentMessage::System(_)))
        .unwrap_or(pending_messages.len());
    let mut result = pending_messages;
    result.insert(insert_index, AgentMessage::System(update));
    result
}

/// Copy a system message with its tool fields replaced by `changes`; empty lists omit the field.
fn with_tool_changes(message: &SystemMessage, changes: &ToolStateChanges) -> SystemMessage {
    SystemMessage {
        tools_added: if changes.tools_added.is_empty() {
            None
        } else {
            Some(changes.tools_added.clone())
        },
        tools_removed: if changes.tools_removed.is_empty() {
            None
        } else {
            Some(changes.tools_removed.clone())
        },
        ..message.clone()
    }
}

/// Stream an assistant response from the LLM. This is where `AgentMessage`s are converted
/// to LLM messages.
async fn stream_assistant_response(
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    signal: &CancellationToken,
    emit: &AgentEventSink,
    stream_fn: &StreamFn,
) -> anyhow::Result<AssistantMessage> {
    // Apply context transform if configured (AgentMessage[] -> AgentMessage[]).
    let mut messages = context.messages.clone();
    if let Some(transform_context) = &config.transform_context {
        messages = transform_context(messages, signal.clone()).await;
    }

    // Convert to LLM-compatible messages (AgentMessage[] -> Message[]).
    let llm_messages = (config.convert_to_llm)(messages).await;
    let llm_context = transcript_from_messages(llm_messages);

    // Resolve API key (important for expiring tokens).
    let resolved_api_key = match &config.get_api_key {
        Some(get_api_key) => get_api_key(config.model.provider.clone()).await,
        None => None,
    }
    .filter(|key| !key.is_empty())
    .or_else(|| config.options.api_key.clone());

    let mut options = config.options.clone();
    options.api_key = resolved_api_key;
    options.identity = merge_client_identity(&[
        Some(&default_agent_identity()),
        config.options.identity.as_ref(),
    ]);
    options.signal = Some(signal.clone());

    let mut response = stream_fn(config.model.clone(), llm_context, options).await?;
    // Record the requested level, whichever stream function answered.
    let thinking_level = thinking_level_of(config);

    let mut partial_added = false;

    while let Some(event) = response.next_event().await {
        match event {
            AssistantMessageEvent::Start { partial } => {
                context
                    .messages
                    .push(AgentMessage::Assistant(partial.clone()));
                partial_added = true;
                emit(AgentEvent::MessageStart {
                    message: AgentMessage::Assistant(partial),
                })
                .await;
            }
            AssistantMessageEvent::Done { .. } | AssistantMessageEvent::Error { .. } => {
                let mut final_message = response.result().await;
                final_message.thinking_level = Some(thinking_level);
                return Ok(finish_response(context, final_message, partial_added, emit).await);
            }
            event => {
                if partial_added {
                    let partial = event.message().clone();
                    if let Some(last) = context.messages.last_mut() {
                        *last = AgentMessage::Assistant(partial.clone());
                    }
                    emit(AgentEvent::MessageUpdate {
                        message: AgentMessage::Assistant(partial),
                        assistant_message_event: Box::new(event),
                    })
                    .await;
                }
            }
        }
    }

    let mut final_message = response.result().await;
    final_message.thinking_level = Some(thinking_level);
    Ok(finish_response(context, final_message, partial_added, emit).await)
}

async fn finish_response(
    context: &mut AgentContext,
    final_message: AssistantMessage,
    partial_added: bool,
    emit: &AgentEventSink,
) -> AssistantMessage {
    if partial_added {
        if let Some(last) = context.messages.last_mut() {
            *last = AgentMessage::Assistant(final_message.clone());
        }
    } else {
        context
            .messages
            .push(AgentMessage::Assistant(final_message.clone()));
        emit(AgentEvent::MessageStart {
            message: AgentMessage::Assistant(final_message.clone()),
        })
        .await;
    }
    emit(AgentEvent::MessageEnd {
        message: AgentMessage::Assistant(final_message.clone()),
    })
    .await;
    final_message
}

struct ExecutedToolCallBatch {
    messages: Vec<ToolResultMessage>,
    terminate: bool,
}

/// Fail all tool calls from an assistant message truncated by the output token limit.
async fn fail_tool_calls_from_truncated_message(
    tool_calls: &[AgentToolCall],
    emit: &AgentEventSink,
) -> ExecutedToolCallBatch {
    let mut messages = Vec::new();
    for tool_call in tool_calls {
        emit(tool_execution_start(tool_call)).await;
        let finalized = AgentToolCallOutcome {
            tool_call: tool_call.clone(),
            result: AgentToolResult::error(format!(
                "Tool call \"{}\" was not executed: the response hit the output token limit, so its arguments may be truncated. Re-issue the tool call with complete arguments.",
                tool_call.name
            )),
            is_error: true,
            duration_ms: None,
        };
        emit_tool_execution_end(&finalized, emit).await;
        let tool_result_message = create_tool_result_message(&finalized);
        emit_tool_result_message(&tool_result_message, emit).await;
        messages.push(tool_result_message);
    }
    ExecutedToolCallBatch {
        messages,
        terminate: false,
    }
}

fn tool_execution_start(tool_call: &AgentToolCall) -> AgentEvent {
    AgentEvent::ToolExecutionStart {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        args: Value::Object(tool_call.arguments.clone()),
    }
}

/// The `before_tool_call` and `after_tool_call` hooks.
#[derive(Clone, Default)]
pub struct ToolCallHooks {
    pub before_tool_call: Option<BeforeToolCallFn>,
    pub after_tool_call: Option<AfterToolCallFn>,
}

impl ToolCallHooks {
    fn from_config(config: &AgentLoopConfig) -> Self {
        Self {
            before_tool_call: config.before_tool_call.clone(),
            after_tool_call: config.after_tool_call.clone(),
        }
    }
}

/// Execute tool calls from an assistant message.
async fn execute_tool_calls(
    current_context: &AgentContext,
    assistant_message: &AssistantMessage,
    config: &AgentLoopConfig,
    signal: &CancellationToken,
    emit: &AgentEventSink,
) -> ExecutedToolCallBatch {
    let tool_calls: Vec<AgentToolCall> = assistant_message.tool_calls().cloned().collect();
    let has_sequential_tool_call = tool_calls.iter().any(|tc| {
        current_context
            .tools()
            .iter()
            .find(|t| t.name() == tc.name)
            .is_some_and(|t| t.execution_mode() == Some(ToolExecutionMode::Sequential))
    });
    let hooks = ToolCallHooks::from_config(config);
    if config.tool_execution == Some(ToolExecutionMode::Sequential) || has_sequential_tool_call {
        execute_tool_calls_sequential(
            current_context,
            assistant_message,
            &tool_calls,
            &hooks,
            signal,
            emit,
        )
        .await
    } else {
        execute_tool_calls_parallel(
            current_context,
            assistant_message,
            &tool_calls,
            &hooks,
            signal,
            emit,
        )
        .await
    }
}

async fn execute_tool_calls_sequential(
    current_context: &AgentContext,
    assistant_message: &AssistantMessage,
    tool_calls: &[AgentToolCall],
    hooks: &ToolCallHooks,
    signal: &CancellationToken,
    emit: &AgentEventSink,
) -> ExecutedToolCallBatch {
    let mut finalized_calls = Vec::new();
    let mut messages = Vec::new();

    for tool_call in tool_calls {
        emit(tool_execution_start(tool_call)).await;

        let preparation = prepare_tool_call(
            current_context,
            assistant_message,
            tool_call,
            hooks,
            signal,
            current_context.tools(),
        )
        .await;
        let finalized = match preparation {
            Preparation::Immediate { result, is_error } => AgentToolCallOutcome {
                tool_call: tool_call.clone(),
                result,
                is_error,
                duration_ms: None,
            },
            Preparation::Prepared(prepared) => {
                let executed = execute_prepared_tool_call(
                    &prepared,
                    signal,
                    emit_tool_execution_update(tool_call, emit),
                )
                .await;
                finalize_executed_tool_call(
                    current_context,
                    assistant_message,
                    prepared,
                    executed,
                    hooks,
                    signal,
                )
                .await
            }
        };

        emit_tool_execution_end(&finalized, emit).await;
        let tool_result_message = create_tool_result_message(&finalized);
        emit_tool_result_message(&tool_result_message, emit).await;
        finalized_calls.push(finalized);
        messages.push(tool_result_message);

        if signal.is_cancelled() {
            break;
        }
    }

    ExecutedToolCallBatch {
        messages,
        terminate: should_terminate_tool_batch(&finalized_calls),
    }
}

enum FinalizedEntry<'a> {
    Done(Box<AgentToolCallOutcome>),
    Pending(Pin<Box<dyn Future<Output = AgentToolCallOutcome> + Send + 'a>>),
}

async fn execute_tool_calls_parallel(
    current_context: &AgentContext,
    assistant_message: &AssistantMessage,
    tool_calls: &[AgentToolCall],
    hooks: &ToolCallHooks,
    signal: &CancellationToken,
    emit: &AgentEventSink,
) -> ExecutedToolCallBatch {
    let mut entries: Vec<FinalizedEntry<'_>> = Vec::new();

    for tool_call in tool_calls {
        emit(tool_execution_start(tool_call)).await;

        let preparation = prepare_tool_call(
            current_context,
            assistant_message,
            tool_call,
            hooks,
            signal,
            current_context.tools(),
        )
        .await;
        match preparation {
            Preparation::Immediate { result, is_error } => {
                let finalized = AgentToolCallOutcome {
                    tool_call: tool_call.clone(),
                    result,
                    is_error,
                    duration_ms: None,
                };
                emit_tool_execution_end(&finalized, emit).await;
                entries.push(FinalizedEntry::Done(Box::new(finalized)));
                if signal.is_cancelled() {
                    break;
                }
                continue;
            }
            Preparation::Prepared(prepared) => {
                entries.push(FinalizedEntry::Pending(Box::pin(async move {
                    if signal.is_cancelled() {
                        let finalized = AgentToolCallOutcome {
                            tool_call: tool_call.clone(),
                            result: AgentToolResult::error("Operation aborted"),
                            is_error: true,
                            duration_ms: None,
                        };
                        emit_tool_execution_end(&finalized, emit).await;
                        return finalized;
                    }
                    let executed = execute_prepared_tool_call(
                        &prepared,
                        signal,
                        emit_tool_execution_update(tool_call, emit),
                    )
                    .await;
                    let finalized = finalize_executed_tool_call(
                        current_context,
                        assistant_message,
                        prepared,
                        executed,
                        hooks,
                        signal,
                    )
                    .await;
                    emit_tool_execution_end(&finalized, emit).await;
                    finalized
                })));
            }
        }
        if signal.is_cancelled() {
            break;
        }
    }

    let ordered_finalized_calls =
        futures::future::join_all(entries.into_iter().map(|entry| async move {
            match entry {
                FinalizedEntry::Done(finalized) => *finalized,
                FinalizedEntry::Pending(future) => future.await,
            }
        }))
        .await;

    let mut messages = Vec::new();
    for finalized in &ordered_finalized_calls {
        let tool_result_message = create_tool_result_message(finalized);
        emit_tool_result_message(&tool_result_message, emit).await;
        messages.push(tool_result_message);
    }

    ExecutedToolCallBatch {
        messages,
        terminate: should_terminate_tool_batch(&ordered_finalized_calls),
    }
}

struct PreparedToolCall {
    tool_call: AgentToolCall,
    tool: AgentToolRef,
    args: Value,
}

enum Preparation {
    Prepared(PreparedToolCall),
    Immediate {
        result: AgentToolResult,
        is_error: bool,
    },
}

struct ExecutedToolCallOutcome {
    result: AgentToolResult,
    is_error: bool,
    duration_ms: u64,
}

fn should_terminate_tool_batch(finalized_calls: &[AgentToolCallOutcome]) -> bool {
    !finalized_calls.is_empty()
        && finalized_calls
            .iter()
            .all(|f| f.result.terminate == Some(true))
}

fn prepare_tool_call_arguments(tool: &AgentToolRef, tool_call: &AgentToolCall) -> AgentToolCall {
    match tool.prepare_arguments(&tool_call.arguments) {
        Some(arguments) if arguments != tool_call.arguments => AgentToolCall {
            arguments,
            ..tool_call.clone()
        },
        _ => tool_call.clone(),
    }
}

fn immediate_error(message: impl Into<String>) -> Preparation {
    Preparation::Immediate {
        result: AgentToolResult::error(message),
        is_error: true,
    }
}

async fn prepare_tool_call(
    current_context: &AgentContext,
    assistant_message: &AssistantMessage,
    tool_call: &AgentToolCall,
    hooks: &ToolCallHooks,
    signal: &CancellationToken,
    tools: &[AgentToolRef],
) -> Preparation {
    let Some(tool) = tools.iter().find(|t| t.name() == tool_call.name) else {
        return immediate_error(format!("Tool {} not found", tool_call.name));
    };

    let prepared_tool_call = prepare_tool_call_arguments(tool, tool_call);
    let mut validated_args = match pi_ai::utils::validation::validate_tool_arguments(
        &tool.to_tool(),
        &prepared_tool_call,
    ) {
        Ok(args) => args,
        Err(error) => return immediate_error(error.0),
    };
    if let Some(before_tool_call) = &hooks.before_tool_call {
        let before_result = before_tool_call(
            BeforeToolCallContext {
                assistant_message: assistant_message.clone(),
                tool_call: tool_call.clone(),
                args: validated_args.clone(),
                context: current_context.clone(),
            },
            signal.clone(),
        )
        .await;
        let before_result = match before_result {
            Ok(result) => result,
            Err(error) => return immediate_error(error.to_string()),
        };
        if signal.is_cancelled() {
            return immediate_error("Operation aborted");
        }
        if let Some(before_result) = before_result {
            if before_result.block {
                let mut result = AgentToolResult::error(
                    before_result
                        .reason
                        .filter(|r| !r.is_empty())
                        .unwrap_or_else(|| "Tool execution was blocked".into()),
                );
                if before_result.terminate == Some(true) {
                    result.terminate = Some(true);
                }
                return Preparation::Immediate {
                    result,
                    is_error: true,
                };
            }
            if let Some(args) = before_result.args {
                validated_args = args;
            }
        }
    }
    if signal.is_cancelled() {
        return immediate_error("Operation aborted");
    }
    Preparation::Prepared(PreparedToolCall {
        tool_call: tool_call.clone(),
        tool: tool.clone(),
        args: validated_args,
    })
}

fn emit_tool_execution_update(tool_call: &AgentToolCall, emit: &AgentEventSink) -> ToolUpdateSink {
    let tool_call_id = tool_call.id.clone();
    let tool_name = tool_call.name.clone();
    let args = Value::Object(tool_call.arguments.clone());
    let emit = emit.clone();
    Arc::new(move |partial_result| {
        emit(AgentEvent::ToolExecutionUpdate {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            args: args.clone(),
            partial_result,
        })
    })
}

/// Options for [`run_tool_call`].
#[derive(Clone)]
pub struct RunToolCallOptions {
    /// Tools the call resolves against.
    pub tools: Vec<AgentToolRef>,
    /// Passed to the hooks as the message that issued the call.
    pub assistant_message: AssistantMessage,
    /// Passed to the hooks as the current agent context.
    pub context: AgentContext,
    pub before_tool_call: Option<BeforeToolCallFn>,
    pub after_tool_call: Option<AfterToolCallFn>,
    pub signal: Option<CancellationToken>,
    pub on_update: Option<ToolUpdateSink>,
}

impl RunToolCallOptions {
    pub fn new(
        tools: Vec<AgentToolRef>,
        assistant_message: AssistantMessage,
        context: AgentContext,
    ) -> Self {
        Self {
            tools,
            assistant_message,
            context,
            before_tool_call: None,
            after_tool_call: None,
            signal: None,
            on_update: None,
        }
    }
}

/// Run one tool call through the same steps as a model-issued call: argument preparation,
/// schema validation, `before_tool_call`, execution, and `after_tool_call`. Emits no events
/// and adds no messages.
///
/// Never fails for tool failures: unknown tools, validation errors, blocked calls, and
/// returned errors come back as `is_error: true`.
pub async fn run_tool_call(
    tool_call: AgentToolCall,
    options: RunToolCallOptions,
) -> AgentToolCallOutcome {
    let signal = options.signal.clone().unwrap_or_default();
    let hooks = ToolCallHooks {
        before_tool_call: options.before_tool_call.clone(),
        after_tool_call: options.after_tool_call.clone(),
    };
    let preparation = prepare_tool_call(
        &options.context,
        &options.assistant_message,
        &tool_call,
        &hooks,
        &signal,
        &options.tools,
    )
    .await;
    match preparation {
        Preparation::Immediate { result, is_error } => AgentToolCallOutcome {
            tool_call,
            result,
            is_error,
            duration_ms: None,
        },
        Preparation::Prepared(prepared) => {
            let on_update = options
                .on_update
                .clone()
                .unwrap_or_else(|| Arc::new(|_| Box::pin(async {})));
            let executed = execute_prepared_tool_call(&prepared, &signal, on_update).await;
            finalize_executed_tool_call(
                &options.context,
                &options.assistant_message,
                prepared,
                executed,
                &hooks,
                &signal,
            )
            .await
        }
    }
}

fn panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        message.to_string()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "Tool panicked".to_string()
    }
}

async fn execute_prepared_tool_call(
    prepared: &PreparedToolCall,
    signal: &CancellationToken,
    on_update: ToolUpdateSink,
) -> ExecutedToolCallOutcome {
    let (tx, mut rx) = mpsc::unbounded_channel::<Option<AgentToolResult>>();
    let accepting = Arc::new(AtomicBool::new(true));
    let callback: AgentToolUpdateCallback = {
        let tx = tx.clone();
        let accepting = accepting.clone();
        Arc::new(move |partial_result| {
            if accepting.load(Ordering::SeqCst) {
                let _ = tx.send(Some(partial_result));
            }
        })
    };

    let started_at = Instant::now();
    let execute = async {
        let outcome = AssertUnwindSafe(prepared.tool.execute(
            &prepared.tool_call.id,
            prepared.args.clone(),
            signal.clone(),
            callback,
        ))
        .catch_unwind()
        .await;
        let duration_ms = (started_at.elapsed().as_secs_f64() * 1000.0).round() as u64;
        accepting.store(false, Ordering::SeqCst);
        let _ = tx.send(None);
        (outcome, duration_ms)
    };
    let drain_updates = async {
        while let Some(Some(partial_result)) = rx.recv().await {
            on_update(partial_result).await;
        }
    };
    let ((outcome, duration_ms), ()) = futures::join!(execute, drain_updates);

    match outcome {
        Ok(Ok(result)) => {
            let is_error = result.is_error == Some(true);
            ExecutedToolCallOutcome {
                result,
                is_error,
                duration_ms,
            }
        }
        Ok(Err(error)) => ExecutedToolCallOutcome {
            result: AgentToolResult::error(error.to_string()),
            is_error: true,
            duration_ms,
        },
        Err(panic) => ExecutedToolCallOutcome {
            result: AgentToolResult::error(panic_message(panic)),
            is_error: true,
            duration_ms,
        },
    }
}

fn non_null(value: Option<Value>) -> Option<Value> {
    value.filter(|v| !v.is_null())
}

async fn finalize_executed_tool_call(
    current_context: &AgentContext,
    assistant_message: &AssistantMessage,
    prepared: PreparedToolCall,
    executed: ExecutedToolCallOutcome,
    hooks: &ToolCallHooks,
    signal: &CancellationToken,
) -> AgentToolCallOutcome {
    let mut result = executed.result;
    let mut is_error = executed.is_error;

    if let Some(after_tool_call) = &hooks.after_tool_call {
        let after_result = after_tool_call(
            AfterToolCallContext {
                assistant_message: assistant_message.clone(),
                tool_call: prepared.tool_call.clone(),
                args: prepared.args.clone(),
                result: result.clone(),
                is_error,
                context: current_context.clone(),
            },
            signal.clone(),
        )
        .await;
        match after_result {
            Ok(Some(after_result)) => {
                // Structured content not replaced along with the content may no longer match it.
                let structured_content = match non_null(after_result.structured_content) {
                    Some(structured) => Some(structured),
                    None if after_result.content.is_some() => None,
                    None => result.structured_content.take(),
                };
                if let Some(content) = after_result.content {
                    result.content = content;
                }
                if let Some(details) = non_null(after_result.details) {
                    result.details = Some(details);
                }
                if let Some(usage) = after_result.usage {
                    result.usage = Some(usage);
                }
                if let Some(terminate) = after_result.terminate {
                    result.terminate = Some(terminate);
                }
                result.structured_content = structured_content;
                if let Some(after_is_error) = after_result.is_error {
                    is_error = after_is_error;
                }
            }
            Ok(None) => {}
            Err(error) => {
                result = AgentToolResult::error(error.to_string());
                is_error = true;
            }
        }
    }

    AgentToolCallOutcome {
        tool_call: prepared.tool_call,
        result,
        is_error,
        duration_ms: Some(executed.duration_ms),
    }
}

async fn emit_tool_execution_end(finalized: &AgentToolCallOutcome, emit: &AgentEventSink) {
    emit(AgentEvent::ToolExecutionEnd {
        tool_call_id: finalized.tool_call.id.clone(),
        tool_name: finalized.tool_call.name.clone(),
        result: finalized.result.clone(),
        is_error: finalized.is_error,
        duration_ms: finalized.duration_ms,
    })
    .await;
}

fn create_tool_result_message(finalized: &AgentToolCallOutcome) -> ToolResultMessage {
    ToolResultMessage {
        tool_call_id: finalized.tool_call.id.clone(),
        tool_name: finalized.tool_call.name.clone(),
        content: finalized.result.content.clone(),
        details: finalized.result.details.clone(),
        usage: finalized.result.usage.clone(),
        nested_calls: None,
        is_error: finalized.is_error,
        timestamp: now_ms(),
        duration_ms: finalized.duration_ms,
    }
}

async fn emit_tool_result_message(tool_result_message: &ToolResultMessage, emit: &AgentEventSink) {
    emit(AgentEvent::MessageStart {
        message: AgentMessage::ToolResult(tool_result_message.clone()),
    })
    .await;
    emit(AgentEvent::MessageEnd {
        message: AgentMessage::ToolResult(tool_result_message.clone()),
    })
    .await;
}
