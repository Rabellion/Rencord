//! Audio device helpers.
//!
//! Tests and `--demo` use the in-memory loopback in `pipeline`. A real join
//! asks cpal for the default input and output. Opening those devices needs a
//! machine with a sound card; this module only reports what it can see.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevices {
    pub output: Option<String>,
    pub input: Option<String>,
}

pub fn default_devices() -> AudioDevices {
    #[cfg(feature = "cpal")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        AudioDevices {
            output: host.default_output_device().map(|device| {
                device
                    .name()
                    .unwrap_or_else(|_| "default output".to_owned())
            }),
            input: host
                .default_input_device()
                .map(|device| device.name().unwrap_or_else(|_| "default input".to_owned())),
        }
    }
    #[cfg(not(feature = "cpal"))]
    {
        AudioDevices {
            output: None,
            input: None,
        }
    }
}

pub fn backend_name() -> &'static str {
    if cfg!(feature = "cpal") {
        "cpal"
    } else {
        "memory"
    }
}

pub fn list_input_devices() -> Vec<String> {
    #[cfg(feature = "cpal")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        host.input_devices()
            .map(|devices| devices.filter_map(|device| device.name().ok()).collect())
            .unwrap_or_default()
    }
    #[cfg(not(feature = "cpal"))]
    Vec::new()
}

pub fn list_output_devices() -> Vec<String> {
    #[cfg(feature = "cpal")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        host.output_devices()
            .map(|devices| devices.filter_map(|device| device.name().ok()).collect())
            .unwrap_or_default()
    }
    #[cfg(not(feature = "cpal"))]
    Vec::new()
}

/// Open the default microphone and speaker and run them through [`crate::CallAudio`].
/// The returned streams must be kept alive. `outbound` receives sealed RTP packets
/// from the microphone. Speaker samples are pulled from `inbound`.
#[cfg(feature = "cpal")]
pub fn attach_default_devices(
    audio: std::sync::Arc<std::sync::Mutex<crate::CallAudio>>,
    outbound: std::sync::mpsc::Sender<Vec<u8>>,
    inbound: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<i16>>>,
) -> Result<AudioSession, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let input = host
        .default_input_device()
        .ok_or_else(|| "no default input device".to_owned())?;
    let output = host
        .default_output_device()
        .ok_or_else(|| "no default output device".to_owned())?;
    let input_config = input
        .default_input_config()
        .map_err(|err| err.to_string())?;
    let output_config = output
        .default_output_config()
        .map_err(|err| err.to_string())?;
    let capture = audio.clone();
    let input_stream = input
        .build_input_stream(
            &input_config.config(),
            move |data: &[f32], _| {
                let mut pcm = Vec::with_capacity(crate::opus::FRAME_SAMPLES);
                for sample in data.iter().take(crate::opus::FRAME_SAMPLES) {
                    pcm.push((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16);
                }
                if pcm.len() < crate::opus::FRAME_SAMPLES {
                    return;
                }
                if let Ok(mut audio) = capture.lock() {
                    if let Ok(Some(packet)) = audio.on_microphone(&pcm) {
                        let _ = outbound.send(packet);
                    }
                }
            },
            tracing_error,
            None,
        )
        .map_err(|err| err.to_string())?;
    let playback = inbound.clone();
    let output_stream = output
        .build_output_stream(
            &output_config.config(),
            move |data: &mut [f32], _| {
                let mut queue = playback.lock().unwrap_or_else(|err| err.into_inner());
                for sample in data.iter_mut() {
                    let next = queue.pop_front().unwrap_or(0);
                    *sample = f32::from(next) / f32::from(i16::MAX);
                }
            },
            tracing_error,
            None,
        )
        .map_err(|err| err.to_string())?;
    input_stream.play().map_err(|err| err.to_string())?;
    output_stream.play().map_err(|err| err.to_string())?;
    Ok(AudioSession {
        _input: input_stream,
        _output: output_stream,
    })
}

/// Owns the cpal streams so the callbacks keep running.
#[cfg(feature = "cpal")]
pub struct AudioSession {
    _input: cpal::Stream,
    _output: cpal::Stream,
}

#[cfg(feature = "cpal")]
pub fn open_default_call_audio() -> Result<AudioSession, String> {
    let audio = std::sync::Arc::new(std::sync::Mutex::new(
        crate::CallAudio::new(1, &[9; 32]).map_err(|err| err.to_string())?,
    ));
    let (outbound, packets) = std::sync::mpsc::channel::<Vec<u8>>();
    let speaker = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
    let playback_audio = audio.clone();
    let playback_queue = speaker.clone();
    std::thread::spawn(move || {
        while let Ok(packet) = packets.recv() {
            let Ok(mut audio) = playback_audio.lock() else {
                continue;
            };
            let Ok(pcm) = audio.on_rtp(&packet, 0, 1) else {
                continue;
            };
            if let Ok(mut queue) = playback_queue.lock() {
                queue.extend(pcm);
            }
        }
    });
    attach_default_devices(audio, outbound, speaker)
}

#[cfg(not(feature = "cpal"))]
pub struct AudioSession;

#[cfg(not(feature = "cpal"))]
pub fn open_default_call_audio() -> Result<AudioSession, String> {
    Err("cpal is not enabled".into())
}

#[cfg(feature = "cpal")]
fn tracing_error(err: cpal::StreamError) {
    let _ = err;
}

#[cfg(test)]
mod device_tests {
    use super::*;

    #[test]
    fn device_lists_do_not_panic() {
        let _ = list_input_devices();
        let _ = list_output_devices();
        let _ = default_devices();
    }
}
