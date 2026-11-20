//! Adaptive jitter buffer.
//!
//! Target delay starts at 80 ms and moves between 40 ms and 120 ms. It grows
//! when packets arrive late or have to be concealed, and it shrinks only after
//! a stretch of clean audio. Missing frames are covered with Opus FEC (when the
//! following packet carries it) or with PLC. The read cursor never jumps to
//! the newest packet: if concealment runs out, it advances one frame and
//! counts a hard gap. When the buffer is deeper than the target, one frame is
//! dropped on purpose (time-stretch compression) so latency can fall without a
//! skip across a hole.

use std::collections::{HashMap, HashSet};

pub const FRAME_MS: u32 = 20;

#[derive(Clone, Debug)]
pub struct AdaptiveConfig {
    pub min_target_ms: u32,
    pub initial_target_ms: u32,
    pub max_target_ms: u32,
    pub frame_ms: u32,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            min_target_ms: 40,
            initial_target_ms: 80,
            max_target_ms: 120,
            frame_ms: FRAME_MS,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JitterStats {
    pub played: u64,
    pub concealed: u64,
    pub fec_used: u64,
    pub hard_gaps: u64,
    pub late_drops: u64,
    pub compressed: u64,
    pub target_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Playout {
    Startup,
    Audio(Vec<u8>),
    /// The next stored packet contains Opus in-band FEC for the missing frame.
    Fec,
    Plc,
    /// A received frame was discarded to pull latency back toward the target.
    Compressed,
    HardGap,
}

#[derive(Clone, Debug)]
pub struct AdaptiveBuffer {
    config: AdaptiveConfig,
    slots: HashMap<u16, Vec<u8>>,
    fec_carriers: HashSet<u16>,
    next: Option<u16>,
    target_ms: u32,
    startup_left: u32,
    started: bool,
    missing_run: u32,
    clean_run: u32,
    jitter_ms: f64,
    last_transit: Option<f64>,
    pub stats: JitterStats,
}

impl AdaptiveBuffer {
    pub fn new(config: AdaptiveConfig) -> Self {
        let target_ms = config
            .initial_target_ms
            .clamp(config.min_target_ms, config.max_target_ms);
        let startup_left = target_ms / config.frame_ms.max(1);
        Self {
            config,
            slots: HashMap::new(),
            fec_carriers: HashSet::new(),
            next: None,
            target_ms,
            startup_left,
            started: false,
            missing_run: 0,
            clean_run: 0,
            jitter_ms: 0.0,
            last_transit: None,
            stats: JitterStats {
                target_ms,
                ..JitterStats::default()
            },
        }
    }

    pub fn target_ms(&self) -> u32 {
        self.target_ms
    }

    pub fn jitter_ms(&self) -> f64 {
        self.jitter_ms
    }

    /// `fec_for_previous` is true when this Opus packet can reconstruct sequence - 1.
    pub fn push(
        &mut self,
        sequence: u16,
        payload: Vec<u8>,
        arrival_ms: u64,
        fec_for_previous: bool,
    ) {
        if !self.started {
            match self.next {
                None => {
                    self.next = Some(sequence);
                    self.startup_left = self.target_ms / self.config.frame_ms.max(1);
                }
                Some(cursor) => {
                    let delta = seq_delta(sequence, cursor);
                    if (-64..0).contains(&delta) {
                        self.next = Some(sequence);
                    }
                }
            }
        } else if let Some(next) = self.next {
            if seq_delta(sequence, next) < 0 {
                self.stats.late_drops += 1;
                self.note_trouble();
                return;
            }
        }
        self.observe_delay(sequence, arrival_ms);
        if fec_for_previous {
            self.fec_carriers.insert(sequence);
        }
        self.slots.insert(sequence, payload);
        self.trim();
        self.stats.target_ms = self.target_ms;
    }

    pub fn pop(&mut self) -> Playout {
        let Some(next) = self.next else {
            return Playout::Startup;
        };
        if !self.started {
            if self.depth_ms() >= self.target_ms || self.startup_left == 0 {
                self.started = true;
            } else {
                self.startup_left = self.startup_left.saturating_sub(1);
                return Playout::Startup;
            }
        }
        if self.depth_ms() > self.target_ms.saturating_add(40)
            && self.slots.contains_key(&next)
            && self.slots.contains_key(&next.wrapping_add(1))
        {
            self.slots.remove(&next);
            self.fec_carriers.remove(&next);
            self.next = Some(next.wrapping_add(1));
            self.stats.compressed += 1;
            self.stats.target_ms = self.target_ms;
            return Playout::Compressed;
        }
        let next = self.next.expect("cursor");
        if let Some(payload) = self.slots.remove(&next) {
            self.fec_carriers.remove(&next);
            self.next = Some(next.wrapping_add(1));
            self.missing_run = 0;
            self.clean_run = self.clean_run.saturating_add(1);
            if self.clean_run >= 50 {
                self.clean_run = 0;
                self.target_ms = self
                    .target_ms
                    .saturating_sub(self.config.frame_ms)
                    .max(self.config.min_target_ms);
            }
            self.stats.played += 1;
            self.stats.target_ms = self.target_ms;
            return Playout::Audio(payload);
        }
        let carrier = next.wrapping_add(1);
        if self.fec_carriers.contains(&carrier) && self.slots.contains_key(&carrier) {
            self.next = Some(carrier);
            self.missing_run = 0;
            self.stats.concealed += 1;
            self.stats.fec_used += 1;
            self.note_trouble();
            return Playout::Fec;
        }
        self.missing_run = self.missing_run.saturating_add(1);
        self.clean_run = 0;
        self.note_trouble();
        let budget = (self.target_ms / self.config.frame_ms.max(1)).max(2);
        self.next = Some(next.wrapping_add(1));
        if self.missing_run <= budget {
            self.stats.concealed += 1;
            self.stats.target_ms = self.target_ms;
            return Playout::Plc;
        }
        self.missing_run = 0;
        self.stats.hard_gaps += 1;
        self.stats.target_ms = self.target_ms;
        Playout::HardGap
    }

    fn observe_delay(&mut self, sequence: u16, arrival_ms: u64) {
        let nominal = u64::from(sequence) * u64::from(self.config.frame_ms);
        let transit = arrival_ms as f64 - nominal as f64;
        if let Some(previous) = self.last_transit {
            let delta = (transit - previous).abs();
            self.jitter_ms += (delta - self.jitter_ms) / 16.0;
        }
        self.last_transit = Some(transit);
    }

    fn note_trouble(&mut self) {
        self.clean_run = 0;
        self.target_ms = self
            .target_ms
            .saturating_add(self.config.frame_ms)
            .min(self.config.max_target_ms);
        self.stats.target_ms = self.target_ms;
    }

    fn depth_ms(&self) -> u32 {
        let Some(next) = self.next else {
            return 0;
        };
        let count = self
            .slots
            .keys()
            .filter(|sequence| {
                let delta = seq_delta(**sequence, next);
                (0..32).contains(&delta)
            })
            .count();
        count as u32 * self.config.frame_ms
    }

    fn trim(&mut self) {
        while self.slots.len() > 48 {
            let Some(cursor) = self.next else {
                break;
            };
            let Some(farthest) = self
                .slots
                .keys()
                .copied()
                .max_by_key(|sequence| seq_delta(*sequence, cursor))
            else {
                break;
            };
            self.slots.remove(&farthest);
            self.fec_carriers.remove(&farthest);
        }
    }
}

fn seq_delta(sequence: u16, cursor: u16) -> i32 {
    sequence.wrapping_sub(cursor) as i16 as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::{FixedPop, FixedWindow};

    struct Arrival {
        seq: u16,
        arrive_ms: u64,
        payload: Vec<u8>,
        fec: bool,
    }

    struct Rng(u64);

    impl Rng {
        fn unit(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((self.0 >> 33) as f64) / (u32::MAX as f64)
        }
    }

    fn network(loss: f64, seed: u64, count: u32) -> Vec<Arrival> {
        let mut rng = Rng(seed);
        let mut out = Vec::new();
        let mut want_fec = false;
        for index in 0..count {
            if rng.unit() < loss {
                want_fec = true;
                continue;
            }
            let spike = if rng.unit() < 0.08 {
                90
            } else {
                (rng.unit() * 30.0) as u64
            };
            out.push(Arrival {
                seq: index as u16,
                arrive_ms: u64::from(index) * 20 + 20 + spike,
                payload: vec![(index % 251) as u8, 1, 2, 3],
                fec: want_fec,
            });
            want_fec = false;
        }
        out.sort_by_key(|item| (item.arrive_ms, item.seq));
        out
    }

    fn run_pair(loss: f64, seed: u64) -> (JitterStats, FixedWindow) {
        let arrivals = network(loss, seed, 3_000);
        let mut adaptive = AdaptiveBuffer::new(AdaptiveConfig::default());
        let mut fixed = FixedWindow::default();
        let mut cursor = 0;
        let end = arrivals
            .last()
            .map(|item| item.arrive_ms + 400)
            .unwrap_or(0);
        let mut clock = 0u64;
        while clock <= end {
            while cursor < arrivals.len() && arrivals[cursor].arrive_ms <= clock {
                let packet = &arrivals[cursor];
                adaptive.push(
                    packet.seq,
                    packet.payload.clone(),
                    packet.arrive_ms,
                    packet.fec,
                );
                fixed.push(packet.seq, packet.payload.clone());
                cursor += 1;
            }
            let _ = adaptive.pop();
            let _ = fixed.pop();
            clock += 20;
        }
        (adaptive.stats, fixed)
    }

    #[test]
    fn reorder_within_the_target_plays_in_order() {
        let mut buffer = AdaptiveBuffer::new(AdaptiveConfig::default());
        buffer.push(1, vec![1], 40, false);
        buffer.push(0, vec![0], 45, false);
        buffer.push(2, vec![2], 60, false);
        let mut heard = Vec::new();
        for _ in 0..8 {
            if let Playout::Audio(payload) = buffer.pop() {
                heard.push(payload[0]);
            }
        }
        assert_eq!(heard, vec![0, 1, 2]);
    }

    #[test]
    fn fec_flag_is_used_before_a_hard_gap() {
        let mut buffer = AdaptiveBuffer::new(AdaptiveConfig {
            initial_target_ms: 40,
            min_target_ms: 40,
            max_target_ms: 120,
            frame_ms: 20,
        });
        buffer.push(0, vec![0], 0, false);
        buffer.push(2, vec![2], 40, true);
        let mut saw_fec = false;
        let mut saw_two = false;
        for _ in 0..6 {
            match buffer.pop() {
                Playout::Fec => saw_fec = true,
                Playout::Audio(payload) if payload[0] == 2 => saw_two = true,
                _ => {}
            }
        }
        assert!(saw_fec);
        assert!(saw_two);
        assert_eq!(buffer.stats.hard_gaps, 0);
    }

    #[test]
    fn target_grows_when_audio_is_late_and_stays_inside_the_cap() {
        let mut buffer = AdaptiveBuffer::new(AdaptiveConfig::default());
        for sequence in 0..6u16 {
            buffer.push(
                sequence,
                vec![sequence as u8],
                u64::from(sequence) * 20,
                false,
            );
        }
        for _ in 0..16 {
            let _ = buffer.pop();
        }
        let before = buffer.target_ms();
        buffer.push(0, b"late".to_vec(), 800, false);
        assert!(buffer.stats.late_drops >= 1);
        assert!(buffer.target_ms() >= before);
        assert!(buffer.target_ms() > 80 || before > 80);
        assert!(buffer.target_ms() <= 120);
    }

    #[test]
    fn adaptive_beats_fixed_40ms_at_mild_and_heavy_loss() {
        for (loss, seed) in [(0.015, 0xC0FFEE_u64), (0.05, 0xA11CE)] {
            let (adaptive, fixed) = run_pair(loss, seed);
            assert!(
                adaptive.hard_gaps < fixed.hard_skips,
                "loss {loss}: adaptive hard gaps {} (played {}, concealed {}, fec {}, target {} ms) vs fixed skips {} (played {}, concealed {}, dropped {})",
                adaptive.hard_gaps,
                adaptive.played,
                adaptive.concealed,
                adaptive.fec_used,
                adaptive.target_ms,
                fixed.hard_skips,
                fixed.played,
                fixed.concealed,
                fixed.dropped
            );
            assert!(
                fixed.hard_skips > 0,
                "fixed window produced no skips at loss {loss}"
            );
            assert!(adaptive.target_ms >= 80, "target {}", adaptive.target_ms);
            let _ = FixedPop::Wait;
        }
    }
}
