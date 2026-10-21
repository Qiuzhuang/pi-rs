//! Port of `packages/coding-agent/src/modes/rpc/jsonl.ts`.

use serde_json::Value;
use std::sync::Arc;

/// Serialize a single strict JSONL record.
///
/// Framing is LF-only. Payload strings may contain other Unicode separators such as
/// U+2028 and U+2029. Clients must split records on `\n` only.
pub fn serialize_json_line(value: Value) -> String {
    todo!("port modes/rpc/jsonl.ts:serializeJsonLine")
}

/// Attach an LF-only JSONL reader to a stream.
///
/// This intentionally does not use Node readline. Readline splits on additional
/// Unicode separators that are valid inside JSON strings and therefore does not
/// implement strict JSONL framing.
pub fn attach_jsonl_line_reader(
    stream: Value,
    on_line: Arc<dyn Fn(String) + Send + Sync>,
) -> Arc<dyn Fn() + Send + Sync> {
    todo!("port modes/rpc/jsonl.ts:attachJsonlLineReader")
}
