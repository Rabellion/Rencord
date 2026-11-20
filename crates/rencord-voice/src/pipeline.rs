//! One-way and loopback audio paths.
//!
//! Capture PCM is Opus-encoded, optionally wrapped by DAVE, then sealed with
//! the transport cipher. Receive does the reverse and plays through the
//! adaptive jitter buffer. [`loopback_call`] runs that path in memory with a
//! configurable loss rate so tests can hear the buffer without a sound card.

use crate::crypto::TransportCipher;
use crate::dave::DaveOps;
use crate::jitter::{AdaptiveBuffer, AdaptiveConfig, Playout};
use crate::opus::{OpusCodec, FRAME_SAMPLES};
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq)]
pub struct LoopbackReport {
    pub sent: u32,
    pub played: u32,
    pub concealed: u32,
    pub hard_gaps: u32,
    pub target_ms: u32,
    pub mean_abs_error: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("opus error: {0}")]
    Opus(#[from] crate::opus::OpusError),
    #[error("dave error: {0}")]
    Dave(String),
    #[error("transport error: {0}")]
    Transport(#[from] crate::crypto::CryptoError),
}

pub struct VoicePipeline {
    pub codec: OpusCodec,
    pub jitter: AdaptiveBuffer,
    pub transport: Option<TransportCipher>,
    pub ssrc: u32,
    sequence: u16,
    timestamp: u32,
}

impl VoicePipeline {
    pub fn new(ssrc: u32) -> Result<Self, PipelineError> {
        Ok(Self {
            codec: OpusCodec::new()?,
            jitter: AdaptiveBuffer::new(AdaptiveConfig::default()),
            transport: None,
            ssrc,
            sequence: 0,
            timestamp: 0,
        })
    }

    pub fn install_key(&mut self, key: &[u8]) -> Result<(), PipelineError> {
        self.transport = Some(TransportCipher::from_bytes(key)?);
        Ok(())
    }

    pub fn send_frame(
        &mut self,
        pcm: &[i16],
        dave: Option<&mut dyn DaveOps>,
    ) -> Result<Vec<u8>, PipelineError> {
        let mut opus = self.codec.encode(pcm)?;
        if let Some(dave) = dave {
            if dave.can_encrypt() {
                opus = dave.encrypt_opus(&opus).map_err(PipelineError::Dave)?;
            }
        }
        let sequence = self.sequence;
        let timestamp = self.timestamp;
        self.sequence = self.sequence.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(FRAME_SAMPLES as u32);
        let Some(transport) = self.transport.as_mut() else {
            return Err(PipelineError::Transport(
                crate::crypto::CryptoError::KeyLength(0),
            ));
        };
        Ok(transport.seal(sequence, timestamp, self.ssrc, &opus))
    }

    pub fn recv_packet(
        &mut self,
        packet: &[u8],
        arrival_ms: u64,
        sender: u64,
        dave: Option<&mut dyn DaveOps>,
        fec: bool,
    ) -> Result<(), PipelineError> {
        let Some(transport) = self.transport.as_ref() else {
            return Err(PipelineError::Transport(
                crate::crypto::CryptoError::KeyLength(0),
            ));
        };
        let opened = transport.open(packet)?;
        let opus = if let Some(dave) = dave {
            dave.decrypt_opus(sender, &opened.opus)
                .map_err(PipelineError::Dave)?
        } else {
            opened.opus
        };
        self.jitter.push(opened.sequence, opus, arrival_ms, fec);
        Ok(())
    }

    pub fn pull_pcm(&mut self) -> Result<Vec<i16>, PipelineError> {
        match self.jitter.pop() {
            Playout::Audio(packet) => self
                .codec
                .decode_packet(&packet)
                .map_err(PipelineError::from),
            Playout::Fec => {
                // The carrier stays buffered for its own playout tick. This tick
                // asks Opus for packet-loss concealment.
                self.codec.decode_plc().map_err(PipelineError::from)
            }
            Playout::Plc | Playout::HardGap | Playout::Startup | Playout::Compressed => {
                self.codec.decode_plc().map_err(PipelineError::from)
            }
        }
    }
}

struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((self.0 >> 33) as f64) / (u32::MAX as f64)
    }
}

/// Encode `frames` of a sine, drop some, and play them back through the adaptive buffer.
pub fn loopback_call(frames: usize, loss: f64, seed: u64) -> Result<LoopbackReport, PipelineError> {
    let mut send = VoicePipeline::new(1)?;
    let mut recv = VoicePipeline::new(1)?;
    let key = [9u8; 32];
    send.install_key(&key)?;
    recv.install_key(&key)?;
    let mut rng = Rng(seed);
    let mut queued: VecDeque<(u64, Vec<u8>, bool)> = VecDeque::new();
    let mut want_fec = false;
    let mut sent = 0u32;
    for step in 0..frames {
        let pcm = crate::opus::sine_frame(step);
        if rng.unit() < loss {
            want_fec = true;
        } else {
            let packet = send.send_frame(&pcm, None)?;
            let delay = 30
                + if rng.unit() < 0.08 {
                    70
                } else {
                    (rng.unit() * 20.0) as u64
                };
            let arrive = step as u64 * 20 + delay;
            queued.push_back((arrive, packet, want_fec));
            want_fec = false;
            sent += 1;
        }
    }
    let mut pending: Vec<_> = queued.drain(..).collect();
    pending.sort_by_key(|item| item.0);
    let mut index = 0;
    let mut played = 0u32;
    let mut error_sum = 0.0;
    let mut error_n = 0u32;
    let end = frames as u64 * 20 + 500;
    let mut clock = 0u64;
    let mut heard = Vec::new();
    while clock <= end {
        while index < pending.len() && pending[index].0 <= clock {
            let (_, packet, fec) = &pending[index];
            recv.recv_packet(packet, clock, 1, None, *fec)?;
            index += 1;
        }
        let pcm = recv.pull_pcm()?;
        if recv.jitter.stats.played > played as u64 {
            played = recv.jitter.stats.played as u32;
        }
        if !pcm.is_empty() {
            heard.push(pcm);
        }
        clock += 20;
    }
    // Compare a mid window of received audio energy against silence.
    for frame in heard.iter().skip(4).take(20) {
        let energy = frame
            .iter()
            .map(|sample| i32::from(*sample).abs())
            .sum::<i32>();
        error_sum += f64::from(energy);
        error_n += 1;
    }
    let mean_abs_error = if error_n == 0 {
        0.0
    } else {
        error_sum / f64::from(error_n)
    };
    Ok(LoopbackReport {
        sent,
        played: recv.jitter.stats.played as u32,
        concealed: recv.jitter.stats.concealed as u32,
        hard_gaps: recv.jitter.stats.hard_gaps as u32,
        target_ms: recv.jitter.target_ms(),
        mean_abs_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_at_one_and_a_half_percent_keeps_energy() {
        let report = loopback_call(200, 0.015, 7).unwrap();
        assert!(report.sent > 170, "{report:?}");
        assert!(report.played > 90, "{report:?}");
        assert!(report.mean_abs_error > 100.0, "{report:?}");
        assert!(report.hard_gaps < 25, "{report:?}");
        assert!(report.target_ms >= 80, "{report:?}");
    }
}
