//! Sheet state. The desktop renders this. It does not start transfers.

use crate::ffmpeg::EncoderChoice;
use crate::limit::{detect_limit, format_bytes, LimitInput, UploadLimit};
use crate::plan::{precheck, MediaKind, MediaProbe, Precheck, SheetOffer, StagedFile};
use crate::provider::{CustomTarget, ProviderKind};
use crate::split::{parse_note, SplitNote};
use crate::store::format_unix;
use crate::store::{DefaultAction, HistoryEntry, UploadSettings};
use crate::transfer::{JobSpec, ProgressSnap};

#[derive(Clone, Debug)]
pub enum Prepared {
    /// A message the UI can post. No bytes leave the machine.
    Note(String),
    Job(Box<JobSpec>),
}
use crate::UploadError;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetScreen {
    Pick,
    Choose,
    Progress,
    Done,
    Reassemble,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowChoice {
    Compress,
    External,
    Split,
}

impl FlowChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compress => "Compress",
            Self::External => "Upload a link",
            Self::Split => "Split",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Flow {
    pub open: bool,
    pub screen: SheetScreen,
    pub settings: UploadSettings,
    pub consent: bool,
    pub limit: UploadLimit,
    pub staged: Option<StagedFile>,
    pub probe: MediaProbe,
    pub source: Option<PathBuf>,
    pub offer: Option<SheetOffer>,
    pub chosen: Option<FlowChoice>,
    pub progress: Option<ProgressSnap>,
    pub outcome: Option<String>,
    pub error: Option<String>,
    pub history: Vec<HistoryEntry>,
    pub reassemble_note: String,
    pub part_paths: Vec<PathBuf>,
    pub encoder: EncoderChoice,
    /// The Defaults disclosure on the sheet. Closed until the user opens it.
    pub defaults_open: bool,
}

impl Default for Flow {
    fn default() -> Self {
        Self {
            open: false,
            screen: SheetScreen::Pick,
            settings: UploadSettings::default(),
            consent: false,
            limit: detect_limit(LimitInput::free_dm()),
            staged: None,
            probe: MediaProbe::default(),
            source: None,
            offer: None,
            chosen: None,
            progress: None,
            outcome: None,
            error: None,
            history: Vec::new(),
            reassemble_note: String::new(),
            part_paths: Vec::new(),
            encoder: EncoderChoice::software_fallback(),
            defaults_open: false,
        }
    }
}

impl Flow {
    pub fn close(&mut self) {
        self.open = false;
        self.error = None;
    }

    pub fn open_picker(&mut self) {
        self.open = true;
        self.screen = SheetScreen::Pick;
        self.error = None;
        self.outcome = None;
        self.consent = false;
    }

    pub fn stage(&mut self, file: StagedFile, probe: MediaProbe, source: Option<PathBuf>) {
        let decision = precheck(&file, &probe, self.limit, &self.settings, &self.encoder);
        self.staged = Some(file);
        self.probe = probe;
        self.source = source;
        self.consent = false;
        self.error = None;
        self.outcome = None;
        self.progress = None;
        match decision {
            Precheck::Fits { .. } => {
                self.offer = None;
                self.chosen = None;
                self.screen = SheetScreen::Choose;
            }
            Precheck::Over { offer, .. } => {
                self.chosen = match self.settings.default_action {
                    DefaultAction::Compress if offer.compress.available => {
                        Some(FlowChoice::Compress)
                    }
                    DefaultAction::External => Some(FlowChoice::External),
                    DefaultAction::Split => Some(FlowChoice::Split),
                    _ => None,
                };
                self.offer = Some(*offer);
                self.screen = SheetScreen::Choose;
            }
        }
        self.open = true;
    }

    pub fn stage_path(&mut self, path: &Path) -> Result<(), UploadError> {
        let (kind, probe, size) = crate::encode::probe_path(path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_owned();
        self.stage(
            StagedFile { name, size, kind },
            probe,
            Some(path.to_owned()),
        );
        Ok(())
    }

    pub fn set_limit(&mut self, limit: UploadLimit) {
        self.limit = limit;
        if let Some(file) = self.staged.clone() {
            let probe = self.probe;
            let source = self.source.clone();
            self.stage(file, probe, source);
        }
    }

    pub fn choose(&mut self, choice: FlowChoice) {
        if choice == FlowChoice::Compress {
            if let Some(offer) = &self.offer {
                if !offer.compress.available && self.over_limit() {
                    self.error = Some(offer.compress.detail.clone());
                    return;
                }
            }
        }
        self.chosen = Some(choice);
        self.error = None;
        if choice != FlowChoice::External {
            self.consent = false;
        }
    }

    pub fn toggle_consent(&mut self) {
        self.consent = !self.consent;
    }

    pub fn over_limit(&self) -> bool {
        self.staged
            .as_ref()
            .is_some_and(|file| file.size > self.limit.bytes)
    }

    pub fn toggle_defaults(&mut self) {
        self.defaults_open = !self.defaults_open;
    }

    pub fn cycle_default(&mut self) {
        self.settings.default_action = self.settings.default_action.cycle();
    }

    pub fn cycle_provider(&mut self) {
        self.settings.provider = self.settings.provider.cycle();
        self.consent = false;
        self.refresh_offer();
    }

    pub fn cycle_quality(&mut self) {
        self.settings.quality = self.settings.quality.cycle();
        self.refresh_offer();
    }

    pub fn cycle_expiry(&mut self) {
        self.settings.cycle_expiry();
        self.refresh_offer();
    }

    pub fn cycle_archive(&mut self) {
        self.settings.archive = self.settings.archive.cycle();
        self.refresh_offer();
    }

    pub fn set_custom_spec(&mut self, spec: &str) {
        self.settings.custom_spec = spec.trim().to_owned();
    }

    fn refresh_offer(&mut self) {
        let Some(file) = self.staged.clone() else {
            return;
        };
        if file.size <= self.limit.bytes {
            self.offer = None;
            return;
        }
        if let Precheck::Over { offer, .. } = precheck(
            &file,
            &self.probe,
            self.limit,
            &self.settings,
            &self.encoder,
        ) {
            self.offer = Some(*offer);
        }
    }

    /// Build the worker job. External uploads fail here, before `run_job`,
    /// when the warning has not been confirmed.
    pub fn prepare_job(&mut self, work_dir: &Path) -> Result<Prepared, UploadError> {
        self.error = None;
        let file = self
            .staged
            .clone()
            .ok_or_else(|| UploadError::message("choose a file first"))?;
        if !self.over_limit() {
            return Ok(Prepared::Note(format!(
                "Attached {} ({}, under the {} cap).",
                file.name,
                format_bytes(file.size),
                format_bytes(self.limit.bytes)
            )));
        }
        let source = self.source.clone().ok_or_else(|| {
            UploadError::message("that file is not on disk. Drop it or type its path.")
        })?;
        let choice = self
            .chosen
            .ok_or_else(|| UploadError::message("pick compress, a link, or split"))?;
        let spec = match choice {
            FlowChoice::Compress => JobSpec::Compress {
                path: source,
                dest: work_dir.join(compressed_name(&file)),
                limit: self.limit.bytes,
                quality: self.settings.quality,
            },
            FlowChoice::External => {
                if !self.consent {
                    let host = self.settings.provider.host().to_owned();
                    self.error = Some(format!(
                        "Confirm the warning before anything is sent to {host}."
                    ));
                    return Err(UploadError::NeedsConsent { host });
                }
                let custom = if self.settings.provider == ProviderKind::Custom {
                    Some(CustomTarget::parse(&self.settings.custom_spec)?)
                } else {
                    None
                };
                JobSpec::External {
                    path: source,
                    plan: crate::provider::UploadPlan {
                        provider: self.settings.provider,
                        hosts: crate::provider::Hosts::default(),
                        litterbox: self.settings.litterbox,
                        zero_x_hours: self.settings.zero_x_hours,
                        custom,
                        file_name: file.name,
                    },
                    consent: true,
                    resume_dir: work_dir.join("resume"),
                    history_path: work_dir.join("upload-history.json"),
                }
            }
            FlowChoice::Split => JobSpec::Split {
                path: source,
                dest_dir: work_dir.join("parts"),
                limit: self.limit.bytes,
                format: self.settings.archive,
            },
        };
        self.screen = SheetScreen::Progress;
        self.progress = Some(ProgressSnap {
            phase: "Starting".into(),
            done: 0,
            total: 1,
            detail: choice.label().into(),
        });
        Ok(Prepared::Job(Box::new(spec)))
    }

    pub fn apply_progress(&mut self, snap: ProgressSnap) {
        self.screen = SheetScreen::Progress;
        self.progress = Some(snap);
    }

    pub fn succeed(&mut self, text: String) {
        self.screen = SheetScreen::Done;
        self.outcome = Some(text);
        self.error = None;
        self.progress = None;
    }

    pub fn fail(&mut self, text: String) {
        self.screen = SheetScreen::Progress;
        self.error = Some(text);
    }

    pub fn can_retry(&self) -> bool {
        self.error.is_some()
    }

    pub fn open_reassemble(&mut self, note: &str) {
        self.open = true;
        self.screen = SheetScreen::Reassemble;
        self.reassemble_note = note.to_owned();
        self.error = None;
    }

    pub fn parsed_note(&self) -> Option<SplitNote> {
        parse_note(&self.reassemble_note)
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.open {
            lines.push("big-file: closed".into());
            return lines;
        }
        lines.push("big-file: open".into());
        lines.push(format!("screen: {}", screen_name(self.screen)));
        if let Some(file) = &self.staged {
            lines.push(format!("file: {}", file.name));
            lines.push(format!("size: {}", file.size));
        }
        lines.push(format!("limit: {}", self.limit.bytes));
        lines.push(format!("account: {}", self.limit.label()));
        if let Some(offer) = &self.offer {
            lines.push(format!("meta: {}", offer.meta));
            lines.push(format!("server: {}", offer.server_note));
            lines.push(format!("compress: {}", offer.compress.detail));
            lines.push(format!("external: {}", offer.external.detail));
            lines.push(format!("warning: {}", offer.external.warning));
            lines.push(format!("split: {}", offer.split.detail));
        }
        if let Some(choice) = self.chosen {
            lines.push(format!("choice: {}", choice.label()));
        }
        lines.push(format!("consent: {}", self.consent));
        lines.push(format!(
            "settings: {} · {} · {}",
            self.settings.default_action.label(),
            self.settings.provider.label(),
            self.settings.quality.label()
        ));
        if let Some(progress) = &self.progress {
            lines.push(format!(
                "progress: {} {}/{} {}",
                progress.phase, progress.done, progress.total, progress.detail
            ));
        }
        if let Some(error) = &self.error {
            lines.push(format!("error: {error}"));
        }
        if let Some(outcome) = &self.outcome {
            lines.push(format!("outcome: {outcome}"));
        }
        for entry in self.history.iter().take(3) {
            lines.push(format!(
                "history: {} {} {}",
                format_unix(entry.at_unix),
                entry.provider,
                entry.url
            ));
        }
        lines
    }

    pub fn preview_sheet() -> Self {
        let mut flow = Self {
            encoder: EncoderChoice {
                name: "h264_nvenc".into(),
                hardware: true,
                two_pass: true,
            },
            ..Self::default()
        };
        flow.stage(
            StagedFile {
                name: "lecture.mp4".into(),
                size: 48_000_000,
                kind: MediaKind::Video,
            },
            MediaProbe {
                duration_ms: Some(90_000),
                width: Some(1920),
                height: Some(1080),
                fps: Some(30),
            },
            None,
        );
        flow.choose(FlowChoice::Compress);
        flow
    }

    pub fn preview_upload() -> Self {
        let mut flow = Self::preview_sheet();
        flow.choose(FlowChoice::External);
        flow.screen = SheetScreen::Progress;
        flow.progress = Some(ProgressSnap {
            phase: "Uploading".into(),
            done: 18_600_000,
            total: 48_000_000,
            detail: "litterbox.catbox.moe".into(),
        });
        flow.error = None;
        flow
    }

    pub fn preview_error() -> Self {
        let mut flow = Self::preview_upload();
        flow.fail("Litterbox did not accept the file. Nothing was posted.".into());
        flow
    }

    pub fn preview_progress() -> Self {
        let mut flow = Self::preview_sheet();
        flow.screen = SheetScreen::Progress;
        flow.progress = Some(ProgressSnap {
            phase: "Compressing".into(),
            done: 12_400_000,
            total: 48_000_000,
            detail: "pass 1 of 2 · h264_nvenc · about 28 s left".into(),
        });
        flow.error = None;
        flow
    }
}

fn screen_name(screen: SheetScreen) -> &'static str {
    match screen {
        SheetScreen::Pick => "pick",
        SheetScreen::Choose => "choose",
        SheetScreen::Progress => "progress",
        SheetScreen::Done => "done",
        SheetScreen::Reassemble => "reassemble",
    }
}

/// `lecture.mp4` becomes `lecture-compressed.mp4`. The container stays when the
/// encoder can write it; the old extension is not stacked on the new one.
pub fn compressed_file_name(name: &str, kind: MediaKind) -> String {
    let (stem, ext) = file_stem(name);
    let container = match kind {
        MediaKind::Video => match ext.as_str() {
            "mp4" | "mkv" | "webm" | "mov" | "m4v" => ext,
            _ => "mp4".to_owned(),
        },
        MediaKind::Image => match ext.as_str() {
            "jpg" | "jpeg" => "jpg".to_owned(),
            "png" | "webp" | "gif" | "avif" => "webp".to_owned(),
            _ => "webp".to_owned(),
        },
        MediaKind::Audio => match ext.as_str() {
            "opus" | "ogg" => ext,
            _ => "opus".to_owned(),
        },
        MediaKind::Other => return format!("{stem}-compressed"),
    };
    format!("{stem}-compressed.{container}")
}

fn file_stem(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
            (stem.to_owned(), ext.to_ascii_lowercase())
        }
        _ => (name.to_owned(), String::new()),
    }
}

