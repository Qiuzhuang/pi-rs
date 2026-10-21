//! Port of `packages/coding-agent/src/utils/mime.ts`.

pub fn detect_supported_image_mime_type(buffer: &[u8]) -> Option<String> {
    todo!("port utils/mime.ts:detectSupportedImageMimeType")
}

pub async fn detect_supported_image_mime_type_from_file(
    file_path: &str,
) -> anyhow::Result<Option<String>> {
    todo!("port utils/mime.ts:detectSupportedImageMimeTypeFromFile")
}
