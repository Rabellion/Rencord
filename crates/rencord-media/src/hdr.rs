//! HDR detection and the SDR fallback.
//!
//! A 10-bit or PQ source is tone-mapped to 8-bit BT.709 before encode.
//! `planned_pixel_path` asks `rencord-hdr` what a Rencord audience could receive.
//! `output_path` does not use that answer, so the worker stays on 8-bit.

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

/// Ask `rencord-hdr` which Discord track this audience should get.
///
/// `output_path` stays the worker's actual choice and still tone-maps. This
/// function is the hook for a later change that would put 10-bit on the wire
/// only when send and receive are on, every viewer advertised HDR, and the
/// caller sets `sfu_proven`. That flag is not set by this crate.
pub fn planned_pixel_path(
    flags: HdrFlags,
    send: bool,
    receive: bool,
    sfu_proven: bool,
) -> PixelPath {
    use rencord_hdr::{
        AudienceMember, DiscordTrack, HdrCapability, HdrSession, HdrSignal, SourceColorimetry,
        ToneCurve,
    };
    if !flags.is_hdr() {
        return PixelPath::Sdr;
    }
    let mut session = HdrSession::from_settings(send, receive, ToneCurve::Bt2390);
    session.sfu_hdr_passthrough_proven = sfu_proven;
    if receive {
        session
            .audience
            .push(AudienceMember::Rencord(HdrCapability::advertise(
                true, false, 1000,
            )));
    } else {
        session.audience.push(AudienceMember::OfficialOrUnknown);
    }
    match session
        .plan(&SourceColorimetry::hdr10(1000.0))
        .discord_track
    {
        DiscordTrack::Hdr10 => PixelPath::RencordHdr10,
        DiscordTrack::Sdr => PixelPath::Sdr,
        DiscordTrack::ToneMappedSdr => PixelPath::ToneMapHdrToSdr,
    }
}

pub fn output_path(flags: HdrFlags) -> PixelPath {
    // The wire flag does not change this path. `planned_pixel_path` is the
    // separate decision, and this encode stays 8-bit.
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

    #[test]
    fn planned_path_follows_the_hdr_signal_and_stays_stricter_than_viewer_kind() {
        let hdr = HdrFlags {
            ten_bit: true,
            pq: true,
        };
        assert_eq!(
            planned_pixel_path(HdrFlags::default(), true, true, true),
            PixelPath::Sdr
        );
        assert_eq!(
            planned_pixel_path(hdr, true, true, false),
            PixelPath::ToneMapHdrToSdr
        );
        assert_eq!(
            planned_pixel_path(hdr, true, false, true),
            PixelPath::ToneMapHdrToSdr
        );
        assert_eq!(
            planned_pixel_path(hdr, false, true, true),
            PixelPath::ToneMapHdrToSdr
        );
        assert_eq!(
            planned_pixel_path(hdr, true, true, true),
            PixelPath::RencordHdr10
        );
        assert_eq!(output_path(hdr), PixelPath::ToneMapHdrToSdr);
    }
}
