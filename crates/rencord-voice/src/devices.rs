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
