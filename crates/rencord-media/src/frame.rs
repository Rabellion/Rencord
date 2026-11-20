//! BGRA frames shared by capture, the worker, and the viewer.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct HdrFlags {
    /// The source reported 10-bit samples.
    pub ten_bit: bool,
    /// The source reported a PQ (SMPTE ST 2084) transfer.
    pub pq: bool,
}

impl HdrFlags {
    pub fn bits(self) -> u32 {
        u32::from(self.ten_bit) | (u32::from(self.pq) << 1)
    }

    pub fn from_bits(bits: u32) -> Self {
        Self {
            ten_bit: bits & 1 != 0,
            pq: bits & 2 != 0,
        }
    }

    pub fn is_hdr(self) -> bool {
        self.ten_bit || self.pq
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub sequence: u64,
    pub captured_at_us: u64,
    pub hdr: HdrFlags,
}

impl Frame {
    pub fn new(width: u32, height: u32, sequence: u64) -> Self {
        let pixels = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        Self {
            width,
            height,
            bgra: vec![0; pixels],
            sequence,
            captured_at_us: 0,
            hdr: HdrFlags::default(),
        }
    }

    pub fn pixel_bytes(&self) -> usize {
        (self.width as usize)
            .saturating_mul(self.height as usize)
            .saturating_mul(4)
    }
}

/// Uncompressed RGB PNG so the source picker can show a thumbnail without a
/// second image crate.
pub fn encode_png(bgra: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row_bytes = (width as usize) * 3;
    let mut raw = Vec::with_capacity((row_bytes + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0);
        for x in 0..width as usize {
            let i = (y * width as usize + x) * 4;
            let (b, g, r) = if i + 2 < bgra.len() {
                (bgra[i], bgra[i + 1], bgra[i + 2])
            } else {
                (0, 0, 0)
            };
            raw.extend_from_slice(&[r, g, b]);
        }
    }
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &store_deflate(&raw));
    write_chunk(&mut png, b"IEND", &[]);
    png
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_bytes = Vec::with_capacity(4 + data.len());
    crc_bytes.extend_from_slice(kind);
    crc_bytes.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_bytes).to_be_bytes());
}

fn store_deflate(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut rest = data;
    while !rest.is_empty() {
        let n = rest.len().min(65535);
        let last = n == rest.len();
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&(n as u16).to_le_bytes());
        out.extend_from_slice(&(!n as u16).to_le_bytes());
        out.extend_from_slice(&rest[..n]);
        rest = &rest[n..];
    }
    let sum = adler32(data);
    out.extend_from_slice(&sum.to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_starts_with_the_signature_and_holds_the_size() {
        let png = encode_png(&[10, 20, 30, 255, 1, 2, 3, 255], 2, 1);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(png.windows(4).any(|chunk| chunk == b"IHDR"));
        assert!(png.windows(4).any(|chunk| chunk == b"IEND"));
    }
}
