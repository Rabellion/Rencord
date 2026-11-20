//! Voice websocket plus UDP.
//!
//! `LiveCall::connect_with` speaks the voice gateway (identify or resume,
//! select protocol, speaking, DAVE opcodes) and discovers the public UDP
//! address. Tests point it at a local stub. A real Discord voice server was
//! not used.

use crate::dave::{DaveEngine, DaveOps, ScriptedDave};
use crate::discovery::{
    bind_for, discover, server_addr, DiscoveryError, DiscoverySchedule, MediaRole, PathWatch,
};
use crate::mixer::GainMixer;
use crate::opus::FRAME_SAMPLES;
use crate::pipeline::{PipelineError, VoicePipeline};
use crate::session::{VoicePhase, VoiceSession};
use futures_util::{SinkExt, StreamExt};
use rencord_protocol::{
    voice_binary, voice_heartbeat, voice_resume, FrameError, VoiceFrame, VoiceHello, VoiceOpcode,
    VoiceReady, TRANSPORT_MODE,
};
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::time::Instant;
use tokio_tungstenite::{
    connect_async, tungstenite::Message as WsMessage, MaybeTlsStream, WebSocketStream,
};

type VoiceSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct FailedOpen {
    error: VoiceLinkError,
    dave: DaveEngine,
}

enum Wake {
    Tick,
    Socket(Option<Result<WsMessage, tokio_tungstenite::tungstenite::Error>>),
    Udp(Result<Vec<u8>, std::io::Error>),
}

