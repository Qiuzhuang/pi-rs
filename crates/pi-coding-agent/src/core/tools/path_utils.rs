//! Port of `packages/coding-agent/src/core/tools/path-utils.ts`.

pub async fn path_exists(file_path: &str) -> anyhow::Result<bool> {
    todo!("port core/tools/path-utils.ts:pathExists")
}

pub fn expand_path(file_path: &str) -> String {
    todo!("port core/tools/path-utils.ts:expandPath")
}

/// Resolve a path relative to the given cwd.
/// Handles ~ expansion and absolute paths.
pub fn resolve_to_cwd(file_path: &str, cwd: &str) -> String {
    todo!("port core/tools/path-utils.ts:resolveToCwd")
}

pub fn resolve_read_path(file_path: &str, cwd: &str) -> String {
    todo!("port core/tools/path-utils.ts:resolveReadPath")
}

pub async fn resolve_read_path_async(file_path: &str, cwd: &str) -> anyhow::Result<String> {
    todo!("port core/tools/path-utils.ts:resolveReadPathAsync")
}
