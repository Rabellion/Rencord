//! Voice websocket state machine.
//!
//! The reducer is synchronous so tests can feed it frames. The desktop task
//! owns the socket and calls [`VoiceSession::handle`]. A DAVE failure is
//! recorded and the transport stays up; the media path is not torn down when
//! an MLS commit is rejected.

use crate::dave::{DaveEngine, DaveOps, OutgoingCommit};
use rencord_protocol::{
    voice_identify, voice_select_protocol, voice_speaking, SessionDescription, VoiceFrame,
    VoiceHello, VoiceOpcode, VoiceReady, TRANSPORT_MODE,
};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoicePhase {
    Idle,
    Identifying,
    WaitingForReady,
    Discovering,
    SelectingProtocol,
    AwaitingDescription,
    Connected,
    TransportOnly,
}

#[derive(Clone, Debug)]
pub struct VoiceFailure {
    pub opcode: u8,
    pub message: String,
}

pub struct VoiceSession {
    pub phase: VoicePhase,
    pub ssrc: Option<u32>,
    pub secret_key: Option<Vec<u8>>,
    pub dave_version: u16,
    pub heartbeat_interval: u64,
    pub failures: Vec<VoiceFailure>,
    pub commits_to_send: Vec<OutgoingCommit>,
    pub dave: DaveEngine,
    /// Set when a new MLS key package (opcode 26) must be sent.
    pub pending_key_package: bool,
    pub recognized: Vec<u64>,
    pub last_transition: Option<u16>,
    server_id: String,
    user_id: String,
    session_id: String,
    token: String,
}

impl VoiceSession {
    pub fn new(
        server_id: impl Into<String>,
        user_id: impl Into<String>,
        session_id: impl Into<String>,
        token: impl Into<String>,
        dave: DaveEngine,
    ) -> Self {
        Self {
            phase: VoicePhase::Idle,
            ssrc: None,
            secret_key: None,
            dave_version: 0,
            heartbeat_interval: 0,
            failures: Vec::new(),
            commits_to_send: Vec::new(),
            dave,
            pending_key_package: false,
            recognized: Vec::new(),
            last_transition: None,
            server_id: server_id.into(),
            user_id: user_id.into(),
            session_id: session_id.into(),
            token: token.into(),
        }
    }

    pub fn identify_message(&mut self) -> Value {
        self.phase = VoicePhase::WaitingForReady;
        voice_identify(
            &self.server_id,
            &self.user_id,
            &self.session_id,
            &self.token,
        )
    }

    pub fn select_protocol_message(&mut self, address: &str, port: u16) -> Value {
        self.phase = VoicePhase::SelectingProtocol;
        voice_select_protocol(address, port, TRANSPORT_MODE)
    }

    pub fn speaking_message(&self, speaking: bool) -> Option<Value> {
        self.ssrc.map(|ssrc| voice_speaking(ssrc, speaking))
    }

