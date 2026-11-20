//! The live call task. It runs on the shared network runtime for the whole
//! call: heartbeats, DAVE frames, microphone frames, and speaker frames.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rencord_voice::dave::{DaveEngine, LiveDave};
use rencord_voice::{LiveCall, VoiceLinkError, VoiceTarget};
use tokio::sync::mpsc;

pub enum VoiceCmd {
    Mute(bool),
    Deaf(bool),
    Leave,
    Devices {
        input: Option<String>,
        output: Option<String>,
    },
}

pub struct CallRequest {
    pub target: VoiceTarget,
    pub user_id: u64,
    pub channel_id: u64,
    pub commands: mpsc::Receiver<VoiceCmd>,
    pub status: std::sync::mpsc::Sender<String>,
    pub server: String,
    pub input: Option<String>,
    pub output: Option<String>,
}

pub async fn run_call(request: CallRequest) {
    let CallRequest {
        target,
        user_id,
        channel_id,
        mut commands,
        status,
        server,
        input,
        output,
    } = request;
    let _ = status.send(format!("Connecting to {server}"));
    let dave = match LiveDave::new(user_id, channel_id) {
        Ok(engine) => DaveEngine::Live(Box::new(engine)),
        Err(err) => {
            let _ = status.send(format!("Voice error ({server}): {err}"));
            return;
        }
    };
    let mut call = match LiveCall::connect_with(target, dave).await {
        Ok(call) => call,
        Err(err) => {
            let _ = status.send(format!("Voice error ({server}): {err}"));
            return;
        }
    };
    let _ = status.send(format!("RTC connected to {server}"));
    let mut dave_announced = false;
    let (pcm_tx, pcm_rx) = std::sync::mpsc::channel();
    let speaker = Arc::new(Mutex::new(VecDeque::new()));
    // cpal streams are not Send, so they stay on their own thread.
    let mut audio_stop = spawn_devices(
        input.as_deref(),
        output.as_deref(),
        pcm_tx.clone(),
        speaker.clone(),
        &status,
        &server,
    );
    let mut last_pcm = Instant::now();
    let mut heard_pcm = false;
    loop {
        while let Ok(pcm) = pcm_rx.try_recv() {
            last_pcm = Instant::now();
            heard_pcm = true;
            if let Err(err) = call.send_microphone(&pcm).await {
                let _ = status.send(format!("Voice error ({server}): {err}"));
                return;
            }
        }
        if heard_pcm && last_pcm.elapsed() > Duration::from_millis(120) {
            if let Err(err) = call.stop_speaking().await {
                let _ = status.send(format!("Voice error ({server}): {err}"));
                return;
            }
            heard_pcm = false;
        }
        tokio::select! {
            command = commands.recv() => {
                match command {
                    None | Some(VoiceCmd::Leave) => {
                        let _ = call.stop_speaking().await;
                        break;
                    }
                    Some(VoiceCmd::Mute(muted)) => call.set_muted(muted),
                    Some(VoiceCmd::Deaf(deafened)) => call.set_deafened(deafened),
                    Some(VoiceCmd::Devices { input, output }) => {
                        drop(audio_stop);
                        audio_stop = spawn_devices(
                            input.as_deref(),
                            output.as_deref(),
                            pcm_tx.clone(),
                            speaker.clone(),
                            &status,
                            &server,
                        );
                    }
                }
            }
            step = call.drive_once() => match step {
                Ok(Some(pcm)) => {
                    if let Ok(mut queue) = speaker.lock() {
                        queue.extend(pcm);
                    }
                }
                Ok(None) => {
                    if !dave_announced && call.dave_ready() {
                        dave_announced = true;
                        let _ = status.send(format!("DAVE ready ({server})"));
                    }
                }
                Err(err) => {
                    let _ = status.send(voice_error(&server, err));
                    break;
                }
            }
        }
    }
    drop(audio_stop);
}

fn spawn_devices(
    input: Option<&str>,
    output: Option<&str>,
    pcm_tx: std::sync::mpsc::Sender<Vec<i16>>,
    speaker: Arc<Mutex<VecDeque<i16>>>,
    status: &std::sync::mpsc::Sender<String>,
    server: &str,
) -> Option<std::sync::mpsc::Sender<()>> {
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    let status_note = status.clone();
    let server = server.to_owned();
    let input = input.map(str::to_owned);
    let output = output.map(str::to_owned);
    std::thread::spawn(move || {
        match rencord_voice::open_pcm_devices(input.as_deref(), output.as_deref(), pcm_tx, speaker)
        {
            Ok(session) => {
                if session.input_fallback || session.output_fallback {
                    let _ = status_note.send(format!(
                        "A chosen device was missing. Using {} and {}.",
                        session.input_name, session.output_name
                    ));
                }
                let _ = stop_rx.recv();
                drop(session);
            }
            Err(err) => {
                let _ = status_note.send(format!(
                    "Voice error ({server}): audio devices were not opened ({err})"
                ));
            }
        }
    });
    Some(stop_tx)
}

fn voice_error(server: &str, err: VoiceLinkError) -> String {
    format!("Voice error ({server}): {err}")
}
