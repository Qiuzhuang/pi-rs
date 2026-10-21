//! Port of `packages/coding-agent/src/modes/rpc/rpc-mode.ts`.

use crate::core::agent_session_runtime::AgentSessionRuntime;
use std::sync::Arc;

pub use crate::modes::rpc::rpc_types::RpcCommand;
pub use crate::modes::rpc::rpc_types::RpcExtensionUIRequest;
pub use crate::modes::rpc::rpc_types::RpcExtensionUIResponse;
pub use crate::modes::rpc::rpc_types::RpcResponse;
pub use crate::modes::rpc::rpc_types::RpcSessionState;

/// Run in RPC mode.
/// Listens for JSON commands on stdin, outputs events and responses on stdout.
pub async fn run_rpc_mode(runtime_host: Arc<AgentSessionRuntime>) -> anyhow::Result<()> {
    todo!("port modes/rpc/rpc-mode.ts:runRpcMode")
}
