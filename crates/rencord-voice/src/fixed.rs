//! Baseline fixed 40 ms window used only by the loss comparison.
//!
//! Two frames of startup (40 ms at a 20 ms Opus frame), eight reorder slots,
//! concealment for at most three missing packets, then a hard skip to the
//! earliest buffered packet. A full window drops the new packet. That is the
//! failure mode a daily voice user hears around one or two percent loss.

const SLOTS: usize = 8;
const STARTUP_TICKS: u8 = 2;
const MAX_PLC: u8 = 3;

#[derive(Clone, Debug, Default)]
pub struct FixedWindow {
    slots: Vec<(u16, Vec<u8>)>,
    next: Option<u16>,
    wait: u8,
    missing: u8,
    pub hard_skips: u64,
    pub dropped: u64,
    pub played: u64,
    pub concealed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FixedPop {
    Wait,
    Audio(Vec<u8>),
    Plc,
    HardSkip { jumped: u16, payload: Vec<u8> },
    Silence,
}

impl FixedWindow {
    pub fn push(&mut self, sequence: u16, payload: Vec<u8>) {
        if self.next.is_none() {
            self.next = Some(sequence);
            self.wait = STARTUP_TICKS;
        }
        let next = self.next.expect("set above");
        let distance = sequence.wrapping_sub(next);
        if distance >= 32_768 {
            self.dropped += 1;
            return;
        }
        if distance >= SLOTS as u16 {
            self.slots.clear();
            self.next = Some(sequence);
            self.wait = STARTUP_TICKS;
            self.missing = 0;
        }
        if self.slots.len() >= SLOTS {
            self.dropped += 1;
            return;
        }
        if self.slots.iter().any(|(id, _)| *id == sequence) {
            return;
        }
        self.slots.push((sequence, payload));
    }

    pub fn pop(&mut self) -> FixedPop {
        if self.wait > 0 && self.slots.len() < SLOTS {
            self.wait -= 1;
            return FixedPop::Wait;
        }
        self.wait = 0;
        let Some(next) = self.next else {
            return FixedPop::Wait;
        };
        self.next = Some(next.wrapping_add(1));
        if let Some(index) = self.slots.iter().position(|(id, _)| *id == next) {
            self.missing = 0;
            self.played += 1;
            return FixedPop::Audio(self.slots.swap_remove(index).1);
        }
        self.missing = self.missing.saturating_add(1);
        if self.missing > MAX_PLC {
            if let Some((index, _)) = self
                .slots
                .iter()
                .enumerate()
                .min_by_key(|(_, (sequence, _))| sequence.wrapping_sub(next))
            {
                let (sequence, payload) = self.slots.swap_remove(index);
                let jumped = sequence.wrapping_sub(next).max(1);
                self.next = Some(sequence.wrapping_add(1));
                self.missing = 0;
                self.hard_skips += u64::from(jumped);
                self.played += 1;
                return FixedPop::HardSkip { jumped, payload };
            }
            self.slots.clear();
            self.next = None;
            self.missing = 0;
            self.hard_skips += 1;
            return FixedPop::Silence;
        }
        self.concealed += 1;
        FixedPop::Plc
    }
}
