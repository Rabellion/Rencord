//! Blocking transfers. Call [`run_job`] from a worker thread, not the UI thread.
//!
//! An external upload checks consent before the HTTP client is built. Cancel
//! is read between body chunks and between S3 parts. Completed S3 parts are
//! written beside the job so a later run skips them.

use crate::encode::{encode_image_to_fit, encode_wav_opus, probe_path, write_bytes};
use crate::ffmpeg::{self, FfmpegPlan};
use crate::plan::{plan_opus, plan_video, MediaKind, MediaProbe, QualityPreference, VideoSource};
use crate::provider::{
    encode_multipart, file_name_of, upload_call, BodyPlan, FieldPlan, HttpCall, OwnedField,
    ProviderKind, S3Recipe, UploadPlan, UploadRecipe, USER_AGENT,
};
use crate::split::{self, ArchiveFormat};
use crate::store::{append_history, HistoryEntry};
use crate::{hex_encode, sha256_file, UploadError};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressSnap {
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub detail: String,
}

impl ProgressSnap {
    pub fn percent(&self) -> u32 {
        self.done
            .saturating_mul(100)
            .checked_div(self.total)
            .unwrap_or(0)
            .min(100) as u32
    }
}

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum JobSpec {
    Compress {
        path: PathBuf,
        dest: PathBuf,
        limit: u64,
        quality: QualityPreference,
    },
    External {
        path: PathBuf,
        plan: UploadPlan,
        /// Must be true. False returns before any socket is opened.
        consent: bool,
        resume_dir: PathBuf,
        history_path: PathBuf,
    },
    Split {
        path: PathBuf,
        dest_dir: PathBuf,
        limit: u64,
        format: ArchiveFormat,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobOutput {
    File {
        path: PathBuf,
        detail: String,
    },
    Link {
        url: String,
        provider: String,
        bytes: u64,
        file_name: String,
        expires: Option<String>,
    },
    Split {
        note: String,
        files: Vec<PathBuf>,
    },
}

type Report = Arc<Mutex<dyn FnMut(ProgressSnap) + Send>>;

pub fn run_job(
    job: JobSpec,
    cancel: &Arc<AtomicBool>,
    progress: impl FnMut(ProgressSnap) + Send + 'static,
) -> Result<JobOutput, UploadError> {
    let progress: Report = Arc::new(Mutex::new(progress));
    if cancel.load(Ordering::Relaxed) {
        return Err(UploadError::Cancelled);
    }
    match job {
        JobSpec::Compress {
            path,
            dest,
            limit,
            quality,
        } => compress(&path, &dest, limit, quality, cancel, &progress),
        JobSpec::External {
            path,
            plan,
            consent,
            resume_dir,
            history_path,
        } => upload_external(
            &path,
            &plan,
            consent,
            &resume_dir,
            &history_path,
            cancel,
            &progress,
        ),
        JobSpec::Split {
            path,
            dest_dir,
            limit,
            format,
        } => split_to_dir(&path, &dest_dir, limit, format, cancel, &progress),
    }
}

fn emit(progress: &Report, snap: ProgressSnap) {
    if let Ok(mut callback) = progress.lock() {
        callback(snap);
    }
}

fn compress(
    path: &Path,
    dest: &Path,
    limit: u64,
    quality: QualityPreference,
    cancel: &Arc<AtomicBool>,
    progress: &Report,
) -> Result<JobOutput, UploadError> {
    let (kind, probe, size) = probe_path(path)?;
    emit(
        progress,
        ProgressSnap {
            phase: "Reading".into(),
            done: 0,
            total: size,
            detail: file_name_of(path),
        },
    );
    check_cancel(cancel)?;
    match kind {
        MediaKind::Image => {
            let image = image::ImageReader::open(path)?
                .with_guessed_format()?
                .decode()?;
            emit(
                progress,
                ProgressSnap {
                    phase: "Compressing".into(),
                    done: 1,
                    total: 2,
                    detail: "resize and re-encode".into(),
                },
            );
            let encoded = encode_image_to_fit(&image, limit, quality)?;
            write_bytes(dest, &encoded.bytes)?;
            Ok(JobOutput::File {
                path: dest.to_owned(),
                detail: format!(
                    "{} {}×{} q{} · {} bytes",
                    encoded.format.label(),
                    encoded.width,
                    encoded.height,
                    encoded.quality,
                    encoded.bytes.len()
                ),
            })
        }
        MediaKind::Audio => compress_audio(path, dest, limit, &probe, cancel, progress),
        MediaKind::Video => compress_video(path, dest, limit, quality, &probe, cancel, progress),
        MediaKind::Other => Err(UploadError::message(
            "no encoder for this file type. Split it, or upload a link.",
        )),
    }
}

fn compress_audio(
    path: &Path,
    dest: &Path,
    limit: u64,
    probe: &MediaProbe,
    cancel: &Arc<AtomicBool>,
    progress: &Report,
) -> Result<JobOutput, UploadError> {
    let duration = probe.duration_ms.unwrap_or(0);
    let opus = plan_opus(duration, limit);
    if !opus.fits {
        return Err(UploadError::message(
            "Opus cannot fit this audio under the cap",
        ));
    }
    emit(
        progress,
        ProgressSnap {
            phase: "Compressing".into(),
            done: 0,
            total: duration.max(1),
            detail: format!("Opus {} kbps", opus.bitrate_kbps),
        },
    );
    check_cancel(cancel)?;
    let wav = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"));
    if wav {
        let bytes = fs::read(path)?;
        let encoded = encode_wav_opus(&bytes, limit)?;
        let mut packed = Vec::new();
        for packet in &encoded.packets {
            let len = u16::try_from(packet.len()).unwrap_or(u16::MAX);
            packed.extend_from_slice(&len.to_be_bytes());
            packed.extend_from_slice(packet);
        }
        write_bytes(dest, &packed)?;
        return Ok(JobOutput::File {
            path: dest.to_owned(),
            detail: format!(
                "Opus {} kbps · {} payload bytes (length-prefixed packets)",
                encoded.bitrate_kbps, encoded.payload_bytes
            ),
        });
    }
    let ffmpeg = ffmpeg::find_ffmpeg().ok_or_else(|| {
        UploadError::message("this audio needs ffmpeg with libopus, and ffmpeg is not on PATH")
    })?;
    let args = [
        "-y",
        "-i",
        &path.display().to_string(),
        "-c:a",
        "libopus",
        "-b:a",
        &format!("{}k", opus.bitrate_kbps.max(6)),
        &dest.display().to_string(),
    ];
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    run_command(&ffmpeg, &owned, cancel)?;
    Ok(JobOutput::File {
        path: dest.to_owned(),
        detail: format!("ffmpeg libopus {} kbps", opus.bitrate_kbps),
    })
}

fn compress_video(
    path: &Path,
    dest: &Path,
    limit: u64,
    quality: QualityPreference,
    probe: &MediaProbe,
    cancel: &Arc<AtomicBool>,
    progress: &Report,
) -> Result<JobOutput, UploadError> {
    let ffmpeg_bin = ffmpeg::find_ffmpeg().ok_or_else(|| {
        UploadError::message(
            "ffmpeg is not installed, so the video was not re-encoded. Split and the external link still work.",
        )
    })?;
    let listing = ffmpeg::probe_encoders(&ffmpeg_bin).unwrap_or_default();
    let encoder = ffmpeg::select_encoder(&listing);
    let duration = probe.duration_ms.ok_or_else(|| {
        UploadError::message("could not read a duration, so the target bitrate is unknown")
    })?;
    let source = VideoSource {
        width: probe.width.unwrap_or(1920),
        height: probe.height.unwrap_or(1080),
        fps: probe.fps.unwrap_or(30),
        duration_ms: duration,
    };
    let plan = plan_video(source, limit, quality, &encoder);
    if !plan.fits {
        return Err(UploadError::message(
            "the target-size plan cannot fit this video under the cap",
        ));
    }
    let passlog = dest.with_extension("passlog");
    let commands = ffmpeg::ffmpeg_commands(
        &ffmpeg_bin,
        path,
        dest,
        &FfmpegPlan {
            encoder: encoder.clone(),
            width: plan.width,
            height: plan.height,
            video_kbps: plan.video_kbps,
            audio_kbps: plan.audio_kbps,
            scale: plan.width != source.width || plan.height != source.height,
        },
        &passlog,
    );
    for (index, command) in commands.iter().enumerate() {
        check_cancel(cancel)?;
        emit(
            progress,
            ProgressSnap {
                phase: "Compressing".into(),
                done: index as u64,
                total: commands.len() as u64,
                detail: format!(
                    "pass {} of {} · {} · {}×{}",
                    index + 1,
                    commands.len(),
                    encoder.name,
                    plan.width,
                    plan.height
                ),
            },
        );
        run_command(Path::new(&command[0]), &command[1..], cancel)?;
    }
    Ok(JobOutput::File {
        path: dest.to_owned(),
        detail: format!(
            "{} {}×{} · ~{} bytes planned",
            encoder.name, plan.width, plan.height, plan.estimated_bytes
        ),
    })
}

fn run_command(
    program: &Path,
    args: &[String],
    cancel: &Arc<AtomicBool>,
) -> Result<(), UploadError> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|err| {
            UploadError::message(format!("could not start {}: {err}", program.display()))
        })?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(UploadError::Cancelled);
        }
        match child.try_wait()? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(UploadError::message(format!(
                    "{} exited {status}",
                    program.display()
                )))
            }
            None => std::thread::sleep(Duration::from_millis(40)),
        }
    }
}