    pub fn handle(&mut self, frame: VoiceFrame) -> Vec<Value> {
        let mut outgoing = Vec::new();
        match frame.op {
            VoiceOpcode::Hello => {
                if let Ok(hello) = serde_json::from_value::<VoiceHello>(frame.data) {
                    self.heartbeat_interval = hello.heartbeat_interval;
                }
                outgoing.push(self.identify_message());
            }
            VoiceOpcode::Ready => match serde_json::from_value::<VoiceReady>(frame.data) {
                Ok(ready) => {
                    self.ssrc = Some(ready.ssrc);
                    self.phase = VoicePhase::Discovering;
                }
                Err(err) => self.fail(2, err.to_string()),
            },
            VoiceOpcode::SessionDescription => {
                match serde_json::from_value::<SessionDescription>(frame.data) {
                    Ok(description) => {
                        self.secret_key = Some(description.secret_key);
                        self.dave_version = description.dave_protocol_version.unwrap_or(0);
                        self.phase = if self.dave_version == 0 {
                            VoicePhase::TransportOnly
                        } else {
                            VoicePhase::Connected
                        };
                        // A non-zero select-protocol ack means this member must
                        // present a fresh MLS key package.
                        if self.dave_version > 0 {
                            self.pending_key_package = true;
                        }
                    }
                    Err(err) => self.fail(4, err.to_string()),
                }
            }
            VoiceOpcode::HeartbeatAck | VoiceOpcode::Resumed | VoiceOpcode::Speaking => {}
            VoiceOpcode::ClientDisconnect => {}
            VoiceOpcode::DavePrepareTransition => {
                let id = json_u16(&frame.data, "transition_id");
                let version = json_u16(&frame.data, "protocol_version");
                self.last_transition = Some(id);
                if let Err(err) = self.dave.prepare_transition(id, version) {
                    self.fail(21, err);
                } else if id == 0 {
                    // The initialization transition executes immediately.
                    if let Err(err) = self.dave.execute_transition(0) {
                        self.fail(21, err);
                    }
                } else {
                    outgoing.push(transition_ready(id));
                }
                if version == 0 {
                    self.dave_version = 0;
                    self.phase = VoicePhase::TransportOnly;
                    self.pending_key_package = false;
                }
            }
            VoiceOpcode::DaveExecuteTransition => {
                let id = frame
                    .data
                    .get("transition_id")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u16;
                if let Err(err) = self.dave.execute_transition(id) {
                    self.fail(22, err);
                }
            }
            VoiceOpcode::DavePrepareEpoch => {
                let epoch = frame
                    .data
                    .get("epoch")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let version = json_u16(&frame.data, "protocol_version");
                if epoch == 1 {
                    self.dave_version = version;
                    if let Err(err) = self.dave.reset_group() {
                        self.fail(24, err);
                    }
                    // A recreated group needs a new key package.
                    if version > 0 {
                        self.pending_key_package = true;
                    }
                }
            }
            VoiceOpcode::ClientsConnect => {
                if let Some(ids) = frame.data.get("user_ids").and_then(|v| v.as_array()) {
                    for id in ids {
                        let parsed = id
                            .as_str()
                            .and_then(|text| text.parse().ok())
                            .or_else(|| id.as_u64());
                        if let Some(user) = parsed {
                            if !self.recognized.contains(&user) {
                                self.recognized.push(user);
                            }
                        }
                    }
                }
            }
            VoiceOpcode::MlsExternalSender => {
                let bytes = payload_bytes(&frame.data);
                if let Err(err) = self.dave.on_external_sender(&bytes) {
                    self.fail(25, err);
                }
            }
            VoiceOpcode::MlsProposals => match proposal_body(&frame.data) {
                Ok((append, body)) => match self.dave.on_proposals(append, &body) {
                    Ok(Some(commit)) => self.commits_to_send.push(commit),
                    Ok(None) => {}
                    Err(err) => self.fail(27, err),
                },
                Err(err) => self.fail(27, err),
            },
            VoiceOpcode::MlsWelcome => match transition_body(&frame.data) {
                Ok((id, body)) => {
                    self.last_transition = Some(id);
                    if let Err(err) = self.dave.on_welcome(&body, &self.recognized) {
                        self.fail(30, err);
                        outgoing.push(invalid_commit(id));
                        self.pending_key_package = true;
                    } else if id != 0 {
                        outgoing.push(transition_ready(id));
                    }
                }
                Err(err) => {
                    self.fail(30, err);
                    outgoing.push(invalid_commit(0));
                    self.pending_key_package = true;
                }
            },
            VoiceOpcode::MlsAnnounceCommitTransition => match transition_body(&frame.data) {
                Ok((id, body)) => {
                    self.last_transition = Some(id);
                    if let Err(err) = self.dave.on_commit(&body) {
                        self.fail(29, err);
                        outgoing.push(invalid_commit(id));
                        self.pending_key_package = true;
                    } else if id != 0 {
                        outgoing.push(transition_ready(id));
                    }
                }
                Err(err) => {
                    self.fail(29, err);
                    outgoing.push(invalid_commit(0));
                    self.pending_key_package = true;
                }
            },
            VoiceOpcode::MlsCommitWelcome => {
                // Opcode 28 is client-to-server. A copy echoed back is ignored.
            }
            _ => {}
        }
        outgoing
    }

    /// Opcode 26 payload, once. `None` when DAVE is off or nothing is due.
    pub fn take_key_package(&mut self) -> Option<Vec<u8>> {
        if !self.pending_key_package || self.dave_version == 0 {
            return None;
        }
        match self.dave.create_key_package() {
            Ok(bytes) => {
                self.pending_key_package = false;
                Some(bytes)
            }
            Err(err) => {
                self.fail(26, err);
                None
            }
        }
    }

    pub fn seal_opus(&mut self, packet: &[u8]) -> Result<Vec<u8>, String> {
        if self.dave.can_encrypt() {
            self.dave.encrypt_opus(packet)
        } else if self.dave_version > 0 && !matches!(self.phase, VoicePhase::TransportOnly) {
            Err("waiting for DAVE keys; refusing to send plaintext".into())
        } else {
            Ok(packet.to_vec())
        }
    }

    fn fail(&mut self, opcode: u8, message: String) {
        self.failures.push(VoiceFailure { opcode, message });
    }
}

