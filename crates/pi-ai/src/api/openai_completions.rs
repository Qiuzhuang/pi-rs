//! OpenAI Chat Completions API (port of `api/openai-completions.ts`).
use crate::models::ProviderStreams;
use crate::types::{AssistantMessageEventStream, Model, StreamOptions, TranscriptContext};
pub struct OpenAICompletionsApi;
impl ProviderStreams for OpenAICompletionsApi {
    fn stream(&self, model: &Model, _c: &TranscriptContext, _o: StreamOptions) -> AssistantMessageEventStream {
        super::lazy::error_stream(model, anyhow::anyhow!("wip"))
    }
    fn stream_simple(&self, model: &Model, _c: &TranscriptContext, _o: StreamOptions) -> AssistantMessageEventStream {
        super::lazy::error_stream(model, anyhow::anyhow!("wip"))
    }
}
