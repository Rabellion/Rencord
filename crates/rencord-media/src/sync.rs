//! Audio and video timestamps for one stream.
//!
//! Positive drift means the video stamp is ahead of the audio stamp. The
//! correction is a number of audio samples to drop (positive) or insert
//! (negative) so the next callback lines up.

pub fn drift_us(video_us: u64, audio_us: u64) -> i64 {
    video_us as i64 - audio_us as i64
}

pub fn correction_samples(drift_us: i64, sample_rate: u32) -> i32 {
    let samples = (drift_us as i128) * i128::from(sample_rate) / 1_000_000;
    samples.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32
}

/// A monitor or loopback device is system audio. A microphone is not.
pub fn is_system_monitor(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("monitor") || lower.contains("loopback") || lower.contains(".monitor")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_becomes_a_sample_correction() {
        assert_eq!(drift_us(1_020_000, 1_000_000), 20_000);
        assert_eq!(correction_samples(20_000, 48_000), 960);
        assert_eq!(correction_samples(-20_000, 48_000), -960);
        assert!(is_system_monitor("alsa_output.monitor"));
        assert!(!is_system_monitor("Built-in Microphone"));
    }
}
