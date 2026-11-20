//! Go Live session state.
//!
//! Creating a stream emits a gateway voice-state update with `self_stream`.
//! Watching someone else emits a stream-watch message for the separate video
//! connection. Volume is per stream, from 0 to 200 percent. Nothing here
//! opens a Discord socket.

use rencord_model::Snowflake;
use rencord_protocol::{stream_watch, voice_state_go_live};
use rencord_voice::dave::{DaveEngine, DaveOps};
use serde_json::Value;

use crate::encode::{active_encoder, encode_software};
use crate::frame::Frame;
use crate::preset::{clamp_bitrate, safe_preset, StreamPreset};
use crate::rtp::{packetize, RtpPacket};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamPhase {
    Idle,
    Live,
    Watching,
}

#[derive(Clone, Debug)]
pub struct StreamStats {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub encoder: &'static str,
    pub dropped_frames: u64,
    pub rtt_ms: u32,
    pub volume_percent: u32,
}

pub struct GoLive {
    pub phase: StreamPhase,
    pub preset: StreamPreset,
    pub bitrate_kbps: u32,
    pub volume_percent: u32,
    pub dropped_frames: u64,
    pub rtt_ms: u32,
    sequence: u16,
    timestamp: u32,
}

impl Default for GoLive {
    fn default() -> Self {
        Self::new()
    }
}

impl GoLive {
    pub fn new() -> Self {
        let preset = safe_preset();
        Self {
            phase: StreamPhase::Idle,
            bitrate_kbps: preset.bitrate_kbps,
            preset,
            volume_percent: 100,
            dropped_frames: 0,
            rtt_ms: 0,
            sequence: 1,
            timestamp: 0,
        }
    }

    pub fn set_preset(&mut self, preset: StreamPreset) {
        self.bitrate_kbps = preset.bitrate_kbps;
        self.preset = preset;
    }

    pub fn set_bitrate(&mut self, kbps: u32) {
        self.bitrate_kbps = clamp_bitrate(self.preset, kbps);
    }

    pub fn set_volume(&mut self, percent: u32) {
        self.volume_percent = percent.min(200);
    }

    pub fn create(&mut self, guild: Option<Snowflake>, channel: Option<Snowflake>) -> Value {
        self.phase = StreamPhase::Live;
        voice_state_go_live(guild, channel, true)
    }

    pub fn watch(&mut self, user_id: &str) -> Value {
        self.phase = StreamPhase::Watching;
        stream_watch(user_id)
    }

    pub fn stop(&mut self, guild: Option<Snowflake>, channel: Option<Snowflake>) -> Value {
        self.phase = StreamPhase::Idle;
        voice_state_go_live(guild, channel, false)
    }

    pub fn stats(&self) -> StreamStats {
        StreamStats {
            width: self.preset.width,
            height: self.preset.height,
            fps: self.preset.fps,
            bitrate_kbps: self.bitrate_kbps,
            encoder: active_encoder().label(),
            dropped_frames: self.dropped_frames,
            rtt_ms: self.rtt_ms,
            volume_percent: self.volume_percent,
        }
    }

    /// Encode one frame and packetize it. When `dave` is set, each RTP payload
    /// is wrapped for the video media type before it would be sent.
    pub fn send_frame(
        &mut self,
        frame: &Frame,
        mut dave: Option<&mut DaveEngine>,
    ) -> Result<Vec<RtpPacket>, String> {
        if self.phase != StreamPhase::Live {
            return Err("stream is not live".into());
        }
        let unit = encode_software(frame, self.bitrate_kbps);
        let mut packets = Vec::new();
        for (index, nal) in unit.nal_units.iter().enumerate() {
            let last = index + 1 == unit.nal_units.len();
            let mut piece = packetize(nal, 1200, self.sequence, self.timestamp, 1)?;
            self.sequence = self.sequence.wrapping_add(piece.len() as u16);
            if let Some(engine) = dave.as_deref_mut() {
                for packet in &mut piece {
                    packet.payload = engine.encrypt_video(&packet.payload)?;
                }
            }
            if !last {
                for packet in &mut piece {
                    packet.marker = false;
                }
            }
            packets.extend(piece);
        }
        self.timestamp = self.timestamp.wrapping_add(90_000 / self.preset.fps.max(1));
        let _ = unit.nal_units.len();
        Ok(packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rencord_voice::dave::{DaveEngine, ScriptedDave};

    #[test]
    fn create_sets_self_stream_and_volume_caps_at_200() {
        let mut live = GoLive::new();
        let payload = live.create(None, None);
        assert_eq!(live.phase, StreamPhase::Live);
        assert_eq!(payload["d"]["self_stream"], true);
        live.set_volume(500);
        assert_eq!(live.volume_percent, 200);
        live.set_bitrate(1);
        assert!(live.bitrate_kbps >= 200);
        let watch = live.watch("42");
        assert_eq!(watch["d"]["user_id"], "42");
        assert_eq!(live.phase, StreamPhase::Watching);
    }

    #[test]
    fn a_live_frame_can_be_wrapped_for_dave_video() {
        let mut live = GoLive::new();
        live.preset.width = 2;
        live.preset.height = 1;
        let _ = live.create(None, None);
        let mut frame = Frame::new(2, 1, 1);
        frame.bgra = vec![0, 0, 255, 255, 10, 10, 10, 255];
        let mut dave = DaveEngine::Scripted(ScriptedDave {
            ready: true,
            ..ScriptedDave::default()
        });
        let packets = live.send_frame(&frame, Some(&mut dave)).unwrap();
        assert!(packets.len() >= 2);
        assert!(packets
            .iter()
            .all(|packet| packet.payload.first() == Some(&0xDB)));
        assert!(packets.last().unwrap().marker);
    }
}
