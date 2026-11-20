//! Per-speaker gain, mute, and deafen applied after the jitter buffer.

use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct GainMixer {
    deaf: bool,
    /// Linear gain per RTP SSRC. Missing keys use 1.0.
    gains: HashMap<u32, f32>,
}

impl GainMixer {
    pub fn set_deaf(&mut self, deaf: bool) {
        self.deaf = deaf;
    }

    pub fn deaf(&self) -> bool {
        self.deaf
    }

    pub fn set_gain(&mut self, ssrc: u32, gain: f32) {
        self.gains.insert(ssrc, gain.clamp(0.0, 2.0));
    }

    pub fn gain(&self, ssrc: u32) -> f32 {
        if self.deaf {
            0.0
        } else {
            self.gains.get(&ssrc).copied().unwrap_or(1.0)
        }
    }

    pub fn apply(&self, ssrc: u32, pcm: &mut [i16]) {
        let gain = self.gain(ssrc);
        if gain == 0.0 {
            pcm.fill(0);
            return;
        }
        if (gain - 1.0).abs() < f32::EPSILON {
            return;
        }
        for sample in pcm.iter_mut() {
            let scaled = f32::from(*sample) * gain;
            *sample = scaled.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deaf_and_half_gain_scale_samples() {
        let mut mixer = GainMixer::default();
        let mut pcm = vec![1000, -1000];
        mixer.set_gain(7, 0.5);
        mixer.apply(7, &mut pcm);
        assert_eq!(pcm, vec![500, -500]);
        mixer.set_deaf(true);
        mixer.apply(7, &mut pcm);
        assert_eq!(pcm, vec![0, 0]);
    }
}