fn compressed_name(file: &StagedFile) -> String {
    compressed_file_name(&file.name, file.kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split::ArchiveFormat;

    #[test]
    fn over_limit_sheet_names_the_server_cap_and_blocks_unconfirmed_upload() {
        let mut flow = Flow::preview_sheet();
        let text = flow.lines().join("\n");
        assert!(text.contains("big-file: open"));
        assert!(text.contains("lecture.mp4"));
        assert!(text.contains("Discord checks the size on the server"));
        assert!(text.contains("does not send"));
        assert!(text.contains("settings: Ask · Litterbox · Balanced"));
        let error = flow.prepare_job(Path::new("/tmp")).unwrap_err();
        assert!(
            matches!(error, UploadError::NeedsConsent { .. })
                || error.to_string().contains("not on disk")
                || matches!(error, UploadError::Message(_))
        );
        flow.choose(FlowChoice::External);
        flow.source = Some(PathBuf::from("/tmp/lecture.mp4"));
        let refused = flow.prepare_job(Path::new("/tmp")).unwrap_err();
        assert!(matches!(refused, UploadError::NeedsConsent { .. }));
        assert!(flow.lines().join("\n").contains("Confirm the warning"));
        flow.consent = true;
        flow.settings.provider = ProviderKind::Litterbox;
        // The file is not real; prepare_job only checks the path is Some.
        // External with consent builds a job and does not touch the network.
        let job = flow.prepare_job(Path::new("/tmp")).unwrap();
        assert!(matches!(
            job,
            Prepared::Job(job) if matches!(job.as_ref(), JobSpec::External { consent: true, .. })
        ));
    }

    #[test]
    fn compressed_name_keeps_the_container_and_drops_the_old_extension() {
        assert_eq!(
            compressed_file_name("lecture.mp4", MediaKind::Video),
            "lecture-compressed.mp4"
        );
        assert_eq!(
            compressed_file_name("clip.MKV", MediaKind::Video),
            "clip-compressed.mkv"
        );
        assert_eq!(
            compressed_file_name("photo.png", MediaKind::Image),
            "photo-compressed.webp"
        );
        assert_eq!(
            compressed_file_name("voice.wav", MediaKind::Audio),
            "voice-compressed.opus"
        );
    }

    #[test]
    fn default_action_preselects_without_sending() {
        let mut flow = Flow::default();
        flow.settings.default_action = DefaultAction::Split;
        flow.settings.archive = ArchiveFormat::Zip;
        flow.stage(
            StagedFile {
                name: "dump.bin".into(),
                size: 40_000_000,
                kind: MediaKind::Other,
            },
            MediaProbe::default(),
            None,
        );
        assert_eq!(flow.chosen, Some(FlowChoice::Split));
        assert!(flow.lines().iter().any(|line| line.starts_with("split:")));
    }
}
