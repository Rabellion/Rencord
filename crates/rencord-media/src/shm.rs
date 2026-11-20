//! One shared-memory frame slot.
//!
//! The worker writes pixels, then publishes the sequence. The UI copies the
//! slot only when the sequence is even and unchanged across the copy, so a
//! torn frame is discarded instead of shown.

use crate::frame::{Frame, HdrFlags};
use memmap2::MmapMut;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const HEADER: usize = 64;
const MAGIC: u32 = 0x5245_4e43;

#[derive(Debug)]
pub struct FrameMap {
    path: PathBuf,
    map: MmapMut,
    capacity: usize,
}

impl FrameMap {
    pub fn create(path: impl Into<PathBuf>, max_pixels: usize) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let capacity = HEADER + max_pixels.saturating_mul(4);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        file.set_len(capacity as u64)?;
        let mut map = unsafe { MmapMut::map_mut(&file)? };
        map[..4].copy_from_slice(&MAGIC.to_le_bytes());
        Ok(Self {
            path,
            map,
            capacity,
        })
    }

    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let capacity = file.metadata()?.len() as usize;
        let map = unsafe { MmapMut::map_mut(&file)? };
        if map.len() < HEADER || u32::from_le_bytes(map[..4].try_into().unwrap_or([0; 4])) != MAGIC
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "frame map magic mismatch",
            ));
        }
        Ok(Self {
            path,
            map,
            capacity,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write_frame(&mut self, frame: &Frame) -> Result<(), String> {
        let pixels = frame.pixel_bytes();
        if HEADER + pixels > self.capacity {
            return Err(format!(
                "frame {}x{} does not fit the shared map",
                frame.width, frame.height
            ));
        }
        let seq = sequence_ptr(&mut self.map);
        // Odd means a write is in progress.
        let published = unsafe { &*seq }.load(Ordering::Acquire);
        unsafe { &*seq }.store(published | 1, Ordering::Release);
        write_u32(&mut self.map, 4, frame.width);
        write_u32(&mut self.map, 8, frame.height);
        write_u32(&mut self.map, 12, frame.width.saturating_mul(4));
        write_u64(&mut self.map, 24, frame.captured_at_us);
        write_u32(&mut self.map, 32, frame.hdr.bits());
        self.map[HEADER..HEADER + pixels].copy_from_slice(&frame.bgra[..pixels]);
        unsafe { &*seq }.store(frame.sequence << 1, Ordering::Release);
        Ok(())
    }

    pub fn read_frame(&self) -> Option<Frame> {
        let seq = sequence_ptr_const(&self.map);
        let before = unsafe { &*seq }.load(Ordering::Acquire);
        if before & 1 == 1 || before == 0 {
            return None;
        }
        let width = read_u32(&self.map, 4);
        let height = read_u32(&self.map, 8);
        let pixels = (width as usize).saturating_mul(height as usize) * 4;
        if HEADER + pixels > self.map.len() {
            return None;
        }
        let mut bgra = vec![0u8; pixels];
        bgra.copy_from_slice(&self.map[HEADER..HEADER + pixels]);
        let captured_at_us = read_u64(&self.map, 24);
        let hdr = HdrFlags::from_bits(read_u32(&self.map, 32));
        let after = unsafe { &*seq }.load(Ordering::Acquire);
        if before != after {
            return None;
        }
        Some(Frame {
            width,
            height,
            bgra,
            sequence: before >> 1,
            captured_at_us,
            hdr,
        })
    }
}

fn sequence_ptr(map: &mut MmapMut) -> *const AtomicU64 {
    unsafe { map.as_mut_ptr().add(16) as *const AtomicU64 }
}

fn sequence_ptr_const(map: &MmapMut) -> *const AtomicU64 {
    unsafe { map.as_ptr().add(16) as *const AtomicU64 }
}

fn write_u32(map: &mut MmapMut, offset: usize, value: u32) {
    map[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(map: &mut MmapMut, offset: usize, value: u64) {
    map[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn read_u32(map: &MmapMut, offset: usize) -> u32 {
    u32::from_le_bytes(map[offset..offset + 4].try_into().unwrap_or([0; 4]))
}

fn read_u64(map: &MmapMut, offset: usize) -> u64 {
    u64::from_le_bytes(map[offset..offset + 8].try_into().unwrap_or([0; 8]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_published_frame_roundtrips() {
        let dir = std::env::temp_dir().join(format!("rencord-shm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("frame");
        let mut map = FrameMap::create(&path, 8).unwrap();
        let mut frame = Frame::new(2, 2, 4);
        frame.bgra = vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255];
        frame.captured_at_us = 42;
        frame.hdr.pq = true;
        map.write_frame(&frame).unwrap();
        let read = FrameMap::open(&path).unwrap().read_frame().unwrap();
        assert_eq!(read.bgra, frame.bgra);
        assert_eq!(read.sequence, 4);
        assert!(read.hdr.pq);
        let _ = fs::remove_dir_all(&dir);
    }
}
