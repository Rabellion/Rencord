//! Zip and 7z volumes under a byte cap, plus the note a recipient uses to join them.
//!
//! Each volume starts with a 64-byte `RNSPLT1` header so the parts can be
//! joined even when they arrive out of order. The bytes after the header are
//! a slice of one zip or 7z archive. Rencord strips the headers, checks the
//! archive hash, and extracts the original file.

use crate::{hex_encode, sha256, UploadError};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const HEADER_LEN: usize = 64;
const MAGIC: &[u8; 8] = b"RNSPLT1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFormat {
    Zip,
    SevenZ,
}

impl ArchiveFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::SevenZ => "7z",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Zip => Self::SevenZ,
            Self::SevenZ => Self::Zip,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Zip => 1,
            Self::SevenZ => 2,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Zip),
            2 => Some(Self::SevenZ),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitNote {
    pub name: String,
    pub format: ArchiveFormat,
    pub parts: u16,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitBundle {
    pub volumes: Vec<Vec<u8>>,
    pub filenames: Vec<String>,
    pub note: SplitNote,
    pub note_text: String,
}

pub fn split_bytes(
    name: &str,
    data: &[u8],
    limit: u64,
    format: ArchiveFormat,
) -> Result<SplitBundle, UploadError> {
    if limit < HEADER_LEN as u64 + 32 {
        return Err(UploadError::message(
            "the upload cap is smaller than one split volume",
        ));
    }
    let safe_name = sanitize_name(name);
    let archive = match format {
        ArchiveFormat::Zip => zip_bytes(&safe_name, data)?,
        ArchiveFormat::SevenZ => sevenz_bytes(&safe_name, data)?,
    };
    let digest = sha256(&archive);
    let payload_cap = (limit as usize) - HEADER_LEN;
    let count = archive.len().div_ceil(payload_cap).max(1);
    if count > u16::MAX as usize {
        return Err(UploadError::message("too many volumes"));
    }
    let mut volumes = Vec::with_capacity(count);
    for index in 0..count {
        let start = index * payload_cap;
        let end = (start + payload_cap).min(archive.len());
        let payload = &archive[start..end];
        let mut volume = header(
            format,
            index as u16,
            count as u16,
            payload.len() as u32,
            archive.len() as u64,
            digest,
        );
        volume.extend_from_slice(payload);
        if volume.len() as u64 > limit {
            return Err(UploadError::message("a volume exceeded the cap"));
        }
        volumes.push(volume);
    }
    let note = SplitNote {
        name: safe_name.clone(),
        format,
        parts: count as u16,
        sha256: hex_encode(&digest),
    };
    let filenames = (1..=count)
        .map(|index| format!("{safe_name}.{}.{index:03}", format.label()))
        .collect();
    let note_text = format_note(&note);
    Ok(SplitBundle {
        volumes,
        filenames,
        note,
        note_text,
    })
}

pub fn reassemble(volumes: &[Vec<u8>]) -> Result<Vec<u8>, UploadError> {
    if volumes.is_empty() {
        return Err(UploadError::message("no volumes"));
    }
    let mut parsed = Vec::with_capacity(volumes.len());
    for volume in volumes {
        parsed.push(read_header(volume)?);
    }
    parsed.sort_by_key(|item| item.index);
    let count = parsed[0].count;
    let format = parsed[0].format;
    let digest = parsed[0].sha256;
    let archive_len = parsed[0].archive_len;
    if parsed.len() != count as usize {
        return Err(UploadError::message(format!(
            "expected {count} volumes, got {}",
            parsed.len()
        )));
    }
    let mut archive = Vec::with_capacity(archive_len as usize);
    for (expected, item) in parsed.iter().enumerate() {
        if item.index != expected as u16 || item.count != count || item.format != format {
            return Err(UploadError::message("volume headers do not agree"));
        }
        if item.sha256 != digest {
            return Err(UploadError::message("volume hash does not match"));
        }
        archive.extend_from_slice(item.payload);
    }
    if sha256(&archive) != digest {
        return Err(UploadError::message("archive hash does not match"));
    }
    match format {
        ArchiveFormat::Zip => unzip_first(&archive),
        ArchiveFormat::SevenZ => unseven_first(&archive),
    }
}

struct ParsedVolume<'a> {
    format: ArchiveFormat,
    index: u16,
    count: u16,
    archive_len: u64,
    sha256: [u8; 32],
    payload: &'a [u8],
}

fn header(
    format: ArchiveFormat,
    index: u16,
    count: u16,
    payload_len: u32,
    archive_len: u64,
    digest: [u8; 32],
) -> Vec<u8> {
    let mut bytes = vec![0u8; HEADER_LEN];
    bytes[0..8].copy_from_slice(MAGIC);
    bytes[8] = 1;
    bytes[9] = format.code();
    bytes[10..12].copy_from_slice(&index.to_le_bytes());
    bytes[12..14].copy_from_slice(&count.to_le_bytes());
    bytes[14..18].copy_from_slice(&payload_len.to_le_bytes());
    bytes[18..26].copy_from_slice(&archive_len.to_le_bytes());
    bytes[26..58].copy_from_slice(&digest);
    bytes
}

