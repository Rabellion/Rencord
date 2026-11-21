//! Rencord-to-Rencord HDR capability signalling.
//!
//! Official clients do not advertise this and must keep working. The bytes are
//! an RTP header extension and an HEVC user-data-unregistered SEI. Both are
//! specified to be ignored when the receiver does not know them. This crate
//! never emits a Discord gateway payload and never adds an `extmap` line to
//! Discord's select-protocol SDP. Discord's servers may still strip the SEI
//! or the extension. That has not been tested against Discord.

use crate::color::SourceColorimetry;
use crate::metadata::SeiMessage;
use crate::tonemap::ToneCurve;

/// UUID of the user-data SEI. Allocated by this repository. Not an IANA registration.
pub const CAPABILITY_UUID: [u8; 16] = [
    0xc2, 0xf6, 0xa1, 0xe4, 0x7b, 0x3d, 0x4e, 0x91, 0x8a, 0x55, 0x0d, 0x6e, 0x9f, 0x2b, 0x14, 0xc8,
];

pub const RTP_EXTENSION_URI: &str = "https://rencord.app/hdr/1";
/// Suggested one-byte id. Real use has to negotiate it. It is not offered in Discord SDP.
pub const RTP_EXTENSION_ID: u8 = 12;

const LIMIT_OFFICIAL: &str = "Official Discord clients have no HDR video path.";
const LIMIT_STRIP: &str = "Discord may strip SEI, unknown RTP header extensions, and unknown JSON keys. That has not been tested against Discord's servers.";
const LIMIT_SEND_OFF: &str = "HDR send is off, so the Discord track is tone-mapped SDR.";
const LIMIT_UNKNOWN: &str =
    "An empty audience is treated as official clients. The Discord track stays SDR.";
const LIMIT_MIXED: &str = "One Go Live track cannot be HDR for some viewers and SDR for others. A mixed audience stays on SDR.";
const LIMIT_UNPROVEN: &str = "10-bit on the Discord video track is withheld until an SFU passthrough test succeeds. That test has not been run.";
const LIMIT_SIDE: &str = "A side-channel access unit can be built for Rencord peers. This crate does not send it as a Discord opcode.";
const LIMIT_TRACK: &str = "The Discord track would be HDR10 only because every viewer advertised Rencord HDR and the passthrough flag was set. The flag defaults to false.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HdrCapability {
    pub receive: bool,
    pub send: bool,
    pub pq: bool,
    pub hlg: bool,
    pub hevc_main10: bool,
    pub av1_10bit: bool,
    pub max_bit_depth: u8,
    pub display_peak_nits: u16,
}

impl HdrCapability {
    pub fn advertise(receive: bool, send: bool, display_peak_nits: u16) -> Self {
        Self {
            receive,
            send,
            pq: true,
            hlg: true,
            hevc_main10: true,
            av1_10bit: true,
            max_bit_depth: 10,
            display_peak_nits,
        }
    }

    pub fn wants_hdr10(self) -> bool {
        self.receive && self.max_bit_depth >= 10 && (self.pq || self.hevc_main10 || self.av1_10bit)
    }

    pub fn payload(&self) -> [u8; 8] {
        let mut flags = 0u8;
        if self.receive {
            flags |= 1 << 0;
        }
        if self.send {
            flags |= 1 << 1;
        }
        if self.pq {
            flags |= 1 << 2;
        }
        if self.hlg {
            flags |= 1 << 3;
        }
        if self.hevc_main10 {
            flags |= 1 << 4;
        }
        if self.av1_10bit {
            flags |= 1 << 5;
        }
        let peak = self.display_peak_nits.to_be_bytes();
        [1, flags, self.max_bit_depth, 0, peak[0], peak[1], 0, 0]
    }

    pub fn user_data_payload(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(24);
        payload.extend_from_slice(&CAPABILITY_UUID);
        payload.extend_from_slice(&self.payload());
        payload
    }

    pub fn sei_message(&self) -> SeiMessage {
        SeiMessage {
            payload_type: 5,
            payload: self.user_data_payload(),
        }
    }
}

pub fn decode_capability_payload(data: &[u8]) -> Option<HdrCapability> {
    if data.len() < 8 || data[0] != 1 {
        return None;
    }
    let flags = data[1];
    Some(HdrCapability {
        receive: flags & (1 << 0) != 0,
        send: flags & (1 << 1) != 0,
        pq: flags & (1 << 2) != 0,
        hlg: flags & (1 << 3) != 0,
        hevc_main10: flags & (1 << 4) != 0,
        av1_10bit: flags & (1 << 5) != 0,
        max_bit_depth: data[2],
        display_peak_nits: u16::from_be_bytes([data[4], data[5]]),
    })
}

