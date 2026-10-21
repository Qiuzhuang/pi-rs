//! The Bedrock stream module (port of `bedrock-provider.ts` and `setBedrockProviderModule` from
//! `api/bedrock-converse-stream.lazy.ts`).

use std::sync::Arc;

use crate::api::bedrock_converse_stream::BedrockConverseStreamApi;
use crate::api::registry::register_builtin_api_implementation;
use crate::models::ProviderStreams;

/// The Bedrock Converse stream implementation (`bedrockProviderModule`).
pub fn bedrock_provider_module() -> Arc<dyn ProviderStreams> {
    Arc::new(BedrockConverseStreamApi)
}

/// Override the implementation behind the built-in `bedrock-converse-stream` API.
pub fn set_bedrock_provider_module(module: Arc<dyn ProviderStreams>) {
    register_builtin_api_implementation("bedrock-converse-stream", module);
}
