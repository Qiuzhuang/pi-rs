//! Port of `packages/coding-agent/src/core/tools/file-mutation-queue.ts`.

use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;

/// Serialize file mutation operations targeting the same file.
/// Operations for different files still run in parallel.
pub async fn with_file_mutation_queue(
    file_path: &str,
    r#fn: Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<Value>> + Send + Sync>,
) -> anyhow::Result<Value> {
    todo!("port core/tools/file-mutation-queue.ts:withFileMutationQueue")
}
