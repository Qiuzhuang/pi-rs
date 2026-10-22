//! pi-agent: general-purpose agent loop with tool execution, steering and follow-up queues.
//!
//! Rust port of the `@earendil-works/pi-agent-core` TypeScript package.

pub mod agent;
pub mod agent_loop;
pub mod error;
pub mod hooks;
pub mod proxy;
pub mod stream_fn;
pub mod transcript;
pub mod types;

pub use agent::*;
pub use agent_loop::*;
pub use error::AgentError;
pub use proxy::*;
pub use stream_fn::{
    compat_stream_simple_fn, get_default_stream_fn, is_compat_stream_simple_fn, models_stream_fn,
    set_default_stream_fn,
};
pub use types::*;