fn split_to_dir(
    path: &Path,
    dest_dir: &Path,
    limit: u64,
    format: ArchiveFormat,
    cancel: &Arc<AtomicBool>,
    progress: &Report,
) -> Result<JobOutput, UploadError> {
    let bytes = fs::read(path)?;
    check_cancel(cancel)?;
    emit(
        progress,
        ProgressSnap {
            phase: "Splitting".into(),
            done: 0,
            total: bytes.len() as u64,
            detail: format.label().into(),
        },
    );
    let bundle = split::split_bytes(&file_name_of(path), &bytes, limit, format)?;
    fs::create_dir_all(dest_dir)?;
    let mut files = Vec::new();
    for (index, (volume, name)) in bundle
        .volumes
        .iter()
        .zip(bundle.filenames.iter())
        .enumerate()
    {
        check_cancel(cancel)?;
        let output = dest_dir.join(name);
        write_bytes(&output, volume)?;
        files.push(output);
        emit(
            progress,
            ProgressSnap {
                phase: "Splitting".into(),
                done: (index as u64) + 1,
                total: bundle.volumes.len() as u64,
                detail: name.clone(),
            },
        );
    }
    write_bytes(
        &dest_dir.join("reassembly.txt"),
        bundle.note_text.as_bytes(),
    )?;
    Ok(JobOutput::Split {
        note: bundle.note_text,
        files,
    })
}