#[derive(Debug, thiserror::Error)]
pub enum VoiceLinkError {
    #[error("websocket: {0}")]
    Socket(String),
    #[error("voice timed out waiting for {0}")]
    Timeout(&'static str),
    #[error("udp: {0}")]
    Udp(#[from] std::io::Error),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("pipeline: {0}")]
    Pipeline(#[from] PipelineError),
    #[error("{0}")]
    DiscoveryFailed(DiscoveryError),
}

impl From<tokio_tungstenite::tungstenite::Error> for VoiceLinkError {
    fn from(value: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Socket(value.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct VoiceTarget {
    pub ws_url: String,
    pub server_id: String,
    pub user_id: String,
    pub session_id: String,
    pub token: String,
}

/// Microphone and speaker path around the adaptive jitter buffer.
pub struct CallAudio {
    pub pipeline: VoicePipeline,
    pub mixer: GainMixer,
    pub muted: bool,
}

impl CallAudio {
    pub fn new(ssrc: u32, key: &[u8]) -> Result<Self, PipelineError> {
        Self::from_pipeline(VoicePipeline::new(ssrc)?, key)
    }

    pub fn new_stereo(ssrc: u32, key: &[u8]) -> Result<Self, PipelineError> {
        Self::from_pipeline(VoicePipeline::new_stereo(ssrc)?, key)
    }

    fn from_pipeline(mut pipeline: VoicePipeline, key: &[u8]) -> Result<Self, PipelineError> {
        pipeline.install_key(key)?;
        Ok(Self {
            pipeline,
            mixer: GainMixer::default(),
            muted: false,
        })
    }

    /// Encode a microphone frame. `None` means mute suppressed the packet.
    pub fn on_microphone(&mut self, pcm: &[i16]) -> Result<Option<Vec<u8>>, PipelineError> {
        if self.muted || pcm.len() != FRAME_SAMPLES {
            return Ok(None);
        }
        Ok(Some(self.pipeline.send_frame(pcm, None)?))
    }

    /// Decrypt, pass through the jitter buffer, then apply per-SSRC gain.
    pub fn on_rtp(
        &mut self,
        packet: &[u8],
        arrival_ms: u64,
        ssrc: u32,
    ) -> Result<Vec<i16>, PipelineError> {
        self.pipeline
            .recv_packet(packet, arrival_ms, u64::from(ssrc), None, false)?;
        let mut pcm = self.pipeline.pull_pcm()?;
        self.mixer.apply(ssrc, &mut pcm);
        Ok(pcm)
    }
}

pub struct LiveCall {
    pub phase: VoicePhase,
    pub ssrc: u32,
    pub external_address: String,
    pub external_port: u16,
    pub muted: bool,
    pub deafened: bool,
    /// UDP audio packets heard; stamps jitter-buffer arrival times.
    pub rx_packets: u64,
    pub role: MediaRole,
    pub probes: u32,
    speaking: bool,
    target: VoiceTarget,
    write: futures_util::stream::SplitSink<VoiceSocket, WsMessage>,
    read: futures_util::stream::SplitStream<VoiceSocket>,
    udp: UdpSocket,
    server_udp: SocketAddr,
    session: VoiceSession,
    audio: CallAudio,
    path: PathWatch,
    schedule: DiscoverySchedule,
    /// Last sequence number the server stamped on a frame (voice v8).
    last_seq: Option<u64>,
    beat_every: Duration,
    next_beat: Instant,
    awaiting_ack: bool,
}

impl LiveCall {
    pub async fn connect_with(
        target: VoiceTarget,
        dave: DaveEngine,
    ) -> Result<Self, VoiceLinkError> {
        Self::connect_role(
            MediaRole::Voice,
            target,
            dave,
            DiscoverySchedule::production(),
        )
        .await
    }

    /// Go Live uses the same handshake, discovery, and keepalive as voice.
    pub async fn connect_stream(
        target: VoiceTarget,
        dave: DaveEngine,
    ) -> Result<Self, VoiceLinkError> {
        Self::connect_role(
            MediaRole::Stream,
            target,
            dave,
            DiscoverySchedule::production(),
        )
        .await
    }

    async fn connect_role(
        role: MediaRole,
        target: VoiceTarget,
        dave: DaveEngine,
        schedule: DiscoverySchedule,
    ) -> Result<Self, VoiceLinkError> {
        let mut dave = dave;
        let mut probes = 0u32;
        let mut server = SocketAddr::from(([0, 0, 0, 0], 0));
        for attempt in 1..=2 {
            match Self::open_once(role, &target, dave, schedule.clone()).await {
                Ok(call) => return Ok(call),
                Err(failed) => {
                    dave = failed.dave;
                    if let VoiceLinkError::DiscoveryFailed(DiscoveryError::Failed {
                        probes: sent,
                        server: addr,
                        ..
                    }) = failed.error
                    {
                        probes += sent;
                        server = addr;
                        if attempt == 2 {
                            return Err(VoiceLinkError::DiscoveryFailed(DiscoveryError::failed(
                                role, server, probes, 2,
                            )));
                        }
                    } else {
                        return Err(failed.error);
                    }
                }
            }
        }
        Err(VoiceLinkError::DiscoveryFailed(DiscoveryError::failed(
            role, server, probes, 2,
        )))
    }

    async fn open_once(
        role: MediaRole,
        target: &VoiceTarget,
        dave: DaveEngine,
        schedule: DiscoverySchedule,
    ) -> Result<Self, Box<FailedOpen>> {
        let opened = match connect_async(&target.ws_url).await {
            Ok(opened) => opened,
            Err(err) => {
                return Err(Box::new(FailedOpen {
                    error: err.into(),
                    dave,
                }))
            }
        };
        let (socket, _) = opened;
        let (write, read) = socket.split();
        let udp = match UdpSocket::bind("127.0.0.1:0").await {
            Ok(udp) => udp,
            Err(err) => {
                return Err(Box::new(FailedOpen {
                    error: err.into(),
                    dave,
                }))
            }
        };
        let audio = match CallAudio::new(1, &[7; 32]) {
            Ok(audio) => audio,
            Err(err) => {
                return Err(Box::new(FailedOpen {
                    error: err.into(),
                    dave,
                }))
            }
        };
        let mut call = Self {
            phase: VoicePhase::Idle,
            ssrc: 0,
            external_address: String::new(),
            external_port: 0,
            muted: false,
            deafened: false,
            rx_packets: 0,
            role,
            probes: 0,
            speaking: false,
            session: VoiceSession::new(
                target.server_id.clone(),
                target.user_id.clone(),
                target.session_id.clone(),
                target.token.clone(),
                dave,
            ),
            target: target.clone(),
            write,
            read,
            udp,
            server_udp: SocketAddr::from(([127, 0, 0, 1], 1)),
            audio,
            path: PathWatch::new(),
            schedule,
            last_seq: None,
            beat_every: Duration::from_secs(41),
            next_beat: Instant::now() + Duration::from_secs(41),
            awaiting_ack: false,
        };
        if let Err(error) = call.finish_handshake().await {
            return Err(Box::new(FailedOpen {
                error,
                dave: call.take_dave(),
            }));
        }
        Ok(call)
    }

    fn take_dave(&mut self) -> DaveEngine {
        std::mem::replace(
            &mut self.session.dave,
            DaveEngine::Scripted(ScriptedDave::default()),
        )
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.audio.muted = muted;
    }

    pub fn set_deafened(&mut self, deafened: bool) {
        self.deafened = deafened;
        self.audio.mixer.set_deaf(deafened);
    }

    pub fn set_ssrc_gain(&mut self, ssrc: u32, gain: f32) {
        self.audio.mixer.set_gain(ssrc, gain);
    }

    pub fn audio_mut(&mut self) -> &mut CallAudio {
        &mut self.audio
    }

    pub fn dave_ready(&self) -> bool {
        self.session.dave.can_encrypt()
    }

    /// Opus-encode, DAVE-encrypt when the epoch is ready, seal, and send.
    /// Returns whether a packet went out. Five silence frames follow a stop.
    pub async fn send_microphone(&mut self, pcm: &[i16]) -> Result<bool, VoiceLinkError> {
        if self.muted || pcm.is_empty() {
            return Ok(false);
        }
        if !self.speaking {
            if let Some(message) = self.session.speaking_message(true) {
                self.send_json(message).await?;
            }
            self.speaking = true;
        }
        let packet = self
            .audio
            .pipeline
            .send_frame(pcm, Some(&mut self.session.dave))?;
        self.udp.send(&packet).await?;
        Ok(true)
    }

    /// Discord asks for five silence frames before speaking stops.
    pub async fn stop_speaking(&mut self) -> Result<(), VoiceLinkError> {
        if !self.speaking {
            return Ok(());
        }
        for _ in 0..5 {
            let packet = self
                .audio
                .pipeline
                .send_opus(crate::opus::SILENCE_OPUS, Some(&mut self.session.dave))?;
            self.udp.send(&packet).await?;
        }
        if let Some(message) = self.session.speaking_message(false) {
            self.send_json(message).await?;
        }
        self.speaking = false;
        Ok(())
    }

    pub async fn recv_speaker(&mut self, wait: Duration) -> Result<Vec<i16>, VoiceLinkError> {
        let deadline = tokio::time::Instant::now() + wait;
        let mut buf = vec![0u8; 2048];
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(VoiceLinkError::Timeout("udp audio"));
            }
            let read = tokio::time::timeout(remaining, self.udp.recv(&mut buf))
                .await
                .map_err(|_| VoiceLinkError::Timeout("udp audio"))??;
            self.path.inbound();
            if read < 12 {
                continue;
            }
            if self.deafened {
                return Ok(vec![0; FRAME_SAMPLES]);
            }
            self.rx_packets = self.rx_packets.wrapping_add(1);
            let arrival = self.rx_packets.saturating_mul(20);
            let ssrc = self.ssrc;
            self.audio.pipeline.recv_packet(
                &buf[..read],
                arrival,
                u64::from(ssrc),
                Some(&mut self.session.dave),
                false,
            )?;
            let mut pcm = self.audio.pipeline.pull_pcm()?;
            self.audio.mixer.apply(ssrc, &mut pcm);
            return Ok(pcm);
        }
    }

    /// Send Discord's UDP ping when it is due, and reconnect when the path is quiet.
    pub async fn maintain(&mut self) -> Result<(), VoiceLinkError> {
        if self.path.dead() {
            // A dead UDP path is not fixed by resuming the websocket, so start over.
            self.reconnect().await?;
            self.path.inbound();
            return Ok(());
        }
        if self.path.keepalive_due() {
            let packet = self.path.next_keepalive();
            self.udp.send(&packet).await?;
        }
        Ok(())
    }

    /// Reopen the websocket and resume the session: Resume (op 7), answered by
    /// Resumed (op 9). The UDP path and secret key are unchanged.
    pub async fn resume_gateway(&mut self) -> Result<(), VoiceLinkError> {
        let (socket, _) = connect_async(&self.target.ws_url).await?;
        let (write, read) = socket.split();
        self.write = write;
        self.read = read;
        let hello = self.read_frame("hello").await?;
        let hello: VoiceHello = serde_json::from_value(hello.data)
            .map_err(|err| VoiceLinkError::Protocol(err.to_string()))?;
        self.send_json(voice_resume(
            &self.target.server_id,
            &self.target.session_id,
            &self.target.token,
            self.last_seq.unwrap_or(0),
        ))
        .await?;
        self.read_op(VoiceOpcode::Resumed, "resumed").await?;
        self.start_heartbeat(hello.heartbeat_interval);
        Ok(())
    }

    /// Open a fresh session on a new websocket, with UDP discovery again.
    pub async fn reconnect(&mut self) -> Result<(), VoiceLinkError> {
        let mut probes = 0u32;
        let mut server = self.server_udp;
        for attempt in 1..=2 {
            let (socket, _) = connect_async(&self.target.ws_url).await?;
            let (write, read) = socket.split();
            self.write = write;
            self.read = read;
            match self.finish_handshake().await {
                Ok(()) => return Ok(()),
                Err(VoiceLinkError::DiscoveryFailed(DiscoveryError::Failed {
                    probes: sent,
                    server: addr,
                    ..
                })) => {
                    probes += sent;
                    server = addr;
                    if attempt == 2 {
                        return Err(VoiceLinkError::DiscoveryFailed(DiscoveryError::failed(
                            self.role, server, probes, 2,
                        )));
                    }
                }
                Err(err) => return Err(err),
            }
        }
        Err(VoiceLinkError::DiscoveryFailed(DiscoveryError::failed(
            self.role, server, probes, 2,
        )))
    }

    async fn finish_handshake(&mut self) -> Result<(), VoiceLinkError> {
        let hello = self.read_frame("hello").await?;
        for message in self.session.handle(hello) {
            self.send_json(message).await?;
        }
        self.start_heartbeat(self.session.heartbeat_interval);
        let ready_frame = self.read_op(VoiceOpcode::Ready, "ready").await?;
        let ready: VoiceReady = serde_json::from_value(ready_frame.data.clone())
            .map_err(|err| VoiceLinkError::Protocol(err.to_string()))?;
        for message in self.session.handle(ready_frame) {
            self.send_json(message).await?;
        }
        self.ssrc = ready.ssrc;
        self.server_udp = server_addr(&ready.ip, ready.port).map_err(VoiceLinkError::Protocol)?;
        self.udp = bind_for(self.server_udp).await?;
        let discovered = discover(
            &self.udp,
            self.server_udp,
            self.ssrc,
            self.role,
            &self.schedule,
        )
        .await
        .map_err(VoiceLinkError::DiscoveryFailed)?;
        self.probes = discovered.probes;
        self.external_address = discovered.address.clone();
        self.external_port = discovered.port;
        self.path.inbound();
        self.udp
            .connect(self.server_udp)
            .await
            .map_err(VoiceLinkError::Udp)?;
        let select = self
            .session
            .select_protocol_message(&self.external_address, self.external_port);
        self.send_json(select).await?;
        let description = self
            .read_op(VoiceOpcode::SessionDescription, "session description")
            .await?;
        for message in self.session.handle(description) {
            self.send_json(message).await?;
        }
        if let Some(package) = self.session.take_key_package() {
            self.send_binary(VoiceOpcode::MlsKeyPackage, &package)
                .await?;
        }
        let key = self
            .session
            .secret_key
            .clone()
            .ok_or_else(|| VoiceLinkError::Protocol("missing secret key".into()))?;
        self.audio = CallAudio::new_stereo(self.ssrc, &key)?;
        self.audio.muted = self.muted;
        self.audio.mixer.set_deaf(self.deafened);
        if let Some(speaking) = self.session.speaking_message(!self.muted) {
            self.send_json(speaking).await?;
        }
        self.phase = self.session.phase;
        // The interval is measured from hello, which is already behind us.
        // The first beat the caller sees should still be a full interval away.
        self.next_beat = Instant::now() + self.beat_every;
        self.awaiting_ack = false;
        let _ = TRANSPORT_MODE;
        Ok(())
    }

    /// Wait for one control frame and apply DAVE / speaking updates. Sends a
    /// heartbeat whenever one is due while waiting, and fails if the previous
    /// one was never acknowledged. Callers run this in a loop; it has no
    /// read timeout of its own because a quiet control channel is normal.
    pub async fn poll_control(&mut self) -> Result<(), VoiceLinkError> {
        let frame = loop {
            let beat_at = self.next_beat;
            tokio::select! {
                biased;
                frame = self.next_frame() => break frame?,
                _ = tokio::time::sleep_until(beat_at) => self.beat().await?,
            }
        };
        self.dispatch_frame(frame).await?;
        Ok(())
    }

    /// One step of the call: a control frame, a heartbeat, a UDP packet, or a
    /// 20 ms tick so microphone packets are not stuck behind a quiet socket.
    pub async fn drive_once(&mut self) -> Result<Option<Vec<i16>>, VoiceLinkError> {
        let wake = {
            let beat_at = self.next_beat;
            let tick = Instant::now() + Duration::from_millis(20);
            let read = &mut self.read;
            let udp = &self.udp;
            let mut buf = vec![0u8; 2048];
            tokio::select! {
                biased;
                message = read.next() => Wake::Socket(message),
                read = udp.recv(&mut buf) => Wake::Udp(read.map(|n| buf[..n].to_vec())),
                _ = tokio::time::sleep_until(beat_at.min(tick)) => Wake::Tick,
            }
        };
        match wake {
            Wake::Tick => {
                if Instant::now() >= self.next_beat {
                    self.beat().await?;
                }
                self.maintain().await?;
                Ok(None)
            }
            Wake::Socket(message) => {
                if let Some(frame) = self.frame_from(message)? {
                    self.dispatch_frame(frame).await?;
                }
                Ok(None)
            }
            Wake::Udp(Ok(buf)) => {
                self.path.inbound();
                if buf.len() < 12 || self.deafened {
                    return Ok(None);
                }
                self.rx_packets = self.rx_packets.wrapping_add(1);
                let arrival = self.rx_packets.saturating_mul(20);
                let ssrc = self.ssrc;
                self.audio.pipeline.recv_packet(
                    &buf,
                    arrival,
                    u64::from(ssrc),
                    Some(&mut self.session.dave),
                    false,
                )?;
                let mut pcm = self.audio.pipeline.pull_pcm()?;
                self.audio.mixer.apply(ssrc, &mut pcm);
                Ok(Some(pcm))
            }
            Wake::Udp(Err(err)) => Err(err.into()),
        }
    }

    fn frame_from(
        &mut self,
        message: Option<Result<WsMessage, tokio_tungstenite::tungstenite::Error>>,
    ) -> Result<Option<VoiceFrame>, VoiceLinkError> {
        let message =
            message.ok_or_else(|| VoiceLinkError::Socket("voice websocket closed".into()))??;
        let parsed = match message {
            WsMessage::Text(text) => VoiceFrame::parse(text.as_bytes()),
            WsMessage::Binary(bytes) => VoiceFrame::parse_binary(&bytes),
            WsMessage::Close(_) => {
                return Err(VoiceLinkError::Socket("voice websocket closed".into()))
            }
            _ => return Ok(None),
        };
        match parsed {
            Ok(frame) => {
                if frame.seq.is_some() {
                    self.last_seq = frame.seq;
                }
                if frame.op == VoiceOpcode::HeartbeatAck {
                    self.awaiting_ack = false;
                }
                Ok(Some(frame))
            }
            Err(FrameError::Opcode) => Ok(None),
            Err(err) => Err(VoiceLinkError::Protocol(err.to_string())),
        }
    }

    async fn dispatch_frame(&mut self, frame: VoiceFrame) -> Result<(), VoiceLinkError> {
        let outgoing = self.session.handle(frame);
        for message in outgoing {
            self.send_json(message).await?;
        }
        for commit in std::mem::take(&mut self.session.commits_to_send) {
            let payload =
                crate::session::commit_welcome_payload(&commit.commit, commit.welcome.as_deref());
            self.send_binary(VoiceOpcode::MlsCommitWelcome, &payload)
                .await?;
        }
        if let Some(package) = self.session.take_key_package() {
            self.send_binary(VoiceOpcode::MlsKeyPackage, &package)
                .await?;
        }
        if let Some(speaking) = self.session.speaking_message(!self.muted) {
            let _ = speaking;
        }
        self.phase = self.session.phase;
        Ok(())
    }

    fn start_heartbeat(&mut self, interval_ms: u64) {
        self.beat_every = Duration::from_millis(interval_ms.max(1));
        self.next_beat = Instant::now() + self.beat_every;
        self.awaiting_ack = false;
    }

    async fn beat(&mut self) -> Result<(), VoiceLinkError> {
        if self.awaiting_ack {
            return Err(VoiceLinkError::Timeout("heartbeat ack"));
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        self.send_json(voice_heartbeat(nonce, self.last_seq.unwrap_or(0)))
            .await?;
        self.awaiting_ack = true;
        self.next_beat += self.beat_every;
        Ok(())
    }

    /// Next frame the session understands, with no timeout. Text and binary
    /// frames both count; one with an opcode we do not know is skipped, since
    /// Discord adds opcodes over time.
    async fn next_frame(&mut self) -> Result<VoiceFrame, VoiceLinkError> {
        loop {
            let message = self
                .read
                .next()
                .await
                .ok_or_else(|| VoiceLinkError::Socket("voice websocket closed".into()))??;
            let parsed = match message {
                WsMessage::Text(text) => VoiceFrame::parse(text.as_bytes()),
                WsMessage::Binary(bytes) => VoiceFrame::parse_binary(&bytes),
                WsMessage::Close(_) => {
                    return Err(VoiceLinkError::Socket("voice websocket closed".into()))
                }
                _ => continue,
            };
            match parsed {
                Ok(frame) => {
                    if frame.seq.is_some() {
                        self.last_seq = frame.seq;
                    }
                    if frame.op == VoiceOpcode::HeartbeatAck {
                        self.awaiting_ack = false;
                    }
                    return Ok(frame);
                }
                Err(FrameError::Opcode) => continue,
                Err(err) => return Err(VoiceLinkError::Protocol(err.to_string())),
            }
        }
    }

    /// Handshake steps only: a silent server there means a failed connect.
    async fn read_frame(&mut self, what: &'static str) -> Result<VoiceFrame, VoiceLinkError> {
        tokio::time::timeout(Duration::from_secs(2), self.next_frame())
            .await
            .map_err(|_| VoiceLinkError::Timeout(what))?
    }

    /// Handshake read that waits for one opcode. Anything else that arrives
    /// first (clients connect, a DAVE message) still goes to the session.
    async fn read_op(
        &mut self,
        op: VoiceOpcode,
        what: &'static str,
    ) -> Result<VoiceFrame, VoiceLinkError> {
        loop {
            let frame = self.read_frame(what).await?;
            if frame.op == op {
                return Ok(frame);
            }
            for message in self.session.handle(frame) {
                self.send_json(message).await?;
            }
        }
    }

    async fn send_binary(&mut self, op: VoiceOpcode, payload: &[u8]) -> Result<(), VoiceLinkError> {
        self.write
            .send(WsMessage::Binary(voice_binary(op, payload).into()))
            .await?;
        Ok(())
    }
    async fn send_json(&mut self, value: serde_json::Value) -> Result<(), VoiceLinkError> {
        self.write
            .send(WsMessage::Text(value.to_string().into()))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dave::{DaveEngine, ScriptedDave};
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    async fn stub() -> (String, std::net::SocketAddr) {
        let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_addr = ws_listener.local_addr().unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_addr = udp.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                let Ok((n, peer)) = udp.recv_from(&mut buf).await else {
                    break;
                };
                if n >= 74 && buf[1] == 1 && buf[3] == 70 {
                    let mut response = [0u8; 74];
                    response[1] = 2;
                    response[3] = 70;
                    response[4..8].copy_from_slice(&buf[4..8]);
                    let ip = b"127.0.0.1";
                    response[8..8 + ip.len()].copy_from_slice(ip);
                    response[72..74].copy_from_slice(&peer.port().to_be_bytes());
                    let _ = udp.send_to(&response, peer).await;
                } else {
                    let _ = udp.send_to(&buf[..n], peer).await;
                }
            }
        });
        let udp_ip = udp_addr.ip().to_string();
        let udp_port = udp_addr.port();
        tokio::spawn(async move {
            async fn serve(
                listener: &TcpListener,
                udp_ip: &str,
                udp_port: u16,
                expect_resume: bool,
            ) {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = accept_async(stream).await.unwrap();
                socket
                    .send(WsMessage::Text(
                        r#"{"op":8,"d":{"heartbeat_interval":45000}}"#.into(),
                    ))
                    .await
                    .unwrap();
                let first = socket.next().await.unwrap().unwrap().into_text().unwrap();
                if expect_resume {
                    // Resume carries the last server seq (5, stamped on op 21 below),
                    // and Discord answers Resumed, with no new Ready.
                    assert!(first.contains("\"op\":7"), "{first}");
                    assert!(first.contains("\"seq_ack\":5"), "{first}");
                    socket
                        .send(WsMessage::Text(r#"{"op":9,"d":null}"#.into()))
                        .await
                        .unwrap();
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    return;
                }
                assert!(first.contains("\"op\":0"), "{first}");
                socket
                    .send(WsMessage::Text(
                        format!(
                            r#"{{"op":2,"d":{{"ssrc":77,"ip":"{udp_ip}","port":{udp_port},"modes":["aead_xchacha20_poly1305_rtpsize"]}}}}"#
                        )
                        .into(),
                    ))
                    .await
                    .unwrap();
                let select = socket.next().await.unwrap().unwrap().into_text().unwrap();
                assert!(
                    select.contains("aead_xchacha20_poly1305_rtpsize"),
                    "{select}"
                );
                let mut key = vec![0u8; 32];
                key[0] = 9;
                socket
                    .send(WsMessage::Text(
                        serde_json::json!({
                            "op": 4,
                            "d": {
                                "mode": "aead_xchacha20_poly1305_rtpsize",
                                "secret_key": key,
                                "dave_protocol_version": 1
                            }
                        })
                        .to_string()
                        .into(),
                    ))
                    .await
                    .unwrap();
                let next = socket.next().await.unwrap().unwrap();
                let speaking = match next {
                    WsMessage::Binary(bytes) => {
                        assert_eq!(bytes[0], VoiceOpcode::MlsKeyPackage as u8, "{bytes:?}");
                        socket
                            .next()
                            .await
                            .unwrap()
                            .unwrap()
                            .into_text()
                            .unwrap()
                            .to_string()
                    }
                    WsMessage::Text(text) => text.to_string(),
                    other => panic!("expected key package or speaking, got {other:?}"),
                };
                assert!(speaking.contains("\"op\":5"), "{speaking}");
                socket
                    .send(WsMessage::Text(
                        r#"{"op":21,"d":{"transition_id":3,"protocol_version":1},"seq":5}"#.into(),
                    ))
                    .await
                    .unwrap();
                let ready = socket.next().await.unwrap().unwrap().into_text().unwrap();
                assert!(ready.contains("\"op\":23"), "{ready}");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            serve(&ws_listener, &udp_ip, udp_port, false).await;
            serve(&ws_listener, &udp_ip, udp_port, true).await;
        });
        (format!("ws://{ws_addr}"), udp_addr)
    }

    #[tokio::test]
    async fn local_voice_server_handshake_audio_and_resume() {
        let (ws_url, _) = stub().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let mut call = LiveCall::connect_with(
            VoiceTarget {
                ws_url,
                server_id: "10".into(),
                user_id: "1".into(),
                session_id: "voice-sess".into(),
                token: "voice-token".into(),
            },
            DaveEngine::Scripted(ScriptedDave::default()),
        )
        .await
        .unwrap();
        assert_eq!(call.ssrc, 77);
        assert_eq!(call.external_address, "127.0.0.1");
        assert!(call.external_port > 0);
        assert_eq!(call.phase, VoicePhase::Connected);
        call.poll_control().await.unwrap();
        let DaveEngine::Scripted(scripted) = &call.session.dave else {
            panic!("scripted dave");
        };
        assert_eq!(scripted.prepares, vec![(3, 1)]);

        let pcm = crate::opus::sine_frame(0);
        for _ in 0..8 {
            call.send_microphone(&pcm).await.unwrap();
        }
        let mut energy = 0i64;
        let mut got = 0u32;
        for _ in 0..8 {
            match call.recv_speaker(Duration::from_millis(300)).await {
                Ok(heard) => {
                    got += 1;
                    energy += heard
                        .iter()
                        .map(|sample| i64::from(sample.abs()))
                        .sum::<i64>();
                }
                Err(VoiceLinkError::Timeout(_)) => break,
                Err(err) => panic!("{err}"),
            }
        }
        assert!(got >= 4, "frames {got} energy {energy}");
        assert!(energy > 1000, "energy {energy}");

        call.set_muted(true);
        call.send_microphone(&pcm).await.unwrap();
        call.set_ssrc_gain(77, 0.0);
        call.set_deafened(true);
        assert!(call.deafened);

        call.resume_gateway().await.unwrap();
        assert_eq!(call.phase, VoicePhase::Connected);
    }

    fn quick_schedule() -> DiscoverySchedule {
        DiscoverySchedule {
            gaps: vec![Duration::from_millis(10), Duration::from_millis(10)],
            deadline: Duration::from_millis(40),
        }
    }

    async fn answering_udp() -> std::net::SocketAddr {
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = udp.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 256];
            loop {
                let Ok((n, peer)) = udp.recv_from(&mut buf).await else {
                    break;
                };
                if n >= 8 && buf[1] == 1 {
                    let mut response = [0u8; 74];
                    response[1] = 2;
                    response[3] = 70;
                    response[4..8].copy_from_slice(&buf[4..8]);
                    let ip = b"127.0.0.1";
                    response[8..8 + ip.len()].copy_from_slice(ip);
                    response[72..74].copy_from_slice(&peer.port().to_be_bytes());
                    let _ = udp.send_to(&response, peer).await;
                }
            }
        });
        addr
    }

    async fn silent_udp() -> std::net::SocketAddr {
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = udp.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 64];
            while udp.recv_from(&mut buf).await.is_ok() {}
        });
        addr
    }