pub fn decode_capability_sei(bytes: &[u8]) -> Option<HdrCapability> {
    let window = bytes
        .windows(16)
        .position(|chunk| chunk == CAPABILITY_UUID)?;
    decode_capability_payload(&bytes[window + 16..])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtpOneByteExtension {
    pub id: u8,
    pub uri: &'static str,
    pub data: [u8; 8],
    /// Always false. Discord select-protocol SDP must not grow an extmap from here.
    pub include_in_discord_sdp: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudienceMember {
    OfficialOrUnknown,
    Rencord(HdrCapability),
}

impl AudienceMember {
    pub fn wants_hdr10(&self) -> bool {
        match self {
            Self::OfficialOrUnknown => false,
            Self::Rencord(capability) => capability.wants_hdr10(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscordTrack {
    Sdr,
    ToneMappedSdr,
    Hdr10,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryPlan {
    pub discord_track: DiscordTrack,
    pub tone_map: bool,
    pub curve: ToneCurve,
    pub side_channel_hdr: bool,
    pub limits: Vec<&'static str>,
}

impl DeliveryPlan {
    pub fn summary(&self) -> String {
        match self.discord_track {
            DiscordTrack::Sdr => "Discord track is SDR.".into(),
            DiscordTrack::ToneMappedSdr => {
                "Discord track is tone-mapped SDR. 10-bit is not sent on that track.".into()
            }
            DiscordTrack::Hdr10 => {
                "Discord track is HDR10 for an exclusive Rencord audience. The SFU flag was set by the caller; this crate does not prove it.".into()
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct HdrSession {
    pub send: bool,
    pub receive: bool,
    pub curve: ToneCurve,
    pub audience: Vec<AudienceMember>,
    /// Caller-supplied. This crate never sets it from a network test.
    pub sfu_hdr_passthrough_proven: bool,
    pub display_peak_nits: u16,
}

impl HdrSession {
    pub fn from_settings(send: bool, receive: bool, curve: ToneCurve) -> Self {
        Self {
            send,
            receive,
            curve,
            audience: Vec::new(),
            sfu_hdr_passthrough_proven: false,
            display_peak_nits: 0,
        }
    }
}

pub trait HdrSignal {
    fn plan(&self, source: &SourceColorimetry) -> DeliveryPlan;
    fn capability(&self) -> HdrCapability;
    fn observe_sei(&mut self, bytes: &[u8]) -> bool;
    fn rtp_extension(&self) -> RtpOneByteExtension;
    fn side_channel_json(&self) -> serde_json::Value;
    /// Always `None`. Do not attach this to a Discord gateway or voice opcode.
    fn discord_gateway_payload(&self) -> Option<serde_json::Value>;
}

impl HdrSignal for HdrSession {
    fn plan(&self, source: &SourceColorimetry) -> DeliveryPlan {
        let mut limits = vec![LIMIT_OFFICIAL, LIMIT_STRIP];
        if !source.is_hdr() {
            return DeliveryPlan {
                discord_track: DiscordTrack::Sdr,
                tone_map: false,
                curve: self.curve,
                side_channel_hdr: false,
                limits,
            };
        }
        if !self.send {
            limits.push(LIMIT_SEND_OFF);
            return tone_mapped(self.curve, limits, false);
        }
        if self.audience.is_empty() {
            limits.push(LIMIT_UNKNOWN);
            return tone_mapped(self.curve, limits, false);
        }
        let hdr_peers = self
            .audience
            .iter()
            .filter(|member| member.wants_hdr10())
            .count();
        let others = self.audience.len() - hdr_peers;
        if others > 0 {
            limits.push(LIMIT_MIXED);
            if hdr_peers > 0 {
                limits.push(LIMIT_SIDE);
            }
            return tone_mapped(self.curve, limits, hdr_peers > 0);
        }
        if !self.sfu_hdr_passthrough_proven {
            limits.push(LIMIT_UNPROVEN);
            limits.push(LIMIT_SIDE);
            return tone_mapped(self.curve, limits, true);
        }
        limits.push(LIMIT_TRACK);
        DeliveryPlan {
            discord_track: DiscordTrack::Hdr10,
            tone_map: false,
            curve: self.curve,
            side_channel_hdr: false,
            limits,
        }
    }

    fn capability(&self) -> HdrCapability {
        HdrCapability::advertise(self.receive, self.send, self.display_peak_nits)
    }

    fn observe_sei(&mut self, bytes: &[u8]) -> bool {
        if let Some(capability) = decode_capability_sei(bytes) {
            self.audience.push(AudienceMember::Rencord(capability));
            true
        } else {
            false
        }
    }

    fn rtp_extension(&self) -> RtpOneByteExtension {
        RtpOneByteExtension {
            id: RTP_EXTENSION_ID,
            uri: RTP_EXTENSION_URI,
            data: self.capability().payload(),
            include_in_discord_sdp: false,
        }
    }

    fn side_channel_json(&self) -> serde_json::Value {
        let capability = self.capability();
        serde_json::json!({
            "rencord_hdr": {
                "v": 1,
                "receive": capability.receive,
                "send": capability.send,
                "peak_nits": capability.display_peak_nits,
                "max_bit_depth": capability.max_bit_depth,
                "hevc_main10": capability.hevc_main10,
                "av1_10bit": capability.av1_10bit,
                "note": "Rencord side channel. Not a Discord gateway field."
            }
        })
    }

    fn discord_gateway_payload(&self) -> Option<serde_json::Value> {
        None
    }
}

fn tone_mapped(curve: ToneCurve, limits: Vec<&'static str>, side: bool) -> DeliveryPlan {
    DeliveryPlan {
        discord_track: DiscordTrack::ToneMappedSdr,
        tone_map: true,
        curve,
        side_channel_hdr: side,
        limits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::hevc_prefix_sei;

    fn hdr_source() -> SourceColorimetry {
        SourceColorimetry::hdr10(1000.0)
    }

    fn peer(receive: bool) -> AudienceMember {
        AudienceMember::Rencord(HdrCapability::advertise(receive, false, 1000))
    }

    #[test]
    fn capability_sei_and_rtp_roundtrip_and_discord_payload_stays_empty() {
        let mut session = HdrSession::from_settings(true, true, ToneCurve::Bt2390);
        session.display_peak_nits = 1000;
        let extension = session.rtp_extension();
        assert!(!extension.include_in_discord_sdp);
        assert_eq!(extension.uri, RTP_EXTENSION_URI);
        assert_eq!(extension.data.len(), 8);
        let decoded = decode_capability_payload(&extension.data).unwrap();
        assert!(decoded.receive && decoded.send && decoded.wants_hdr10());
        assert_eq!(decoded.display_peak_nits, 1000);
        let nal = hevc_prefix_sei(&[session.capability().sei_message()]);
        assert!(session.observe_sei(&nal));
        assert_eq!(session.audience.len(), 1);
        assert!(session.discord_gateway_payload().is_none());
        let json = session.side_channel_json();
        assert!(json.get("op").is_none());
        assert_eq!(json["rencord_hdr"]["v"], 1);
        assert!(decode_capability_sei(b"not a sei").is_none());
    }

    #[test]
    fn plan_keeps_official_and_mixed_audiences_on_sdr() {
        let mut session = HdrSession::from_settings(true, true, ToneCurve::Hable);
        let off = session.plan(&hdr_source());
        assert_eq!(off.discord_track, DiscordTrack::ToneMappedSdr);
        assert!(!off.side_channel_hdr);

        session.audience.push(AudienceMember::OfficialOrUnknown);
        let official = session.plan(&hdr_source());
        assert_eq!(official.discord_track, DiscordTrack::ToneMappedSdr);
        assert!(!official.side_channel_hdr);

        session.audience.push(peer(true));
        let mixed = session.plan(&hdr_source());
        assert_eq!(mixed.discord_track, DiscordTrack::ToneMappedSdr);
        assert!(mixed.side_channel_hdr);
        assert!(mixed.limits.iter().any(|line| line.contains("mixed")));

        session.audience.clear();
        session.audience.push(peer(true));
        let unproven = session.plan(&hdr_source());
        assert_eq!(unproven.discord_track, DiscordTrack::ToneMappedSdr);
        assert!(unproven.side_channel_hdr);

        session.sfu_hdr_passthrough_proven = true;
        let exclusive = session.plan(&hdr_source());
        assert_eq!(exclusive.discord_track, DiscordTrack::Hdr10);
        assert!(!exclusive.tone_map);

        session.audience.clear();
        session.audience.push(peer(false));
        session.sfu_hdr_passthrough_proven = true;
        let silent = session.plan(&hdr_source());
        assert_eq!(silent.discord_track, DiscordTrack::ToneMappedSdr);

        session.send = false;
        session.audience.clear();
        session.audience.push(peer(true));
        assert_eq!(
            session.plan(&hdr_source()).discord_track,
            DiscordTrack::ToneMappedSdr
        );
        assert_eq!(
            session.plan(&SourceColorimetry::sdr()).discord_track,
            DiscordTrack::Sdr
        );
    }
}
