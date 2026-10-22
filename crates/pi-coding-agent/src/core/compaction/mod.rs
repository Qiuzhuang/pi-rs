//! Port of `packages/coding-agent/src/core/compaction/index.ts`.

pub mod branch_summarization;
#[allow(clippy::module_inception)] // mirrors the TS file layout (`compaction/compaction.ts`)
pub mod compaction;
pub mod utils;

pub use crate::core::compaction::branch_summarization::*;
pub use crate::core::compaction::compaction::*;
pub use crate::core::compaction::utils::*;