fn json_u16(value: &Value, key: &str) -> u16 {
    value.get(key).and_then(|item| item.as_u64()).unwrap_or(0) as u16
}

fn transition_ready(id: u16) -> Value {
    json!({
        "op": VoiceOpcode::DaveTransitionReady as u8,
        "d": { "transition_id": id }
    })
}

fn invalid_commit(id: u16) -> Value {
    json!({
        "op": VoiceOpcode::MlsInvalidCommitWelcome as u8,
        "d": { "transition_id": id }
    })
}

/// Opcode 27: the first byte is append (0) or revoke (1). The rest is MLS.
fn proposal_body(value: &Value) -> Result<(bool, Vec<u8>), String> {
    let bytes = payload_bytes(value);
    let Some((kind, body)) = bytes.split_first() else {
        return Err("proposals frame is missing the operation type".into());
    };
    let append = match kind {
        0 => true,
        1 => false,
        other => return Err(format!("unknown proposals operation {other}")),
    };
    Ok((append, body.to_vec()))
}

/// Opcodes 29 and 30: a big-endian transition id, then the MLS message.
fn transition_body(value: &Value) -> Result<(u16, Vec<u8>), String> {
    let bytes = payload_bytes(value);
    if bytes.len() < 2 {
        return Err("DAVE frame is missing the transition id".into());
    }
    let id = u16::from_be_bytes([bytes[0], bytes[1]]);
    Ok((id, bytes[2..].to_vec()))
}

/// Opcode 28 is the commit MLSMessage, then a Welcome only when the commit adds members.
pub fn commit_welcome_payload(commit: &[u8], welcome: Option<&[u8]>) -> Vec<u8> {
    let mut payload = commit.to_vec();
    if let Some(welcome) = welcome {
        payload.extend_from_slice(welcome);
    }
    payload
}

