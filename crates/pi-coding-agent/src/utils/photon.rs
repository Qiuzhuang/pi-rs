//! Port of `packages/coding-agent/src/utils/photon.ts`.

use serde_json::Value;

pub type ReadFileSync = Value;

/// Load the photon module asynchronously.
/// Returns cached module on subsequent calls.
pub async fn load_photon() -> anyhow::Result<Option<Value>> {
    todo!("port utils/photon.ts:loadPhoton")
}
