//! HDR detection and the SDR fallback.
//!
//! A 10-bit or PQ source is tone-mapped to 8-bit BT.709 before encode.
//! `RENCORD_HDR_WIRE=1` records that a later milestone may send 10-bit between
//! two Rencord clients. The wire format itself is not implemented.

use crate::frame::HdrFlags;
use crate::preset::PixelPath;

pub fn hdr_wire_enabled() -> bool {
    std::env::var("RENCORD_HDR_WIRE").ok().as_deref() == Some("1")
}

/// The Rencord-to-Rencord HDR wire format is intentionally absent.
pub fn hdr_wire_format() -> Result<&'static str, &'static str> {
    if hdr_wire_enabled() {
        Err("HDR wire is flagged on, but the 10-bit packet format is not in this milestone")
    } else {
        Err("HDR wire is off; frames are tone-mapped to 8-bit SDR")
    }
}

pub fn output_path(flags: HdrFlags) -> PixelPath {
    // The wire flag does not change the path. 10-bit between Rencord clients
    // is still tone-mapped until the later milestone defines the packet format.
    if flags.is_hdr() {
        PixelPath::ToneMapHdrToSdr
    } else {
        PixelPath::Sdr
    }
}

/// Map a 10-bit PQ code value to an 8-bit SDR luma sample.
///
/// This is a short curve for the fallback, not a full display transform.
pub fn tone_map_pq_to_sdr(code: u16) -> u8 {
    let unit = (code.min(1023) as f32) / 1023.0;
    let shaped = unit.powf(0.45);
    (shaped * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pq_maps_into_byte_range_and_the_wire_stays_closed() {
        assert_eq!(tone_map_pq_to_sdr(0), 0);
        assert_eq!(tone_map_pq_to_sdr(1023), 255);
        assert!(tone_map_pq_to_sdr(200) < tone_map_pq_to_sdr(800));
        assert!(hdr_wire_format().is_err());
        assert_eq!(
            output_path(HdrFlags {
                ten_bit: true,
                pq: true
            }),
            PixelPath::ToneMapHdrToSdr
        );
        assert_eq!(output_path(HdrFlags::default()), PixelPath::Sdr);
    }
}
