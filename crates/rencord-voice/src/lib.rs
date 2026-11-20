//! Voice receive and send path.
//!
//! The adaptive jitter buffer is original. [`fixed::FixedWindow`] is a small
//! baseline that reproduces the failure mode documented against a fixed 40 ms
//! window (eight reorder slots, three concealment frames, then a hard skip,
//! and a silent drop when the window is full). It exists so tests can show the
//! adaptive buffer keeps more audio under loss. It is not a copy of another
//! client's source.

pub mod crypto;
pub mod dave;
pub mod devices;
pub mod fixed;
pub mod jitter;
pub mod opus;
pub mod pipeline;
pub mod session;

pub use crypto::{TransportCipher, TRANSPORT_MODE};
pub use dave::{DaveEngine, LiveDave, ScriptedDave};
pub use devices::{backend_name, default_devices};
pub use jitter::{AdaptiveBuffer, AdaptiveConfig, JitterStats, Playout};
pub use pipeline::{loopback_call, LoopbackReport};
pub use session::{VoicePhase, VoiceSession};
