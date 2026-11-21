//! Voice receive and send path.
//!
//! The adaptive jitter buffer is original. [`fixed::FixedWindow`] is a small
//! baseline that reproduces the failure mode documented against a fixed 40 ms
//! window (eight reorder slots, three concealment frames, then a hard skip,
//! and a silent drop when the window is full). It exists so tests can show the
//! adaptive buffer keeps more audio under loss. It is not a copy of another
//! client's source.

pub mod connection;
pub mod crypto;
pub mod dave;
pub mod devices;
pub mod discovery;
pub mod fixed;
pub mod jitter;
pub mod mixer;
pub mod opus;
pub mod pipeline;
pub mod session;

pub use connection::{CallAudio, LiveCall, VoiceLinkError, VoiceTarget};
pub use crypto::{TransportCipher, TRANSPORT_MODE};
pub use dave::{DaveEngine, LiveDave, ScriptedDave};
pub use devices::{
    backend_name, choose_device, default_devices, list_input_devices, list_output_devices,
    open_call_audio, open_default_call_audio, AudioSession, DeviceChoice,
};
pub use discovery::{
    bind_for, discover, keepalive_packet, server_addr, DiscoveredAddress, DiscoveryError,
    DiscoverySchedule, MediaRole, PathWatch, DISCOVERY_HINT, KEEPALIVE_INTERVAL, PATH_DEAD_AFTER,
    SIO_UDP_CONNRESET,
};
pub use jitter::{AdaptiveBuffer, AdaptiveConfig, JitterStats, Playout};
pub use pipeline::{loopback_call, LoopbackReport};
pub use session::{VoicePhase, VoiceSession};
