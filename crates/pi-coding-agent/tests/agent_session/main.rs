//! AgentSession suites (ports of `test/suite/agent-session-*.test.ts`, `test/agent-session-*.test.ts`,
//! `test/sdk-*.test.ts` and session regressions). One test binary keeps link time and disk use down.

#[path = "../support/mod.rs"]
mod support;

mod common;
mod queue;
mod retry_events;
mod compaction;
mod compaction_model_overrides;
mod boundaries;
mod extension_mutations;
mod runtime;
mod bash_persistence;
mod model_extension;
mod lax_content;
mod virtual_models;
mod concurrent;
