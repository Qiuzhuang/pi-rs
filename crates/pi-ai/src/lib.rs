//! pi-ai: unified LLM API with model catalog, streaming and provider implementations.
//!
//! Rust port of the `@earendil-works/pi-ai` TypeScript package.

pub mod api;
pub mod auth;
pub mod models;
pub mod models_store;
pub mod providers;
pub mod serde_util;
pub mod session_resources;
pub mod types;
pub mod utils;

pub use models::*;
pub use models_store::*;
pub use session_resources::*;
pub use types::*;
pub use utils::diagnostics::*;
pub use utils::event_stream::*;
pub use utils::identity::*;
pub use utils::json_parse::{parse_streaming_json, parse_streaming_json_object, parse_tool_call_arguments};
pub use utils::overflow::{is_context_overflow, is_recoverable_length};
pub use utils::retry::*;
pub use utils::text::*;
pub use utils::transcript::*;
pub use utils::uuid::uuidv7;
pub use utils::validation::{validate_tool_arguments, validate_tool_call};
pub use utils::version::PI_AI_VERSION;
