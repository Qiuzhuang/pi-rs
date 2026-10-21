//! Port of `packages/coding-agent/src/utils/wsl.ts`.

use serde_json::Value;

/// Windows Subsystem for Linux, where Windows executables are reachable through interop.
pub fn is_wsl(env: Option<Value>) -> bool {
    todo!("port utils/wsl.ts:isWSL")
}