fn read_header(volume: &[u8]) -> Result<ParsedVolume<'_>, UploadError> {
    if volume.len() < HEADER_LEN || &volume[0..8] != MAGIC || volume[8] != 1 {
        return Err(UploadError::message("not a Rencord split volume"));
    }
    let format = ArchiveFormat::from_code(volume[9])
        .ok_or_else(|| UploadError::message("unknown split format"))?;
    let index = u16::from_le_bytes(volume[10..12].try_into().unwrap());
    let count = u16::from_le_bytes(volume[12..14].try_into().unwrap());
    let payload_len = u32::from_le_bytes(volume[14..18].try_into().unwrap()) as usize;
    let archive_len = u64::from_le_bytes(volume[18..26].try_into().unwrap());
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&volume[26..58]);
    let payload = volume
        .get(HEADER_LEN..HEADER_LEN + payload_len)
        .ok_or_else(|| UploadError::message("volume payload is short"))?;
    Ok(ParsedVolume {
        format,
        index,
        count,
        archive_len,
        sha256: digest,
        payload,
    })
}

fn sanitize_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("file");
    let cleaned: String = base
        .chars()
        .map(|ch| if ch.is_control() { '_' } else { ch })
        .collect();
    if cleaned.is_empty() {
        "file".to_owned()
    } else {
        cleaned
    }
}

fn zip_bytes(name: &str, data: &[u8]) -> Result<Vec<u8>, UploadError> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    // Stored keeps the volume count predictable. Deflate of an already
    // compressed video often grows the archive past the cap.
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    writer.start_file(name, options)?;
    writer.write_all(data)?;
    let cursor = writer.finish()?;
    Ok(cursor.into_inner())
}

fn unzip_first(bytes: &[u8]) -> Result<Vec<u8>, UploadError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut file = archive.by_index(0)?;
    let mut out = Vec::new();
    file.read_to_end(&mut out)?;
    Ok(out)
}

fn sevenz_bytes(name: &str, data: &[u8]) -> Result<Vec<u8>, UploadError> {
    let mut writer = sevenz_rust2::ArchiveWriter::new(Cursor::new(Vec::new()))
        .map_err(|err| UploadError::message(format!("7z writer: {err}")))?;
    writer.set_content_methods(vec![sevenz_rust2::EncoderConfiguration::new(
        sevenz_rust2::EncoderMethod::COPY,
    )]);
    let mut entry = sevenz_rust2::ArchiveEntry::new_file(name);
    entry.size = data.len() as u64;
    writer
        .push_archive_entry(entry, Some(Cursor::new(data)))
        .map_err(|err| UploadError::message(format!("7z entry: {err}")))?;
    let cursor = writer
        .finish()
        .map_err(|err| UploadError::message(format!("7z finish: {err}")))?;
    Ok(cursor.into_inner())
}

fn unseven_first(bytes: &[u8]) -> Result<Vec<u8>, UploadError> {
    let mut reader = sevenz_rust2::ArchiveReader::new(
        Cursor::new(bytes.to_vec()),
        sevenz_rust2::Password::empty(),
    )
    .map_err(|err| UploadError::message(format!("7z reader: {err}")))?;
    let mut out = Vec::new();
    reader
        .for_each_entries(|_entry, reader| {
            reader.read_to_end(&mut out)?;
            Ok(true)
        })
        .map_err(|err| UploadError::message(format!("7z extract: {err}")))?;
    if out.is_empty() {
        return Err(UploadError::message("7z archive had no file"));
    }
    Ok(out)
}

pub fn format_note(note: &SplitNote) -> String {
    format!(
        "rencord-split v1\nname: {}\narchive: {}\nparts: {}\nsha256: {}\nJoin the attachments in order ({}.001 through .{:03}). Rencord reassembles this in one click. Other clients can concatenate the payloads after the 64-byte RNSPLT1 header and extract the archive.",
        note.name,
        note.format.label(),
        note.parts,
        note.sha256,
        note.format.label(),
        note.parts
    )
}

pub fn parse_note(text: &str) -> Option<SplitNote> {
    if !text.lines().any(|line| line.trim() == "rencord-split v1") {
        return None;
    }
    let mut name = None;
    let mut format = None;
    let mut parts = None;
    let mut sha256 = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("name: ") {
            name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("archive: ") {
            format = match value.trim() {
                "zip" => Some(ArchiveFormat::Zip),
                "7z" => Some(ArchiveFormat::SevenZ),
                _ => None,
            };
        } else if let Some(value) = line.strip_prefix("parts: ") {
            parts = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("sha256: ") {
            sha256 = Some(value.trim().to_owned());
        }
    }
    Some(SplitNote {
        name: name?,
        format: format?,
        parts: parts?,
        sha256: sha256?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(format: ArchiveFormat) {
        let mut data = vec![0u8; 50_000];
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = (index.wrapping_mul(17) % 251) as u8;
        }
        let limit = 16 * 1024;
        let bundle = split_bytes("lecture.mp4", &data, limit, format).unwrap();
        assert!(bundle.volumes.len() >= 2, "{}", bundle.volumes.len());
        for volume in &bundle.volumes {
            assert!(volume.len() as u64 <= limit, "{}", volume.len());
        }
        let parsed = parse_note(&bundle.note_text).unwrap();
        assert_eq!(parsed, bundle.note);
        let mut shuffled = bundle.volumes.clone();
        shuffled.reverse();
        let restored = reassemble(&shuffled).unwrap();
        assert_eq!(restored, data);
        let mut broken = bundle.volumes.clone();
        let last = broken.len() - 1;
        let end = broken[last].len() - 1;
        broken[last][end] ^= 0xff;
        assert!(reassemble(&broken).is_err());
    }

    #[test]
    fn zip_split_round_trip() {
        round_trip(ArchiveFormat::Zip);
    }

    #[test]
    fn sevenz_split_round_trip() {
        round_trip(ArchiveFormat::SevenZ);
    }
}
