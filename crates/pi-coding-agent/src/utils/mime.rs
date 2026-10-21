//! Port of `packages/coding-agent/src/utils/mime.ts`.

const IMAGE_TYPE_SNIFF_BYTES: usize = 4100;
const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

pub fn detect_supported_image_mime_type(buffer: &[u8]) -> Option<String> {
    if starts_with(buffer, &[0xff, 0xd8, 0xff]) {
        return if buffer.get(3) == Some(&0xf7) { None } else { Some("image/jpeg".to_string()) };
    }
    if starts_with(buffer, &PNG_SIGNATURE) {
        return if is_png(buffer) && !is_animated_png(buffer) {
            Some("image/png".to_string())
        } else {
            None
        };
    }
    if starts_with_ascii(buffer, 0, "GIF87a") || starts_with_ascii(buffer, 0, "GIF89a") {
        return Some("image/gif".to_string());
    }
    if starts_with_ascii(buffer, 0, "RIFF") && starts_with_ascii(buffer, 8, "WEBP") {
        return Some("image/webp".to_string());
    }
    if starts_with_ascii(buffer, 0, "BM") && is_bmp(buffer) {
        return Some("image/bmp".to_string());
    }
    None
}

pub async fn detect_supported_image_mime_type_from_file(
    file_path: &str,
) -> anyhow::Result<Option<String>> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(file_path)
        .await
        .map_err(|err| crate::core::tools::node_fs::node_fs_error(&err, "open", Some(file_path)))?;
    let mut buffer = vec![0u8; IMAGE_TYPE_SNIFF_BYTES];
    let mut bytes_read = 0;
    while bytes_read < IMAGE_TYPE_SNIFF_BYTES {
        match file.read(&mut buffer[bytes_read..]).await {
            Ok(0) => break,
            Ok(n) => bytes_read += n,
            Err(err) => {
                return Err(crate::core::tools::node_fs::node_fs_error(&err, "read", None).into());
            }
        }
    }
    Ok(detect_supported_image_mime_type(&buffer[..bytes_read]))
}

fn is_png(buffer: &[u8]) -> bool {
    buffer.len() >= 16 && read_u32_be(buffer, PNG_SIGNATURE.len()) == 13 && starts_with_ascii(buffer, 12, "IHDR")
}

fn is_animated_png(buffer: &[u8]) -> bool {
    let mut offset = PNG_SIGNATURE.len();
    while offset + 8 <= buffer.len() {
        let chunk_length = read_u32_be(buffer, offset) as usize;
        let chunk_type_offset = offset + 4;
        if starts_with_ascii(buffer, chunk_type_offset, "acTL") {
            return true;
        }
        if starts_with_ascii(buffer, chunk_type_offset, "IDAT") {
            return false;
        }
        let next_offset = offset + 8 + chunk_length + 4;
        if next_offset <= offset || next_offset > buffer.len() {
            return false;
        }
        offset = next_offset;
    }
    false
}

fn is_bmp(buffer: &[u8]) -> bool {
    if buffer.len() < 26 {
        return false;
    }
    let declared_file_size = read_u32_le(buffer, 2);
    let pixel_data_offset = read_u32_le(buffer, 10);
    let dib_header_size = read_u32_le(buffer, 14);
    if declared_file_size != 0 && declared_file_size < 26 {
        return false;
    }
    if (pixel_data_offset as u64) < 14 + dib_header_size as u64 {
        return false;
    }
    if declared_file_size != 0 && pixel_data_offset >= declared_file_size {
        return false;
    }
    let (color_planes, bits_per_pixel) = if dib_header_size == 12 {
        (read_u16_le(buffer, 22), read_u16_le(buffer, 24))
    } else if (40..=124).contains(&dib_header_size) {
        if buffer.len() < 30 {
            return false;
        }
        (read_u16_le(buffer, 26), read_u16_le(buffer, 28))
    } else {
        return false;
    };
    color_planes == 1 && [1, 4, 8, 16, 24, 32].contains(&bits_per_pixel)
}

fn byte(buffer: &[u8], offset: usize) -> u32 {
    buffer.get(offset).copied().unwrap_or(0) as u32
}

fn read_u16_le(buffer: &[u8], offset: usize) -> u32 {
    byte(buffer, offset) + (byte(buffer, offset + 1) << 8)
}

fn read_u32_be(buffer: &[u8], offset: usize) -> u32 {
    (byte(buffer, offset) << 24) + (byte(buffer, offset + 1) << 16) + (byte(buffer, offset + 2) << 8) + byte(buffer, offset + 3)
}

fn read_u32_le(buffer: &[u8], offset: usize) -> u32 {
    byte(buffer, offset) + (byte(buffer, offset + 1) << 8) + (byte(buffer, offset + 2) << 16) + (byte(buffer, offset + 3) << 24)
}

fn starts_with(buffer: &[u8], bytes: &[u8]) -> bool {
    buffer.len() >= bytes.len() && &buffer[..bytes.len()] == bytes
}

fn starts_with_ascii(buffer: &[u8], offset: usize, text: &str) -> bool {
    buffer.len() >= offset + text.len() && &buffer[offset..offset + text.len()] == text.as_bytes()
}
