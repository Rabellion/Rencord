//! HDR10 static metadata as integers, HEVC prefix SEI, and AV1 metadata OBUs.
//!
//! The integer layout is the one in ITU-T H.265 mastering-display colour volume
//! (payload type 137), content light level (payload type 144), and the AV1
//! metadata OBU types 1 and 2. Chromaticity is in units of 0.00002. Luminance
//! is in units of 0.0001 cd/m².

use crate::color::{Chromaticities, ColorPrimaries};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hdr10StaticMetadata {
    pub display: Chromaticities,
    pub max_luminance: f64,
    pub min_luminance: f64,
    pub max_cll: u16,
    pub max_fall: u16,
}

impl Hdr10StaticMetadata {
    /// BT.2020 master, 1 000 cd/m² peak, 0.005 cd/m² black, MaxCLL 1000, MaxFALL 400.
    pub fn bt2020_1000() -> Self {
        Self {
            display: Chromaticities::bt2020(),
            max_luminance: 1000.0,
            min_luminance: 0.005,
            max_cll: 1000,
            max_fall: 400,
        }
    }

    pub fn for_primaries(primaries: ColorPrimaries, peak_nits: f64, min_nits: f64) -> Self {
        Self {
            display: Chromaticities::for_primaries(primaries),
            max_luminance: peak_nits,
            min_luminance: min_nits,
            max_cll: peak_nits.round().clamp(0.0, 65535.0) as u16,
            max_fall: (peak_nits * 0.4).round().clamp(0.0, 65535.0) as u16,
        }
    }

    /// 24-byte mastering-display payload. Order is R, G, B, white, max, min.
    pub fn mastering_payload(&self) -> [u8; 24] {
        let mut payload = [0u8; 24];
        let mut offset = 0;
        for primary in [self.display.red, self.display.green, self.display.blue] {
            write_u16(&mut payload, &mut offset, chroma_code(primary.x));
            write_u16(&mut payload, &mut offset, chroma_code(primary.y));
        }
        write_u16(&mut payload, &mut offset, chroma_code(self.display.white.x));
        write_u16(&mut payload, &mut offset, chroma_code(self.display.white.y));
        write_u32(
            &mut payload,
            &mut offset,
            luminance_code(self.max_luminance),
        );
        write_u32(
            &mut payload,
            &mut offset,
            luminance_code(self.min_luminance),
        );
        payload
    }

    pub fn content_light_payload(&self) -> [u8; 4] {
        let mut payload = [0u8; 4];
        payload[0..2].copy_from_slice(&self.max_cll.to_be_bytes());
        payload[2..4].copy_from_slice(&self.max_fall.to_be_bytes());
        payload
    }
}

pub fn chroma_code(value: f64) -> u16 {
    (value * 50_000.0).round().clamp(0.0, 50_000.0) as u16
}

pub fn luminance_code(nits: f64) -> u32 {
    (nits.max(0.0) * 10_000.0)
        .round()
        .clamp(0.0, u32::MAX as f64) as u32
}

pub fn chroma_from_code(code: u16) -> f64 {
    f64::from(code) / 50_000.0
}

pub fn nits_from_code(code: u32) -> f64 {
    f64::from(code) / 10_000.0
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeiMessage {
    pub payload_type: u32,
    pub payload: Vec<u8>,
}

pub fn mastering_sei(metadata: &Hdr10StaticMetadata) -> SeiMessage {
    SeiMessage {
        payload_type: 137,
        payload: metadata.mastering_payload().to_vec(),
    }
}

pub fn content_light_sei(metadata: &Hdr10StaticMetadata) -> SeiMessage {
    SeiMessage {
        payload_type: 144,
        payload: metadata.content_light_payload().to_vec(),
    }
}

/// HEVC prefix SEI NAL in Annex B, with emulation prevention and a trailing bit.
pub fn hevc_prefix_sei(messages: &[SeiMessage]) -> Vec<u8> {
    let mut rbsp = Vec::new();
    for message in messages {
        push_ff(&mut rbsp, message.payload_type as usize);
        push_ff(&mut rbsp, message.payload.len());
        rbsp.extend_from_slice(&message.payload);
    }
    rbsp.push(0x80);
    let mut nal = vec![0x00, 0x00, 0x00, 0x01, 0x4E, 0x01];
    nal.extend(rbsp_to_ebsp(&rbsp));
    nal
}

pub fn hevc_hdr10_sei(metadata: &Hdr10StaticMetadata) -> Vec<u8> {
    hevc_prefix_sei(&[mastering_sei(metadata), content_light_sei(metadata)])
}

pub fn rbsp_to_ebsp(rbsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rbsp.len() + 8);
    let mut zeros = 0u8;
    for &byte in rbsp {
        if zeros >= 2 && byte <= 0x03 {
            out.push(0x03);
            zeros = 0;
        }
        out.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
    out
}

pub fn ebsp_to_rbsp(ebsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ebsp.len());
    let mut zeros = 0u8;
    let mut index = 0;
    while index < ebsp.len() {
        if zeros >= 2 && ebsp[index] == 0x03 {
            zeros = 0;
            index += 1;
            continue;
        }
        let byte = ebsp[index];
        out.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        index += 1;
    }
    out
}

