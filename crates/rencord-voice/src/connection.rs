//! Voice websocket plus UDP.
//!
//! `LiveCall::connect_with` speaks the voice gateway (identify or resume,
//! select protocol, speaking, DAVE opcodes) and discovers the public UDP
//! address. Tests point it at a local stub. A real Discord voice server was
//! not used.

use crate::dave::{DaveEngine, ScriptedDave};
use crate::discovery::{
    bind_for, discover, server_addr, DiscoveryError, DiscoverySchedule, MediaRole, PathWatch,
};
use crate::mixer::GainMixer;
use crate::opus::FRAME_SAMPLES;
use crate::pipeline::{PipelineError, VoicePipeline};
use crate::session::{VoicePhase, VoiceSession};
use futures_util::{SinkExt, StreamExt};
use rencord_protocol::{voice_resume, VoiceFrame, VoiceOpcode, VoiceReady, TRANSPORT_MODE};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio_tungstenite::{
    connect_async, tungstenite::Message as WsMessage, MaybeTlsStream, WebSocketStream,
};

type VoiceSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct FailedOpen {
    error: VoiceLinkError,
    dave: DaveEngine,
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
        let mut pipeline = VoicePipeline::new(ssrc)?;
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
    pub seq_ack: u64,
    pub role: MediaRole,
    pub probes: u32,
    target: VoiceTarget,
    write: futures_util::stream::SplitSink<VoiceSocket, WsMessage>,
    read: futures_util::stream::SplitStream<VoiceSocket>,
    udp: UdpSocket,
    server_udp: SocketAddr,
    session: VoiceSession,
    audio: CallAudio,
    path: PathWatch,
    schedule: DiscoverySchedule,
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
    ) -> Result<Self, FailedOpen> {
        let opened = match connect_async(&target.ws_url).await {
            Ok(opened) => opened,
            Err(err) => {
                return Err(FailedOpen {
                    error: err.into(),
                    dave,
                })
            }
        };
        let (socket, _) = opened;
        let (write, read) = socket.split();
        let udp = match UdpSocket::bind("127.0.0.1:0").await {
            Ok(udp) => udp,
            Err(err) => {
                return Err(FailedOpen {
                    error: err.into(),
                    dave,
                })
            }
        };
        let audio = match CallAudio::new(1, &[7; 32]) {
            Ok(audio) => audio,
            Err(err) => {
                return Err(FailedOpen {
                    error: err.into(),
                    dave,
                })
            }
        };
        let mut call = Self {
            phase: VoicePhase::Idle,
            ssrc: 0,
            external_address: String::new(),
            external_port: 0,
            muted: false,
            deafened: false,
            seq_ack: 0,
            role,
            probes: 0,
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
        };
        if let Err(error) = call.finish_handshake(false).await {
            return Err(FailedOpen {
                error,
                dave: call.take_dave(),
            });
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

    pub async fn send_microphone(&mut self, pcm: &[i16]) -> Result<(), VoiceLinkError> {
        let Some(packet) = self.audio.on_microphone(pcm)? else {
            return Ok(());
        };
        self.udp.send(&packet).await?;
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
            self.seq_ack = self.seq_ack.wrapping_add(1);
            return self
                .audio
                .on_rtp(&buf[..read], self.seq_ack.saturating_mul(20), self.ssrc)
                .map_err(VoiceLinkError::from);
        }
    }

    /// Send Discord's UDP ping when it is due, and resume when the path is quiet.
    pub async fn maintain(&mut self) -> Result<(), VoiceLinkError> {
        if self.path.dead() {
            self.resume_gateway().await?;
            self.path.inbound();
            return Ok(());
        }
        if self.path.keepalive_due() {
            let packet = self.path.next_keepalive();
            self.udp.send(&packet).await?;
        }
        Ok(())
    }

    /// Drop the websocket and resume the same media role.
    pub async fn resume_gateway(&mut self) -> Result<(), VoiceLinkError> {
        let mut probes = 0u32;
        let mut server = self.server_udp;
        for attempt in 1..=2 {
            let (socket, _) = connect_async(&self.target.ws_url).await?;
            let (write, read) = socket.split();
            self.write = write;
            self.read = read;
            match self.finish_handshake(true).await {
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

    async fn finish_handshake(&mut self, resume: bool) -> Result<(), VoiceLinkError> {
        let hello = self.read_frame("hello").await?;
        if resume {
            self.send_json(voice_resume(
                &self.target.server_id,
                &self.target.session_id,
                &self.target.token,
                self.seq_ack,
            ))
            .await?;
        } else {
            for message in self.session.handle(hello) {
                self.send_json(message).await?;
            }
        }
        let ready_frame = self.read_frame("ready").await?;
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
        let description = self.read_frame("session description").await?;
        if description.op != VoiceOpcode::SessionDescription {
            return Err(VoiceLinkError::Protocol(format!(
                "expected session description, got {:?}",
                description.op
            )));
        }
        for message in self.session.handle(description) {
            self.send_json(message).await?;
        }
        let key = self
            .session
            .secret_key
            .clone()
            .ok_or_else(|| VoiceLinkError::Protocol("missing secret key".into()))?;
        self.audio = CallAudio::new(self.ssrc, &key)?;
        self.audio.muted = self.muted;
        self.audio.mixer.set_deaf(self.deafened);
        if let Some(speaking) = self.session.speaking_message(!self.muted) {
            self.send_json(speaking).await?;
        }
        self.phase = self.session.phase;
        let _ = TRANSPORT_MODE;
        Ok(())
    }

    /// Read one control frame and apply DAVE / speaking updates.
    pub async fn poll_control(&mut self) -> Result<(), VoiceLinkError> {
        let frame = self.read_frame("control").await?;
        let outgoing = self.session.handle(frame);
        for message in outgoing {
            self.send_json(message).await?;
        }
        if let Some(speaking) = self.session.speaking_message(!self.muted) {
            let _ = speaking;
        }
        self.phase = self.session.phase;
        Ok(())
    }

    async fn read_frame(&mut self, what: &'static str) -> Result<VoiceFrame, VoiceLinkError> {
        let message = tokio::time::timeout(Duration::from_secs(2), self.read.next())
            .await
            .map_err(|_| VoiceLinkError::Timeout(what))?
            .ok_or(VoiceLinkError::Timeout(what))??;
        let text = match message {
            WsMessage::Text(text) => text.to_string(),
            WsMessage::Close(_) => return Err(VoiceLinkError::Timeout(what)),
            _ => return Err(VoiceLinkError::Protocol("non-text voice frame".into())),
        };
        VoiceFrame::parse(text.as_bytes()).map_err(|err| VoiceLinkError::Protocol(err.to_string()))
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
                    assert!(
                        first.contains("\"op\":7") || first.contains("\"op\":0"),
                        "{first}"
                    );
                } else {
                    assert!(first.contains("\"op\":0"), "{first}");
                }
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
                let speaking = socket.next().await.unwrap().unwrap().into_text().unwrap();
                assert!(speaking.contains("\"op\":5"), "{speaking}");
                socket
                    .send(WsMessage::Text(
                        r#"{"op":21,"d":{"transition_id":3,"protocol_version":1}}"#.into(),
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
}
