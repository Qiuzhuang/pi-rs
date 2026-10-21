//! Helpers that turn async closures into the boxed hook types of [`crate::types`].

use std::future::Future;
use std::sync::Arc;

use pi_ai::Message;
use tokio_util::sync::CancellationToken;

use crate::agent::{AgentListener, AgentPrepareNextTurnFn, AgentPrepareNextTurnWithContextFn};
use crate::agent_loop::{AgentEventSink, ToolUpdateSink};
use crate::types::{
    AfterToolCallContext, AfterToolCallFn, AfterToolCallResult, AgentEvent, AgentLoopTurnUpdate, AgentMessage,
    AgentRequestUpdate, AgentToolResult, AgentTurnContext, AgentTurnDecision, BeforeToolCallContext,
    BeforeToolCallFn, BeforeToolCallResult, ConvertToLlmFn, FinishTurnFn, GetApiKeyFn, GetMessagesFn,
    PrepareNextTurnContext, PrepareNextTurnFn, PrepareRequestContext, PrepareRequestFn, TransformContextFn,
};

pub fn convert_to_llm<F, Fut>(f: F) -> ConvertToLlmFn
where
    F: Fn(Vec<AgentMessage>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Vec<Message>> + Send + 'static,
{
    Arc::new(move |messages| Box::pin(f(messages)))
}

pub fn transform_context<F, Fut>(f: F) -> TransformContextFn
where
    F: Fn(Vec<AgentMessage>, CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Vec<AgentMessage>> + Send + 'static,
{
    Arc::new(move |messages, signal| Box::pin(f(messages, signal)))
}

pub fn get_api_key<F, Fut>(f: F) -> GetApiKeyFn
where
    F: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<String>> + Send + 'static,
{
    Arc::new(move |provider| Box::pin(f(provider)))
}

pub fn finish_turn<F, Fut>(f: F) -> FinishTurnFn
where
    F: Fn(AgentTurnContext, CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<AgentTurnDecision>> + Send + 'static,
{
    Arc::new(move |turn, signal| Box::pin(f(turn, signal)))
}

pub fn prepare_request<F, Fut>(f: F) -> PrepareRequestFn
where
    F: Fn(PrepareRequestContext, CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<AgentRequestUpdate>> + Send + 'static,
{
    Arc::new(move |request, signal| Box::pin(f(request, signal)))
}

pub fn prepare_next_turn<F, Fut>(f: F) -> PrepareNextTurnFn
where
    F: Fn(PrepareNextTurnContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<AgentLoopTurnUpdate>> + Send + 'static,
{
    Arc::new(move |turn| Box::pin(f(turn)))
}

/// Agent-level legacy `prepare_next_turn` (signal only).
pub fn agent_prepare_next_turn<F, Fut>(f: F) -> AgentPrepareNextTurnFn
where
    F: Fn(CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<AgentLoopTurnUpdate>> + Send + 'static,
{
    Arc::new(move |signal| Box::pin(f(signal)))
}

/// Agent-level `prepare_next_turn_with_context`.
pub fn agent_prepare_next_turn_with_context<F, Fut>(f: F) -> AgentPrepareNextTurnWithContextFn
where
    F: Fn(PrepareNextTurnContext, CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<AgentLoopTurnUpdate>> + Send + 'static,
{
    Arc::new(move |turn, signal| Box::pin(f(turn, signal)))
}

pub fn get_messages<F, Fut>(f: F) -> GetMessagesFn
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Vec<AgentMessage>> + Send + 'static,
{
    Arc::new(move || Box::pin(f()))
}

pub fn before_tool_call<F, Fut>(f: F) -> BeforeToolCallFn
where
    F: Fn(BeforeToolCallContext, CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<Option<BeforeToolCallResult>>> + Send + 'static,
{
    Arc::new(move |context, signal| Box::pin(f(context, signal)))
}

pub fn after_tool_call<F, Fut>(f: F) -> AfterToolCallFn
where
    F: Fn(AfterToolCallContext, CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<Option<AfterToolCallResult>>> + Send + 'static,
{
    Arc::new(move |context, signal| Box::pin(f(context, signal)))
}

pub fn listener<F, Fut>(f: F) -> AgentListener
where
    F: Fn(AgentEvent, CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Arc::new(move |event, signal| Box::pin(f(event, signal)))
}

pub fn event_sink<F, Fut>(f: F) -> AgentEventSink
where
    F: Fn(AgentEvent) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Arc::new(move |event| Box::pin(f(event)))
}

pub fn tool_update_sink<F, Fut>(f: F) -> ToolUpdateSink
where
    F: Fn(AgentToolResult) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Arc::new(move |partial| Box::pin(f(partial)))
}
