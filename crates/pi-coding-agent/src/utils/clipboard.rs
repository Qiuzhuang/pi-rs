//! Port of `packages/coding-agent/src/utils/clipboard.ts`.

/// Read plain text from the system clipboard.
pub async fn read_clipboard_text() -> anyhow::Result<Option<String>> {
    todo!("port utils/clipboard.ts:readClipboardText")
}

/// Read file paths, such as Finder file copies, from the native clipboard.
pub async fn read_clipboard_file_paths() -> anyhow::Result<Option<Vec<String>>> {
    todo!("port utils/clipboard.ts:readClipboardFilePaths")
}

pub async fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    todo!("port utils/clipboard.ts:copyToClipboard")
}