fn payload_bytes(value: &Value) -> Vec<u8> {
    if let Some(bytes) = value.get("data").and_then(|item| item.as_array()) {
        return bytes
            .iter()
            .filter_map(|item| item.as_u64().map(|byte| byte as u8))
            .collect();
    }
    if let Some(text) = value.as_str() {
        return text.as_bytes().to_vec();
    }
    if let Some(text) = value.get("data").and_then(|item| item.as_str()) {
        return text.as_bytes().to_vec();
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dave::{DaveEngine, ScriptedDave};
    use rencord_protocol::VoiceOpcode;

    fn session() -> VoiceSession {
        VoiceSession::new(
            "10",
            "1",
            "sess",
            "voice-token",
            DaveEngine::Scripted(ScriptedDave::default()),
        )
    }

    #[test]
    fn hello_ready_and_description_reach_connected() {
        let mut voice = session();
        let hello = VoiceFrame::parse(br#"{"op":8,"d":{"heartbeat_interval":1000}}"#).unwrap();
        let replies = voice.handle(hello);
        assert_eq!(replies[0]["op"], VoiceOpcode::Identify as u8);
        assert_eq!(voice.phase, VoicePhase::WaitingForReady);
        let ready = VoiceFrame::parse(
            br#"{"op":2,"d":{"ssrc":42,"ip":"1.2.3.4","port":9,"modes":["aead_xchacha20_poly1305_rtpsize"]}}"#,
        )
        .unwrap();
        voice.handle(ready);
        assert_eq!(voice.ssrc, Some(42));
        assert_eq!(voice.phase, VoicePhase::Discovering);
        let select = voice.select_protocol_message("203.0.113.8", 50000);
        assert_eq!(
            select["d"]["data"]["mode"],
            "aead_xchacha20_poly1305_rtpsize"
        );
        let mut key = vec![0u8; 32];
        key[0] = 4;
        let description = VoiceFrame {
            seq: None,
            op: VoiceOpcode::SessionDescription,
            data: serde_json::json!({
                "mode": "aead_xchacha20_poly1305_rtpsize",
                "secret_key": key,
                "dave_protocol_version": 1
            }),
        };
        voice.handle(description);
        assert_eq!(voice.phase, VoicePhase::Connected);
        assert_eq!(voice.secret_key.unwrap().len(), 32);
    }

    #[test]
    fn dave_opcodes_are_forwarded_and_a_bad_welcome_does_not_drop_transport() {
        let mut voice = session();
        voice.phase = VoicePhase::Connected;
        voice.secret_key = Some(vec![1; 32]);
        voice.handle(VoiceFrame {
            seq: None,
            op: VoiceOpcode::MlsExternalSender,
            data: serde_json::json!({"data": [1, 2, 3]}),
        });
        voice.handle(VoiceFrame {
            seq: None,
            op: VoiceOpcode::DavePrepareTransition,
            data: serde_json::json!({"transition_id": 7, "protocol_version": 1}),
        });
        voice.handle(VoiceFrame {
            seq: None,
            op: VoiceOpcode::DaveExecuteTransition,
            data: serde_json::json!({"transition_id": 7}),
        });
        let DaveEngine::Scripted(scripted) = &voice.dave else {
            panic!("scripted");
        };
        assert_eq!(scripted.external_senders, 1);
        assert_eq!(scripted.prepares, vec![(7, 1)]);
        assert_eq!(scripted.executes, vec![7]);
        assert!(voice.failures.is_empty());
        let sealed = voice.seal_opus(b"frame").unwrap();
        assert_eq!(sealed[0], 0xDA);

        voice.dave = DaveEngine::Scripted(ScriptedDave::default());
        // Welcome with empty bytes is still forwarded. Scripted accepts it.
        // A transport key remains installed either way.
        voice.handle(VoiceFrame {
            seq: None,
            op: VoiceOpcode::MlsWelcome,
            data: serde_json::json!({"data": []}),
        });
        assert!(voice.secret_key.is_some());
        assert_ne!(voice.phase, VoicePhase::Idle);
    }

    #[test]
    fn session_description_requests_a_key_package_and_prefixes_are_stripped() {
        let mut voice = session();
        voice.dave_version = 1;
        voice.handle(VoiceFrame {
            seq: None,
            op: VoiceOpcode::SessionDescription,
            data: serde_json::json!({
                "mode": "aead_xchacha20_poly1305_rtpsize",
                "secret_key": vec![4u8; 32],
                "dave_protocol_version": 1
            }),
        });
        let package = voice.take_key_package().expect("key package");
        assert_eq!(package, b"key-package");
        assert!(voice.take_key_package().is_none());

        let replies = voice.handle(VoiceFrame {
            seq: None,
            op: VoiceOpcode::MlsProposals,
            data: serde_json::json!({"data": [0, 9, 8, 7]}),
        });
        assert!(replies.is_empty());
        let DaveEngine::Scripted(scripted) = &voice.dave else {
            panic!("scripted");
        };
        assert_eq!(scripted.last_append, Some(true));
        assert_eq!(scripted.last_body, vec![9, 8, 7]);

        let replies = voice.handle(VoiceFrame {
            seq: None,
            op: VoiceOpcode::MlsAnnounceCommitTransition,
            data: serde_json::json!({"data": [0, 12, 1, 2, 3]}),
        });
        assert_eq!(replies[0]["op"], VoiceOpcode::DaveTransitionReady as u8);
        assert_eq!(replies[0]["d"]["transition_id"], 12);
        let DaveEngine::Scripted(scripted) = &voice.dave else {
            panic!("scripted");
        };
        assert_eq!(scripted.last_body, vec![1, 2, 3]);

        voice.handle(VoiceFrame {
            seq: None,
            op: VoiceOpcode::DavePrepareEpoch,
            data: serde_json::json!({"epoch": 1, "protocol_version": 1}),
        });
        assert_eq!(
            voice.take_key_package().as_deref(),
            Some(&b"key-package"[..])
        );
        let DaveEngine::Scripted(scripted) = &voice.dave else {
            panic!("scripted");
        };
        assert_eq!(scripted.resets, 1);

        let welcome = voice.handle(VoiceFrame {
            seq: None,
            op: VoiceOpcode::MlsWelcome,
            data: serde_json::json!({"data": [0, 4, 5]}),
        });
        assert_eq!(welcome[0]["d"]["transition_id"], 4);
        let payload = commit_welcome_payload(&[1, 2], Some(&[3, 4]));
        assert_eq!(payload, vec![1, 2, 3, 4]);
        assert_eq!(commit_welcome_payload(&[1, 2], None), vec![1, 2]);
    }

    #[test]
    fn version_zero_downgrades_to_passthrough() {
        let mut voice = session();
        voice.dave_version = 1;
        voice.phase = VoicePhase::Connected;
        let replies = voice.handle(VoiceFrame {
            seq: None,
            op: VoiceOpcode::DavePrepareTransition,
            data: serde_json::json!({"transition_id": 3, "protocol_version": 0}),
        });
        assert_eq!(voice.phase, VoicePhase::TransportOnly);
        assert_eq!(voice.dave_version, 0);
        assert_eq!(replies[0]["op"], VoiceOpcode::DaveTransitionReady as u8);
        let sealed = voice.seal_opus(b"plain").unwrap();
        assert_eq!(sealed, b"plain");
    }
}
