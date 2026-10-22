//! Port of `packages/coding-agent/src/utils/zip.ts`.

use std::io::Write;

use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ZipEntryData {
    String(String),
    List(Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZipEntry {
    pub name: String,
    pub data: ZipEntryData,
}

fn dos_date_time<T: Datelike + Timelike>(date: &T) -> (u16, u16) {
    let time = (date.hour() << 11) | (date.minute() << 5) | (date.second() >> 1);
    let year = (date.year().max(1980) - 1980) as u32;
    let day = (year << 9) | (date.month() << 5) | date.day();
    (time as u16, day as u16)
}

/// Create the small, classic ZIP archives used by bug reports.
fn create_zip_archive(entries: &[ZipEntry]) -> anyhow::Result<Vec<u8>> {
    let mut files: Vec<u8> = Vec::new();
    let mut directory: Vec<u8> = Vec::new();
    let (time, day) = dos_date_time(&Local::now());
    let mut offset: u32 = 0;

    for entry in entries {
        let name = entry.name.as_bytes();
        let data: &[u8] = match &entry.data {
            ZipEntryData::String(s) => s.as_bytes(),
            ZipEntryData::List(bytes) => bytes,
        };
        let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data)?;
        let compressed = encoder.finish()?;
        let checksum = crc32fast::hash(data);

        let mut local = Vec::with_capacity(30);
        local.extend_from_slice(&0x04034b50u32.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&0x0800u16.to_le_bytes());
        local.extend_from_slice(&8u16.to_le_bytes());
        local.extend_from_slice(&time.to_le_bytes());
        local.extend_from_slice(&day.to_le_bytes());
        local.extend_from_slice(&checksum.to_le_bytes());
        local.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        local.extend_from_slice(&(data.len() as u32).to_le_bytes());
        local.extend_from_slice(&(name.len() as u16).to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());

        let mut central = Vec::with_capacity(46);
        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&8u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&day.to_le_bytes());
        central.extend_from_slice(&checksum.to_le_bytes());
        central.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        // extra length, comment length, disk number, internal attrs, external attrs
        central.extend_from_slice(&[0u8; 12]);
        central.extend_from_slice(&offset.to_le_bytes());

        let entry_len = (local.len() + name.len() + compressed.len()) as u32;
        files.extend_from_slice(&local);
        files.extend_from_slice(name);
        files.extend_from_slice(&compressed);
        directory.extend_from_slice(&central);
        directory.extend_from_slice(name);
        offset += entry_len;
    }

    let mut end = Vec::with_capacity(22);
    end.extend_from_slice(&0x06054b50u32.to_le_bytes());
    end.extend_from_slice(&[0u8; 4]);
    end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    end.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    end.extend_from_slice(&offset.to_le_bytes());
    end.extend_from_slice(&0u16.to_le_bytes());

    let mut archive = files;
    archive.extend_from_slice(&directory);
    archive.extend_from_slice(&end);
    Ok(archive)
}

pub async fn write_zip_archive(file_path: &str, entries: &[ZipEntry]) -> anyhow::Result<()> {
    let archive = create_zip_archive(entries)?;
    tokio::fs::write(file_path, archive).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn creates_a_readable_archive() {
        let archive = create_zip_archive(&[
            ZipEntry { name: "a.txt".into(), data: ZipEntryData::String("hello hello hello".into()) },
            ZipEntry { name: "dir/b.bin".into(), data: ZipEntryData::List(vec![0, 1, 2, 3]) },
        ])
        .unwrap();
        assert_eq!(&archive[..4], &0x04034b50u32.to_le_bytes());
        // Decode the first entry by hand.
        let name_len = u16::from_le_bytes([archive[26], archive[27]]) as usize;
        let compressed_len = u32::from_le_bytes(archive[18..22].try_into().unwrap()) as usize;
        let start = 30 + name_len;
        assert_eq!(&archive[30..start], b"a.txt");
        let mut decoder = flate2::read::DeflateDecoder::new(&archive[start..start + compressed_len]);
        let mut out = String::new();
        decoder.read_to_string(&mut out).unwrap();
        assert_eq!(out, "hello hello hello");
        let checksum = u32::from_le_bytes(archive[14..18].try_into().unwrap());
        assert_eq!(checksum, crc32fast::hash(b"hello hello hello"));
        let end = &archive[archive.len() - 22..];
        assert_eq!(&end[..4], &0x06054b50u32.to_le_bytes());
        assert_eq!(u16::from_le_bytes([end[10], end[11]]), 2);
    }
}