pub fn parse_annex_b_sei(bytes: &[u8]) -> Vec<SeiMessage> {
    let mut messages = Vec::new();
    for nal in split_annex_b(bytes) {
        if nal.len() < 3 || nal_unit_type(nal[0]) != 39 {
            continue;
        }
        let rbsp = ebsp_to_rbsp(&nal[2..]);
        messages.extend(parse_sei_rbsp(&rbsp));
    }
    messages
}

pub fn nal_unit_type(first: u8) -> u8 {
    (first >> 1) & 0x3f
}

pub fn split_annex_b(bytes: &[u8]) -> Vec<&[u8]> {
    let mut nalus = Vec::new();
    let mut start = None;
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == 0 && bytes[index + 1] == 0 && bytes[index + 2] == 1 {
            if let Some(from) = start {
                let mut end = index;
                if end > from && bytes[end - 1] == 0 {
                    end -= 1;
                }
                if end > from {
                    nalus.push(&bytes[from..end]);
                }
            }
            start = Some(index + 3);
            index += 3;
            continue;
        }
        index += 1;
    }
    if let Some(from) = start {
        if from < bytes.len() {
            nalus.push(&bytes[from..]);
        }
    }
    nalus
}

fn parse_sei_rbsp(rbsp: &[u8]) -> Vec<SeiMessage> {
    let mut messages = Vec::new();
    let mut index = 0;
    while index + 1 < rbsp.len() {
        if rbsp[index] == 0x80 {
            break;
        }
        let Some(payload_type) = read_ff(rbsp, &mut index) else {
            break;
        };
        let Some(size) = read_ff(rbsp, &mut index) else {
            break;
        };
        if index + size > rbsp.len() {
            break;
        }
        messages.push(SeiMessage {
            payload_type: payload_type as u32,
            payload: rbsp[index..index + size].to_vec(),
        });
        index += size;
    }
    messages
}

fn push_ff(out: &mut Vec<u8>, mut value: usize) {
    while value >= 255 {
        out.push(0xff);
        value -= 255;
    }
    out.push(value as u8);
}

fn read_ff(data: &[u8], index: &mut usize) -> Option<usize> {
    let mut value = 0usize;
    loop {
        if *index >= data.len() {
            return None;
        }
        let byte = data[*index] as usize;
        *index += 1;
        value += byte;
        if byte != 255 {
            return Some(value);
        }
    }
}

pub fn parse_mastering_payload(payload: &[u8]) -> Option<Hdr10StaticMetadata> {
    if payload.len() != 24 {
        return None;
    }
    let u16_at = |offset: usize| u16::from_be_bytes([payload[offset], payload[offset + 1]]);
    let u32_at = |offset: usize| {
        u32::from_be_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ])
    };
    let xy = |offset: usize| crate::color::Xy {
        x: chroma_from_code(u16_at(offset)),
        y: chroma_from_code(u16_at(offset + 2)),
    };
    let metadata = Hdr10StaticMetadata {
        display: Chromaticities {
            red: xy(0),
            green: xy(4),
            blue: xy(8),
            white: xy(12),
        },
        max_luminance: nits_from_code(u32_at(16)),
        min_luminance: nits_from_code(u32_at(20)),
        max_cll: 0,
        max_fall: 0,
    };
    Some(metadata)
}

pub fn parse_content_light_payload(payload: &[u8]) -> Option<(u16, u16)> {
    if payload.len() != 4 {
        return None;
    }
    Some((
        u16::from_be_bytes([payload[0], payload[1]]),
        u16::from_be_bytes([payload[2], payload[3]]),
    ))
}

