//! Big-file flow for attachments Discord will reject.
//!
//! Discord enforces the per-file cap on the server. This crate does not raise
//! that cap. It detects the cap from the account's premium type and, in a
//! guild, the boost tier (or from `attachment_size_limit` when the API sends
//! one), then plans three things the user can explicitly choose: re-encode
//! under the cap, upload to a host they picked and post the link, or split
//! the bytes into volumes a Rencord client can join again.
//!
//! Nothing here opens a socket unless [`run_job`] is called, and an external
//! upload returns [`UploadError::NeedsConsent`] until the caller passes a
//! confirmation. The work is synchronous so the desktop can run it on a
//! worker thread; do not call it on the UI thread.

mod encode;
mod ffmpeg;
mod flow;
mod limit;
mod plan;
mod provider;
mod sign;
mod split;
mod store;
mod transfer;

pub use encode::{encode_image_to_fit, encode_wav_opus, probe_path, EncodedImage, OpusEncode};
pub use ffmpeg::{
    estimate_encode_secs, ffmpeg_commands, find_ffmpeg, probe_encoders, select_encoder,
    EncoderChoice, FfmpegPlan,
};
pub use flow::{compressed_file_name, Flow, FlowChoice, Prepared, SheetScreen};
pub use limit::{
    detect_limit, format_bytes, limit_from_json, LimitInput, LimitSource, UploadLimit,
    FREE_LIMIT_BYTES, NITRO_BASIC_LIMIT_BYTES, NITRO_LIMIT_BYTES,
};
pub use plan::{
    estimate_image_bytes, estimate_volumes, estimated_video_bytes, offer, opus_estimate_bytes,
    plan_image, plan_opus, plan_video, precheck, video_bitrate_kbps, CompressOffer, ExternalOffer,
    ImageFormat, ImagePlan, MediaKind, MediaProbe, OpusPlan, Precheck, QualityPreference,
    SheetOffer, SplitOffer, StagedFile, VideoPlan, VideoSource,
};
pub use provider::{
    encode_multipart, upload_call, BodyPlan, CustomTarget, FieldPlan, Hosts, HttpCall,
    LitterboxTime, ProviderKind, UploadPlan,
};
pub use split::{
    format_note, parse_note, reassemble, split_bytes, ArchiveFormat, SplitBundle, SplitNote,
    HEADER_LEN,
};
pub use store::{
    append_history, load_history, load_settings, save_settings, scrub_saved_secret, HistoryEntry,
    UploadSettings,
};
pub use transfer::{run_job, JobOutput, JobSpec, ProgressSnap};

/// Failures from planning, encoding, splitting, or transferring.
#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("consent is required before sending anything to {host}")]
    NeedsConsent { host: String },
    #[error("cancelled")]
    Cancelled,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("http {status}: {body}")]
    Http { status: u16, body: String },
    #[error("image: {0}")]
    Image(#[from] image::ImageError),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("opus: {0}")]
    Opus(#[from] audiopus::Error),
    #[error("http client: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("{0}")]
    Message(String),
}

impl UploadError {
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub(crate) fn sha256_file(path: &std::path::Path) -> Result<[u8; 32], UploadError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Ok(out)
}