fn upload_external(
    path: &Path,
    plan: &UploadPlan,
    consent: bool,
    resume_dir: &Path,
    history_path: &Path,
    cancel: &Arc<AtomicBool>,
    progress: &Report,
) -> Result<JobOutput, UploadError> {
    let host = plan.provider.host().to_owned();
    if !consent {
        return Err(UploadError::NeedsConsent { host });
    }
    let recipe = upload_call(plan)?;
    let client = http_client()?;
    let url = match recipe {
        UploadRecipe::OneShot(call) => {
            let (content_type, body) = match &call.body {
                BodyPlan::RawFile { .. } => {
                    let bytes = fs::read(path)?;
                    ("application/octet-stream".to_owned(), bytes)
                }
                _ => multipart_from_file(&call, path)?,
            };
            send_with_progress(
                &client,
                &call,
                &body,
                &content_type,
                cancel,
                progress,
                &host,
            )?
        }
        UploadRecipe::S3(recipe) => {
            s3_upload(&client, path, &recipe, resume_dir, cancel, progress)?
        }
    };
    let bytes = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let expires = expiry_of(plan);
    let _ = append_history(
        history_path,
        HistoryEntry {
            at_unix: unix_now(),
            provider: plan.provider.label().to_owned(),
            file_name: file_name_of(path),
            url: url.clone(),
            expires: Some(expires.clone()),
            bytes,
        },
    );
    Ok(JobOutput::Link {
        url,
        provider: plan.provider.label().to_owned(),
        bytes,
        file_name: file_name_of(path),
        expires: Some(expires),
    })
}

fn expiry_of(plan: &UploadPlan) -> String {
    match plan.provider {
        ProviderKind::Litterbox => plan.litterbox.as_api().to_owned(),
        ProviderKind::ZeroX => plan
            .zero_x_hours
            .map(|hours| format!("{hours}h"))
            .unwrap_or_else(|| "none".into()),
        ProviderKind::Catbox => "none".into(),
        ProviderKind::Custom => "endpoint".into(),
    }
}

fn multipart_from_file(call: &HttpCall, path: &Path) -> Result<(String, Vec<u8>), UploadError> {
    let BodyPlan::Multipart(fields) = &call.body else {
        return Err(UploadError::message("expected a multipart body"));
    };
    let file_bytes = fs::read(path)?;
    let mut owned = Vec::new();
    for field in fields {
        match field {
            FieldPlan::Text { name, value } => owned.push(OwnedField {
                name: name.clone(),
                filename: None,
                value: value.as_bytes().to_vec(),
            }),
            FieldPlan::File { name, filename } => owned.push(OwnedField {
                name: name.clone(),
                filename: Some(filename.clone()),
                value: file_bytes.clone(),
            }),
        }
    }
    let boundary = format!("rencordBoundary{}", unix_now());
    let (content_type, bytes) = encode_multipart(&boundary, &owned);
    Ok((content_type, bytes))
}