pub fn metadata_from_sei(messages: &[SeiMessage]) -> Option<Hdr10StaticMetadata> {
    let mut metadata = None;
    for message in messages {
        if message.payload_type == 137 {
            metadata = parse_mastering_payload(&message.payload);
        }
    }
    let mut metadata = metadata?;
    for message in messages {
        if message.payload_type == 144 {
            if let Some((max_cll, max_fall)) = parse_content_light_payload(&message.payload) {
                metadata.max_cll = max_cll;
                metadata.max_fall = max_fall;
            }
        }
    }
    Some(metadata)
}

fn leb128(mut value: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
    out
}

fn obu(obu_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut size = leb128(payload.len() as u32);
    let header = (obu_type & 0x0f) << 3 | 0x02;
    let mut out = Vec::with_capacity(1 + size.len() + payload.len());
    out.push(header);
    out.append(&mut size);
    out.extend_from_slice(payload);
    out
}

/// AV1 metadata OBUs for HDR CLL (type 1) and MDCV (type 2), each with a size field.
pub fn av1_hdr_metadata_obus(metadata: &Hdr10StaticMetadata) -> Vec<u8> {
    let mut cll = vec![1u8];
    cll.extend_from_slice(&metadata.content_light_payload());
    let mut mdcv = vec![2u8];
    mdcv.extend_from_slice(&metadata.mastering_payload());
    let mut out = obu(5, &cll);
    out.extend(obu(5, &mdcv));
    out
}

fn write_u16(out: &mut [u8], offset: &mut usize, value: u16) {
    out[*offset..*offset + 2].copy_from_slice(&value.to_be_bytes());
    *offset += 2;
}

fn write_u32(out: &mut [u8], offset: &mut usize, value: u32) {
    out[*offset..*offset + 4].copy_from_slice(&value.to_be_bytes());
    *offset += 4;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt2020_mastering_integers_are_the_spec_units() {
        let payload = Hdr10StaticMetadata::bt2020_1000().mastering_payload();
        let codes = [35400u16, 14600, 8500, 39850, 6550, 2300, 15635, 16450];
        for (index, code) in codes.iter().enumerate() {
            let offset = index * 2;
            assert_eq!(
                u16::from_be_bytes([payload[offset], payload[offset + 1]]),
                *code
            );
        }
        assert_eq!(
            u32::from_be_bytes(payload[16..20].try_into().unwrap()),
            10_000_000
        );
        assert_eq!(u32::from_be_bytes(payload[20..24].try_into().unwrap()), 50);
        let light = Hdr10StaticMetadata::bt2020_1000().content_light_payload();
        assert_eq!(&light, &[0x03, 0xE8, 0x01, 0x90]);
    }

    #[test]
    fn hevc_sei_roundtrips_and_inserts_emulation_prevention_for_zero_black() {
        let mut metadata = Hdr10StaticMetadata::bt2020_1000();
        metadata.min_luminance = 0.0;
        let nal = hevc_hdr10_sei(&metadata);
        assert_eq!(&nal[0..6], &[0x00, 0x00, 0x00, 0x01, 0x4E, 0x01]);
        assert!(
            nal.windows(3).any(|window| window == [0x00, 0x00, 0x03]),
            "zero luminance must be escaped"
        );
        let parsed = metadata_from_sei(&parse_annex_b_sei(&nal)).unwrap();
        assert_eq!(parsed.max_cll, 1000);
        assert_eq!(parsed.max_fall, 400);
        assert_eq!(parsed.min_luminance, 0.0);
        assert!((parsed.max_luminance - 1000.0).abs() < 1e-6);
        assert!((parsed.display.red.x - 0.708).abs() < 1e-4);
        assert!((parsed.display.white.y - 0.329).abs() < 1e-4);
    }

    #[test]
    fn av1_cll_obu_bytes() {
        let bytes = av1_hdr_metadata_obus(&Hdr10StaticMetadata::bt2020_1000());
        assert_eq!(&bytes[0..7], &[0x2A, 0x05, 0x01, 0x03, 0xE8, 0x01, 0x90]);
        assert_eq!(bytes[7], 0x2A);
        assert_eq!(bytes[8], 25);
        assert_eq!(bytes[9], 2);
        assert_eq!(bytes.len(), 34);
    }
}
