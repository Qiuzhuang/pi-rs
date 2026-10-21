//! pi-ai: unified LLM API with model catalog, streaming and provider implementations.
//!
//! Rust port of the `@earendil-works/pi-ai` TypeScript package.

pub mod providers;
pub mod serde_util;
pub mod types;
pub mod utils;

pub use types::*;
pub use utils::diagnostics::*;
pub use utils::event_stream::*;
pub use utils::identity::*;
pub use utils::version::PI_AI_VERSION;
