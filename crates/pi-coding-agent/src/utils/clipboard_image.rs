//! Port of `packages/coding-agent/src/utils/clipboard-image.ts`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

pub fn is_wayland_session(env: Option<Value>) -> bool {
    todo!("port utils/clipboard-image.ts:isWaylandSession")
}

pub fn extension_for_image_mime_type(mime_type: &str) -> Option<String> {
    todo!("port utils/clipboard-image.ts:extensionForImageMimeType")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadClipboardImageOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<Value>,
}

pub async fn read_clipboard_image(
    options: Option<&ReadClipboardImageOptions>,
) -> anyhow::Result<Option<ClipboardImage>> {
    todo!("port utils/clipboard-image.ts:readClipboardImage")
}