    async fn ready_server(ports: Vec<std::net::SocketAddr>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for udp in ports {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = accept_async(stream).await.unwrap();
                socket
                    .send(WsMessage::Text(
                        r#"{"op":8,"d":{"heartbeat_interval":45000}}"#.into(),
                    ))
                    .await
                    .unwrap();
                let first = socket.next().await.unwrap().unwrap().into_text().unwrap();
                let resumed = first.contains("\"op\":7");
                socket
                    .send(WsMessage::Text(
                        format!(
                            r#"{{"op":2,"d":{{"ssrc":77,"ip":"{}","port":{},"modes":["aead_xchacha20_poly1305_rtpsize"]}}}}"#,
                            udp.ip(),
                            udp.port()
                        )
                        .into(),
                    ))
                    .await
                    .unwrap();
                if resumed {
                    continue;
                }
                let next = socket.next().await;
                let Some(Ok(message)) = next else {
                    continue;
                };
                let select = message.into_text().unwrap();
                if !select.contains("aead_xchacha20_poly1305_rtpsize") {
                    continue;
                }
                let mut key = vec![0u8; 32];
                key[0] = 9;
                socket
                    .send(WsMessage::Text(
                        serde_json::json!({
                            "op": 4,
                            "d": {
                                "mode": "aead_xchacha20_poly1305_rtpsize",
                                "secret_key": key
                            }
                        })
                        .to_string()
                        .into(),
                    ))
                    .await
                    .unwrap();
                let _ = socket.next().await;
            }
        });
        format!("ws://{addr}")
    }

    #[tokio::test]
    async fn stream_retries_discovery_once_then_connects() {
        let dead = silent_udp().await;
        let live = answering_udp().await;
        let ws_url = ready_server(vec![dead, live]).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let call = LiveCall::connect_role(
            MediaRole::Stream,
            VoiceTarget {
                ws_url,
                server_id: "10".into(),
                user_id: "1".into(),
                session_id: "stream-sess".into(),
                token: "stream-token".into(),
            },
            DaveEngine::Scripted(ScriptedDave::default()),
            quick_schedule(),
        )
        .await
        .unwrap();
        assert_eq!(call.role, MediaRole::Stream);
        assert_eq!(call.external_address, "127.0.0.1");
        assert!(call.external_port > 0);
        assert_eq!(call.phase, VoicePhase::TransportOnly);
    }

    #[tokio::test]
    async fn voice_discovery_reports_probe_count_when_both_attempts_are_silent() {
        let first = silent_udp().await;
        let second = silent_udp().await;
        let ws_url = ready_server(vec![first, second]).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let err = match LiveCall::connect_role(
            MediaRole::Voice,
            VoiceTarget {
                ws_url,
                server_id: "10".into(),
                user_id: "1".into(),
                session_id: "voice-sess".into(),
                token: "voice-token".into(),
            },
            DaveEngine::Scripted(ScriptedDave::default()),
            quick_schedule(),
        )
        .await
        {
            Ok(_) => panic!("silent servers should fail discovery"),
            Err(err) => err,
        };
        let text = err.to_string();
        assert!(text.contains("voice UDP discovery failed"), "{text}");
        assert!(text.contains("2 connection attempts"), "{text}");
        assert!(text.contains("probes"), "{text}");
        assert!(text.contains("mobile hotspot is a good test"), "{text}");
        let VoiceLinkError::DiscoveryFailed(DiscoveryError::Failed {
            probes, attempts, ..
        }) = err
        else {
            panic!("{text}");
        };
        assert_eq!(attempts, 2);
        assert!(probes >= 4, "probes {probes}");
    }

    type TestSocket = WebSocketStream<tokio::net::TcpStream>;

    fn test_target(ws_url: String) -> VoiceTarget {
        VoiceTarget {
            ws_url,
            server_id: "10".into(),
            user_id: "1".into(),
            session_id: "voice-sess".into(),
            token: "voice-token".into(),
        }
    }

    fn text(value: &str) -> WsMessage {
        WsMessage::Text(value.to_string().into())
    }

    /// Plays Discord's side of the handshake up to Speaking, then hands the
    /// socket to `script`. `noisy` sprinkles opcodes Rencord does not know, and
    /// Clients Connect, into the handshake.
    async fn control_server<F, Fut, T>(
        beat_ms: u64,
        noisy: bool,
        script: F,
    ) -> (String, tokio::task::JoinHandle<T>)
    where
        F: FnOnce(TestSocket) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let udp = answering_udp().await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(text(&format!(
                r#"{{"op":8,"d":{{"heartbeat_interval":{beat_ms}}}}}"#
            )))
            .await
            .unwrap();
            ws.next().await.unwrap().unwrap(); // identify
            if noisy {
                ws.send(text(r#"{"op":99,"d":{"future":true}}"#))
                    .await
                    .unwrap();
                ws.send(text(r#"{"op":11,"d":{"user_ids":["2"]},"seq":1}"#))
                    .await
                    .unwrap();
            }
            ws.send(text(&format!(
                r#"{{"op":2,"d":{{"ssrc":77,"ip":"{}","port":{},"modes":["aead_xchacha20_poly1305_rtpsize"]}}}}"#,
                udp.ip(),
                udp.port()
            )))
            .await
            .unwrap();
            ws.next().await.unwrap().unwrap(); // select protocol
            if noisy {
                ws.send(text(r#"{"op":11,"d":{"user_ids":["3"]},"seq":2}"#))
                    .await
                    .unwrap();
            }
            let key: Vec<u8> = (0..32).collect();
            ws.send(text(
                &serde_json::json!({
                    "op": 4,
                    "d": { "mode": "aead_xchacha20_poly1305_rtpsize", "secret_key": key,
                           "dave_protocol_version": 1 }
                })
                .to_string(),
            ))
            .await
            .unwrap();
            let next = ws.next().await.unwrap().unwrap();
            if matches!(next, WsMessage::Binary(_)) {
                ws.next().await.unwrap().unwrap(); // speaking after the key package
            }
            script(ws).await
        });
        (format!("ws://{addr}"), task)
    }

    async fn connect(ws_url: String) -> LiveCall {
        LiveCall::connect_role(
            MediaRole::Voice,
            test_target(ws_url),
            DaveEngine::Scripted(ScriptedDave::default()),
            quick_schedule(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn handshake_skips_unknown_opcodes_and_clients_connect() {
        let (url, server) = control_server(45000, true, |_ws| async {}).await;
        let call = connect(url).await;
        assert_eq!(call.ssrc, 77);
        assert_eq!(call.phase, VoicePhase::Connected);
        assert_eq!(call.last_seq, Some(2));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn binary_dave_frames_reach_dave_and_commits_go_out_as_binary() {
        let (url, server) = control_server(45000, false, |mut ws| async move {
            // [seq u16 BE][op 25 external sender][payload]
            ws.send(WsMessage::Binary(vec![0, 9, 25, 1, 2, 3].into()))
                .await
                .unwrap();
            ws.send(text(r#"{"op":6,"d":null}"#)).await.unwrap();
            ws.next().await.unwrap().unwrap()
        })
        .await;
        let mut call = connect(url).await;
        call.poll_control().await.unwrap();
        let DaveEngine::Scripted(dave) = &call.session.dave else {
            panic!("scripted dave");
        };
        assert_eq!(dave.external_senders, 1);
        assert_eq!(call.last_seq, Some(9));
        call.session
            .commits_to_send
            .push(crate::dave::OutgoingCommit {
                commit: vec![1, 2],
                welcome: Some(vec![3]),
            });
        call.poll_control().await.unwrap();
        // [op 28 commit+welcome][commit][welcome], sent as a binary frame.
        let WsMessage::Binary(sent) = server.await.unwrap() else {
            panic!("commit was not sent as a binary frame");
        };
        assert_eq!(&sent[..], &[28, 1, 2, 3]);
    }

    #[tokio::test]
    async fn heartbeat_is_sent_each_interval_and_acks_the_last_seq() {
        let (url, server) = control_server(400, false, |mut ws| async move {
            ws.send(text(r#"{"op":5,"d":{"ssrc":9,"speaking":0},"seq":7}"#))
                .await
                .unwrap();
            let beat = ws.next().await.unwrap().unwrap().into_text().unwrap();
            ws.send(text(r#"{"op":6,"d":null}"#)).await.unwrap();
            beat.to_string()
        })
        .await;
        let mut call = connect(url).await;
        call.poll_control().await.unwrap(); // the seq-7 frame
        call.poll_control().await.unwrap(); // beats, then reads the ack
        let beat: serde_json::Value = serde_json::from_str(&server.await.unwrap()).unwrap();
        assert_eq!(beat["op"], 3);
        assert_eq!(beat["d"]["seq_ack"], 7);
        assert!(!call.awaiting_ack);
    }

    #[tokio::test]
    async fn a_missing_heartbeat_ack_ends_the_call() {
        let (url, _server) = control_server(30, false, |mut ws| async move {
            while ws.next().await.is_some() {} // read beats, never ack
        })
        .await;
        let mut call = connect(url).await;
        let err = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                call.poll_control().await?;
            }
            #[allow(unreachable_code)]
            Ok::<(), VoiceLinkError>(())
        })
        .await
        .expect("poll_control must give up on a missing ack")
        .unwrap_err();
        assert!(
            matches!(err, VoiceLinkError::Timeout("heartbeat ack")),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_quiet_control_channel_is_not_an_error() {
        let (url, _server) = control_server(45000, false, |mut ws| async move {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            ws.send(text(r#"{"op":6,"d":null}"#)).await.unwrap();
            ws.next().await;
        })
        .await;
        let mut call = connect(url).await;
        call.poll_control().await.unwrap();
    }
}
