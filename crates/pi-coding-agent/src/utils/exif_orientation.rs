//! Port of `packages/coding-agent/src/utils/exif-orientation.ts`.

use crate::utils::photon::{Photon, PhotonImage};

fn read_orientation_from_tiff(bytes: &[u8], tiff_start: usize) -> u16 {
    if tiff_start + 8 > bytes.len() {
        return 1;
    }
    let byte_order = ((bytes[tiff_start] as u16) << 8) | bytes[tiff_start + 1] as u16;
    let le = byte_order == 0x4949;
    let byte = |pos: usize| bytes.get(pos).copied().unwrap_or(0) as u32;
    let read16 = |pos: usize| -> u32 {
        if le {
            byte(pos) | (byte(pos + 1) << 8)
        } else {
            (byte(pos) << 8) | byte(pos + 1)
        }
    };
    let read32 = |pos: usize| -> u32 {
        if le {
            byte(pos) | (byte(pos + 1) << 8) | (byte(pos + 2) << 16) | (byte(pos + 3) << 24)
        } else {
            (byte(pos) << 24) | (byte(pos + 1) << 16) | (byte(pos + 2) << 8) | byte(pos + 3)
        }
    };

    let ifd_offset = read32(tiff_start + 4) as usize;
    let ifd_start = tiff_start + ifd_offset;
    if ifd_start + 2 > bytes.len() {
        return 1;
    }
    let entry_count = read16(ifd_start) as usize;
    for i in 0..entry_count {
        let entry_pos = ifd_start + 2 + i * 12;
        if entry_pos + 12 > bytes.len() {
            return 1;
        }
        if read16(entry_pos) == 0x0112 {
            let value = read16(entry_pos + 8) as u16;
            return if (1..=8).contains(&value) { value } else { 1 };
        }
    }
    1
}

fn has_exif_header(bytes: &[u8], offset: usize) -> bool {
    bytes.get(offset..offset + 6) == Some(&[0x45, 0x78, 0x69, 0x66, 0x00, 0x00][..])
}

fn find_jpeg_tiff_offset(bytes: &[u8]) -> Option<usize> {
    let mut offset = 2usize;
    while offset + 1 < bytes.len() {
        if bytes[offset] != 0xff {
            return None;
        }
        let marker = bytes[offset + 1];
        if marker == 0xff {
            offset += 1;
            continue;
        }
        if marker == 0xe1 {
            if offset + 4 >= bytes.len() {
                return None;
            }
            let segment_start = offset + 4;
            if segment_start + 6 > bytes.len() {
                return None;
            }
            if has_exif_header(bytes, segment_start) {
                return Some(segment_start + 6);
            }
        }
        if offset + 4 > bytes.len() {
            return None;
        }
        let length = ((bytes[offset + 2] as usize) << 8) | bytes[offset + 3] as usize;
        offset += 2 + length;
    }
    None
}

fn find_webp_tiff_offset(bytes: &[u8]) -> Option<usize> {
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_size = (bytes[offset + 4] as u32
            | ((bytes[offset + 5] as u32) << 8)
            | ((bytes[offset + 6] as u32) << 16)
            | ((bytes[offset + 7] as u32) << 24)) as usize;
        let data_start = offset + 8;
        if chunk_id == b"EXIF" {
            if data_start + chunk_size > bytes.len() {
                return None;
            }
            // Some WebP files have "Exif\0\0" prefix before the TIFF header.
            return Some(if chunk_size >= 6 && has_exif_header(bytes, data_start) {
                data_start + 6
            } else {
                data_start
            });
        }
        // RIFF chunks are padded to even size.
        offset = data_start + chunk_size + (chunk_size % 2);
    }
    None
}

/// EXIF orientation (1-8) of JPEG and WebP data; 1 when absent.
pub fn get_exif_orientation(bytes: &[u8]) -> u16 {
    let tiff_offset = if bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] == 0xd8 {
        find_jpeg_tiff_offset(bytes)
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        find_webp_tiff_offset(bytes)
    } else {
        None
    };
    match tiff_offset {
        Some(offset) => read_orientation_from_tiff(bytes, offset),
        None => 1,
    }
}

pub type DstIndexFn = fn(u32, u32, u32, u32) -> u32;

fn rotate90(image: &PhotonImage, dst_index: DstIndexFn) -> PhotonImage {
    let w = image.get_width();
    let h = image.get_height();
    let src = image.get_raw_pixels();
    let mut dst = vec![0u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let src_idx = ((y * w + x) * 4) as usize;
            let dst_idx = (dst_index(x, y, w, h) * 4) as usize;
            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
        }
    }
    PhotonImage::new(dst, h, w).expect("rotated buffer size")
}

/// Apply the EXIF orientation of `original_bytes` to a decoded image. Flips mutate in place;
/// rotations return a new image.
pub fn apply_exif_orientation(photon: &Photon, mut image: PhotonImage, original_bytes: &[u8]) -> PhotonImage {
    let orientation = get_exif_orientation(original_bytes);
    match orientation {
        2 => {
            photon.fliph(&mut image);
            image
        }
        3 => {
            photon.fliph(&mut image);
            photon.flipv(&mut image);
            image
        }
        4 => {
            photon.flipv(&mut image);
            image
        }
        5 => {
            let mut rotated = rotate90(&image, |x, y, _w, h| x * h + (h - 1 - y));
            photon.fliph(&mut rotated);
            rotated
        }
        6 => rotate90(&image, |x, y, _w, h| x * h + (h - 1 - y)),
        7 => {
            let mut rotated = rotate90(&image, |x, y, w, h| (w - 1 - x) * h + y);
            photon.fliph(&mut rotated);
            rotated
        }
        8 => rotate90(&image, |x, y, w, h| (w - 1 - x) * h + y),
        _ => image,
    }
}
