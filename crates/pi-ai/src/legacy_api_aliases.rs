//! Deprecated per-API stream aliases (port of `legacy-api-aliases.ts`).

use crate::api::registry::builtin_api;
use crate::types::{AssistantMessageEventStream, Model, StreamOptions, TranscriptContext};

macro_rules! legacy_alias {
    ($stream:ident, $simple:ident, $api:literal) => {
        #[deprecated(note = "Use the API implementation directly.")]
        pub fn $stream(model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
            builtin_api($api).stream(model, context, options)
        }
        #[deprecated(note = "Use the API implementation directly.")]
        pub fn $simple(model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
            builtin_api($api).stream_simple(model, context, options)
        }
    };
}

legacy_alias!(stream_anthropic, stream_simple_anthropic, "anthropic-messages");
legacy_alias!(stream_azure_openai_responses, stream_simple_azure_openai_responses, "azure-openai-responses");
legacy_alias!(stream_google, stream_simple_google, "google-generative-ai");
legacy_alias!(stream_google_vertex, stream_simple_google_vertex, "google-vertex");
legacy_alias!(stream_mistral, stream_simple_mistral, "mistral-conversations");
legacy_alias!(stream_openai_codex_responses, stream_simple_openai_codex_responses, "openai-codex-responses");
legacy_alias!(stream_openai_completions, stream_simple_openai_completions, "openai-completions");
legacy_alias!(stream_openai_responses, stream_simple_openai_responses, "openai-responses");