fn send_with_progress(
    client: &reqwest::blocking::Client,
    call: &HttpCall,
    body: &[u8],
    content_type: &str,
    cancel: &Arc<AtomicBool>,
    progress: &Report,
    detail: &str,
) -> Result<String, UploadError> {
    let total = body.len() as u64;
    let progress_cb = progress.clone();
    let detail = detail.to_owned();
    let reader = ArcReader {
        inner: io::Cursor::new(body.to_vec()),
        sent: 0,
        total,
        cancel: cancel.clone(),
        report_every: 64 * 1024,
        last_report: 0,
        on_progress: move |done, total| {
            emit(
                &progress_cb,
                ProgressSnap {
                    phase: "Uploading".into(),
                    done,
                    total,
                    detail: detail.clone(),
                },
            );
        },
    };
    let response = dispatch(client, call, content_type, total, reader)?;
    finish_response(response)
}

fn dispatch<R: Read + Send + 'static>(
    client: &reqwest::blocking::Client,
    call: &HttpCall,
    content_type: &str,
    length: u64,
    reader: R,
) -> Result<reqwest::blocking::Response, UploadError> {
    let method = reqwest::Method::from_bytes(call.method.as_str().as_bytes())
        .map_err(|err| UploadError::message(err.to_string()))?;
    let mut request = client.request(method, &call.url);
    for (name, value) in &call.headers {
        if name.eq_ignore_ascii_case("content-type") || name.eq_ignore_ascii_case("content-length")
        {
            continue;
        }
        request = request.header(name, value);
    }
    request = request
        .header("Content-Type", content_type)
        .header("Content-Length", length.to_string())
        .header("User-Agent", USER_AGENT);
    match request.body(reqwest::blocking::Body::new(reader)).send() {
        Ok(response) => Ok(response),
        Err(err) if err.to_string().contains("cancelled") => Err(UploadError::Cancelled),
        Err(err) => Err(err.into()),
    }
}

fn finish_response(response: reqwest::blocking::Response) -> Result<String, UploadError> {
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(UploadError::Http {
            status: status.as_u16(),
            body,
        });
    }
    Ok(body.trim().to_owned())
}

fn s3_upload(
    client: &reqwest::blocking::Client,
    path: &Path,
    recipe: &S3Recipe,
    resume_dir: &Path,
    cancel: &Arc<AtomicBool>,
    progress: &Report,
) -> Result<String, UploadError> {
    fs::create_dir_all(resume_dir)?;
    let digest = hex_encode(&sha256_file(path)?);
    let resume_path = resume_dir.join(format!("resume-{digest}.json"));
    let file_len = fs::metadata(path)?.len();
    let mut state = if let Some(existing) = load_resume(&resume_path, &digest, file_len) {
        existing
    } else {
        let call = recipe.initiate();
        let response = dispatch(client, &call, "application/xml", 0, io::empty())?;
        let status = response.status();
        let body = response.text()?;
        if !status.is_success() {
            return Err(UploadError::Http {
                status: status.as_u16(),
                body,
            });
        }
        let upload_id = xml_tag(&body, "UploadId")
            .ok_or_else(|| UploadError::message("S3 initiate response had no UploadId"))?;
        ResumeState {
            upload_id,
            part_size: recipe.part_size,
            file_len,
            file_sha256: digest,
            parts: Vec::new(),
            object_url: recipe.object_url.clone(),
        }
    };
    let mut offset = 0u64;
    let mut number = 1u32;
    let mut file = File::open(path)?;
    while offset < file_len {
        check_cancel(cancel)?;
        let end = (offset + state.part_size).min(file_len);
        if state.parts.iter().any(|part| part.number == number) {
            offset = end;
            number += 1;
            emit(
                progress,
                ProgressSnap {
                    phase: "Uploading".into(),
                    done: offset,
                    total: file_len,
                    detail: "resumed".into(),
                },
            );
            continue;
        }
        let len = (end - offset) as usize;
        let mut payload = vec![0u8; len];
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut payload)?;
        let call = recipe.part(number, &state.upload_id, &payload);
        let response = dispatch(
            client,
            &call,
            "application/octet-stream",
            payload.len() as u64,
            io::Cursor::new(payload),
        )?;
        let status = response.status();
        let etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("\"part\"")
            .to_owned();
        let body = response.text().unwrap_or_default();
        if !status.is_success() {
            save_resume(&resume_path, &state)?;
            return Err(UploadError::Http {
                status: status.as_u16(),
                body,
            });
        }
        state.parts.push(DonePart { number, etag });
        save_resume(&resume_path, &state)?;
        offset = end;
        number += 1;
        emit(
            progress,
            ProgressSnap {
                phase: "Uploading".into(),
                done: offset,
                total: file_len,
                detail: format!("part {}", number - 1),
            },
        );
    }
    state.parts.sort_by_key(|part| part.number);
    let pairs: Vec<(u32, String)> = state
        .parts
        .iter()
        .map(|part| (part.number, part.etag.clone()))
        .collect();
    let call = recipe.complete(&state.upload_id, &pairs);
    let xml = match &call.body {
        BodyPlan::Xml(xml) => xml.clone(),
        _ => String::new(),
    };
    let response = dispatch(
        client,
        &call,
        "application/xml",
        xml.len() as u64,
        io::Cursor::new(xml.into_bytes()),
    )?;
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(UploadError::Http {
            status: status.as_u16(),
            body,
        });
    }
    let _ = fs::remove_file(&resume_path);
    Ok(recipe.object_url.clone())
}

