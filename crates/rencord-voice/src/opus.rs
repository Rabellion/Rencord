//! Opus encode / decode with PLC and in-band FEC.
//!
//! Frames are 20 ms, 48 kHz, mono (960 samples). The encoder turns FEC on so a
//! following packet can reconstruct one lost frame. The jitter buffer decides
//! when to ask for FEC versus PLC; this type only talks to libopus.

use audiopus::coder::{Decoder, Encoder};
use audiopus::{Application, Bitrate, Channels, SampleRate};

pub const FRAME_SAMPLES: usize = 960;
pub const SAMPLE_RATE: u32 = 48_000;
/// Opus silence frame. Five of these go out before speaking stops.
pub const SILENCE_OPUS: &[u8] = &[0xF8, 0xFF, 0xFE];

#[derive(Debug, thiserror::Error)]
pub enum OpusError {
    #[error("opus error: {0}")]
    Codec(#[from] audiopus::Error),
    #[error("expected {expected} samples, got {got}")]
    FrameSize { expected: usize, got: usize },
}

pub struct OpusCodec {
    encoder: Encoder,
    decoder: Decoder,
    channels: usize,
}

impl OpusCodec {
    pub fn new() -> Result<Self, OpusError> {
        Self::with_channels(Channels::Mono)
    }

    /// Discord voice is 48 kHz stereo, 20 ms: 960 samples per channel.
    pub fn stereo() -> Result<Self, OpusError> {
        Self::with_channels(Channels::Stereo)
    }

    fn with_channels(channels: Channels) -> Result<Self, OpusError> {
        let mut encoder = Encoder::new(SampleRate::Hz48000, channels, Application::Voip)?;
        encoder.set_bitrate(Bitrate::BitsPerSecond(64_000))?;
        encoder.set_inband_fec(true)?;
        encoder.set_packet_loss_perc(15)?;
        let decoder = Decoder::new(SampleRate::Hz48000, channels)?;
        let channels = match channels {
            Channels::Mono => 1,
            Channels::Stereo | Channels::Auto => 2,
        };
        Ok(Self {
            encoder,
            decoder,
            channels,
        })
    }

    pub fn frame_len(&self) -> usize {
        FRAME_SAMPLES * self.channels
    }

    pub fn encode(&self, pcm: &[i16]) -> Result<Vec<u8>, OpusError> {
        if pcm.len() != self.frame_len() {
            return Err(OpusError::FrameSize {
                expected: self.frame_len(),
                got: pcm.len(),
            });
        }
        let mut output = vec![0u8; 512];
        let written = self.encoder.encode(pcm, &mut output)?;
        output.truncate(written);
        Ok(output)
    }

    pub fn decode_packet(&mut self, packet: &[u8]) -> Result<Vec<i16>, OpusError> {
        self.decode(Some(packet), false)
    }

    pub fn decode_fec(&mut self, next_packet: &[u8]) -> Result<Vec<i16>, OpusError> {
        self.decode(Some(next_packet), true)
    }

    pub fn decode_plc(&mut self) -> Result<Vec<i16>, OpusError> {
        self.decode(None, false)
    }

    fn decode(&mut self, packet: Option<&[u8]>, fec: bool) -> Result<Vec<i16>, OpusError> {
        let mut pcm = vec![0i16; self.frame_len()];
        let samples = self.decoder.decode(packet, &mut pcm[..], fec)?;
        pcm.truncate(samples * self.channels);
        Ok(pcm)
    }
}

pub fn sine_frame(step: usize) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|index| {
            let t = (step * FRAME_SAMPLES + index) as f32;
            (t * 0.08).sin() * 8_000.0
        })
        .map(|sample| sample as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_and_plc_and_fec_produce_samples() {
        let mut codec = OpusCodec::new().unwrap();
        let mut packets = Vec::new();
        for step in 0..6 {
            packets.push(codec.encode(&sine_frame(step)).unwrap());
        }
        let first = codec.decode_packet(&packets[0]).unwrap();
        assert_eq!(first.len(), FRAME_SAMPLES);
        let concealed = codec.decode_plc().unwrap();
        assert!(!concealed.is_empty());
        let fec = codec.decode_fec(&packets[3]).unwrap();
        assert!(!fec.is_empty());
        let after = codec.decode_packet(&packets[3]).unwrap();
        assert_eq!(after.len(), FRAME_SAMPLES);
    }

    #[test]
    fn stereo_frame_is_20ms_at_48khz() {
        let mut codec = OpusCodec::stereo().unwrap();
        assert_eq!(codec.frame_len(), FRAME_SAMPLES * 2);
        let pcm: Vec<i16> = (0..codec.frame_len())
            .map(|index| ((index as f32) * 0.05).sin() as i16 * 1000)
            .collect();
        let packet = codec.encode(&pcm).unwrap();
        let decoded = codec.decode_packet(&packet).unwrap();
        assert_eq!(decoded.len(), FRAME_SAMPLES * 2);
    }
}
