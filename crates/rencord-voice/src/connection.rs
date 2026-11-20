//! Voice websocket plus UDP.
//!
//! `LiveCall::connect_with` speaks the voice gateway (identify or resume,
//! select protocol, speaking, DAVE opcodes) and discovers the public UDP
//! address. Tests point it at a local stub. A real Discord voice server was
//! not used.

use crate::crypto::{discovery_request, parse_discovery_response};
use crate::dave::DaveEngine;
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
    #[error("discovery response was not understood")]
    Discovery,
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
    target: VoiceTarget,
    write: futures_util::stream::SplitSink<VoiceSocket, WsMessage>,
    read: futures_util::stream::SplitStream<VoiceSocket>,
    udp: UdpSocket,
    server_udp: SocketAddr,
    session: VoiceSession,
    audio: CallAudio,
}

impl LiveCall {
    pub async fn connect_with(
        target: VoiceTarget,
        dave: DaveEngine,
    ) -> Result<Self, VoiceLinkError> {
        let (socket, _) = connect_async(&target.ws_url).await?;
        let (write, read) = socket.split();
        let mut call = Self {
            phase: VoicePhase::Idle,
            ssrc: 0,
            external_address: String::new(),
            external_port: 0,
            muted: false,
            deafened: false,
            seq_ack: 0,
            session: VoiceSession::new(
                target.server_id.clone(),
                target.user_id.clone(),
                target.session_id.clone(),
                target.token.clone(),
                dave,
            ),
            target,
            write,
            read,
            udp: UdpSocket::bind("127.0.0.1:0").await?,
            server_udp: SocketAddr::from(([127, 0, 0, 1], 1)),
            audio: CallAudio::new(1, &[7; 32])?,
        };
        call.finish_handshake(false).await?;
        Ok(call)
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
        let mut buf = vec![0u8; 2048];
        let read = tokio::time::timeout(wait, self.udp.recv(&mut buf))
            .await
            .map_err(|_| VoiceLinkError::Timeout("udp audio"))??;
        if self.deafened {
            return Ok(vec![0; FRAME_SAMPLES]);
        }
        self.seq_ack = self.seq_ack.wrapping_add(1);
        self.audio
            .on_rtp(&buf[..read], self.seq_ack.saturating_mul(20), self.ssrc)
            .map_err(VoiceLinkError::from)
    }

    /// Drop the websocket and resume the same UDP media path.
    pub async fn resume_gateway(&mut self) -> Result<(), VoiceLinkError> {
        let (socket, _) = connect_async(&self.target.ws_url).await?;
        let (write, read) = socket.split();
        self.write = write;
        self.read = read;
        self.finish_handshake(true).await
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
        self.server_udp = format!("{}:{}", ready.ip, ready.port)
            .parse()
            .map_err(|err| VoiceLinkError::Protocol(format!("{err}")))?;
        self.udp
            .send_to(&discovery_request(self.ssrc), self.server_udp)
            .await?;
        let mut discovery = [0u8; 74];
        let size = tokio::time::timeout(Duration::from_secs(2), self.udp.recv(&mut discovery))
            .await
            .map_err(|_| VoiceLinkError::Timeout("ip discovery"))??;
        let (address, port) =
            parse_discovery_response(&discovery[..size]).ok_or(VoiceLinkError::Discovery)?;
        self.external_address = address.clone();
        self.external_port = port;
        self.udp
            .connect(self.server_udp)
            .await
            .map_err(VoiceLinkError::Udp)?;
        let select = self.session.select_protocol_message(&address, port);
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
                    response[..8].copy_from_slice(&buf[..8]);
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
}