fn http_client() -> Result<reqwest::blocking::Client, UploadError> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .pool_max_idle_per_host(0)
        .build()?)
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), UploadError> {
    if cancel.load(Ordering::Relaxed) {
        Err(UploadError::Cancelled)
    } else {
        Ok(())
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn xml_tag(body: &str, tag: &str) -> Option<String> {
    let start = format!("<{tag}>");
    let end = format!("</{tag}>");
    let from = body.find(&start)? + start.len();
    let to = body[from..].find(&end)? + from;
    Some(body[from..to].trim().to_owned())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ResumeState {
    upload_id: String,
    part_size: u64,
    file_len: u64,
    file_sha256: String,
    parts: Vec<DonePart>,
    object_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DonePart {
    number: u32,
    etag: String,
}

fn load_resume(path: &Path, digest: &str, file_len: u64) -> Option<ResumeState> {
    let text = fs::read_to_string(path).ok()?;
    let state: ResumeState = serde_json::from_str(&text).ok()?;
    (state.file_sha256 == digest && state.file_len == file_len).then_some(state)
}

fn save_resume(path: &Path, state: &ResumeState) -> Result<(), UploadError> {
    let body = serde_json::to_string(state)
        .map_err(|err| UploadError::message(format!("resume JSON: {err}")))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, body)?;
    Ok(())
}

struct ArcReader<R, F> {
    inner: R,
    sent: u64,
    total: u64,
    cancel: Arc<AtomicBool>,
    report_every: u64,
    last_report: u64,
    on_progress: F,
}

impl<R: Read, F: Fn(u64, u64)> Read for ArcReader<R, F> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            // Not `Interrupted`: std::io::copy retries that forever.
            return Err(io::Error::other("cancelled"));
        }
        let read = self.inner.read(buf)?;
        self.sent += read as u64;
        if read > 0
            && (self.sent - self.last_report >= self.report_every || self.sent >= self.total)
        {
            self.last_report = self.sent;
            (self.on_progress)(self.sent, self.total);
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Hosts, LitterboxTime};

    #[test]
    fn reader_stops_after_cancel() {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let mut reader = ArcReader {
            inner: io::Cursor::new(vec![7u8; 1000]),
            sent: 0,
            total: 1000,
            cancel,
            report_every: 1,
            last_report: 0,
            on_progress: move |done, _| {
                if done >= 64 {
                    flag.store(true, Ordering::Relaxed);
                }
            },
        };
        let mut out = Vec::new();
        let error = io::copy(&mut reader, &mut out).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("cancelled"));
        assert!(out.len() < 1000);
        assert!(out.len() >= 64);
    }

    #[test]
    fn refusing_consent_does_not_open_a_socket() {
        let progress: Report = Arc::new(Mutex::new(|_| {}));
        let error = upload_external(
            Path::new("missing.bin"),
            &UploadPlan {
                provider: ProviderKind::Litterbox,
                hosts: Hosts::default(),
                litterbox: LitterboxTime::H24,
                zero_x_hours: None,
                custom: None,
                file_name: "missing.bin".into(),
            },
            false,
            Path::new("/tmp"),
            Path::new("/tmp/rencord-history-unused.json"),
            &Arc::new(AtomicBool::new(false)),
            &progress,
        )
        .unwrap_err();
        assert!(matches!(error, UploadError::NeedsConsent { .. }));
    }
}
