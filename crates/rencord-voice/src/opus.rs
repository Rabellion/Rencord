//! Opus encode / decode with PLC and in-band FEC.
//!
//! Frames are 20 ms, 48 kHz, mono (960 samples). The encoder turns FEC on so a
//! following packet can reconstruct one lost frame. The jitter buffer decides
//! when to ask for FEC versus PLC; this type only talks to libopus.

use audiopus::coder::{Decoder, Encoder};
use audiopus::{Application, Bitrate, Channels, SampleRate};

pub const FRAME_SAMPLES: usize = 960;
pub const SAMPLE_RATE: u32 = 48_000;

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
}

impl OpusCodec {
    pub fn new() -> Result<Self, OpusError> {
        let mut encoder = Encoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip)?;
        encoder.set_bitrate(Bitrate::BitsPerSecond(64_000))?;
        encoder.set_inband_fec(true)?;
        encoder.set_packet_loss_perc(15)?;
        let decoder = Decoder::new(SampleRate::Hz48000, Channels::Mono)?;
        Ok(Self { encoder, decoder })
    }

    pub fn encode(&self, pcm: &[i16]) -> Result<Vec<u8>, OpusError> {
        if pcm.len() != FRAME_SAMPLES {
            return Err(OpusError::FrameSize {
                expected: FRAME_SAMPLES,
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
        let mut pcm = vec![0i16; FRAME_SAMPLES];
        let samples = self.decoder.decode(packet, &mut pcm[..], fec)?;
        pcm.truncate(samples);
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
}
