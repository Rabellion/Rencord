//! DAVE engine used by the voice session.
//!
//! [`LiveDave`] is the production path: it owns a [`davey::DaveSession`] and
//! forwards MLS and transition opcodes to it. [`ScriptedDave`] records those
//! calls so the voice state machine can be tested without an MLS group.
//! A separate integration test builds a real two-member group and checks that
//! Opus plaintext survives encrypt and decrypt.

use davey::{Codec, DaveSession, MediaType, ProposalsOperationType};
use std::num::NonZeroU16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutgoingCommit {
    pub commit: Vec<u8>,
    pub welcome: Option<Vec<u8>>,
}

pub trait DaveOps {
    fn on_external_sender(&mut self, data: &[u8]) -> Result<(), String>;
    fn on_proposals(&mut self, append: bool, data: &[u8])
        -> Result<Option<OutgoingCommit>, String>;
    fn on_welcome(&mut self, data: &[u8], recognized: &[u64]) -> Result<(), String>;
    fn on_commit(&mut self, data: &[u8]) -> Result<(), String>;
    fn prepare_transition(&mut self, id: u16, version: u16) -> Result<(), String>;
    fn execute_transition(&mut self, id: u16) -> Result<bool, String>;
    fn encrypt_opus(&mut self, packet: &[u8]) -> Result<Vec<u8>, String>;
    fn decrypt_opus(&mut self, user_id: u64, packet: &[u8]) -> Result<Vec<u8>, String>;
    fn encrypt_video(&mut self, packet: &[u8]) -> Result<Vec<u8>, String>;
    fn decrypt_video(&mut self, user_id: u64, packet: &[u8]) -> Result<Vec<u8>, String>;
    fn can_encrypt(&self) -> bool;
}

#[derive(Debug)]
pub enum DaveEngine {
    Live(Box<LiveDave>),
    Scripted(ScriptedDave),
}

#[derive(Debug)]
pub struct LiveDave {
    session: DaveSession,
}

impl DaveOps for DaveEngine {
    fn on_external_sender(&mut self, data: &[u8]) -> Result<(), String> {
        match self {
            Self::Live(engine) => engine.on_external_sender(data),
            Self::Scripted(engine) => engine.on_external_sender(data),
        }
    }
    fn on_proposals(
        &mut self,
        append: bool,
        data: &[u8],
    ) -> Result<Option<OutgoingCommit>, String> {
        match self {
            Self::Live(engine) => engine.on_proposals(append, data),
            Self::Scripted(engine) => engine.on_proposals(append, data),
        }
    }
    fn on_welcome(&mut self, data: &[u8], recognized: &[u64]) -> Result<(), String> {
        match self {
            Self::Live(engine) => engine.on_welcome(data, recognized),
            Self::Scripted(engine) => engine.on_welcome(data, recognized),
        }
    }
    fn on_commit(&mut self, data: &[u8]) -> Result<(), String> {
        match self {
            Self::Live(engine) => engine.on_commit(data),
            Self::Scripted(engine) => engine.on_commit(data),
        }
    }
    fn prepare_transition(&mut self, id: u16, version: u16) -> Result<(), String> {
        match self {
            Self::Live(engine) => engine.prepare_transition(id, version),
            Self::Scripted(engine) => engine.prepare_transition(id, version),
        }
    }
    fn execute_transition(&mut self, id: u16) -> Result<bool, String> {
        match self {
            Self::Live(engine) => engine.execute_transition(id),
            Self::Scripted(engine) => engine.execute_transition(id),
        }
    }
    fn encrypt_opus(&mut self, packet: &[u8]) -> Result<Vec<u8>, String> {
        match self {
            Self::Live(engine) => engine.encrypt_opus(packet),
            Self::Scripted(engine) => engine.encrypt_opus(packet),
        }
    }
    fn decrypt_opus(&mut self, user_id: u64, packet: &[u8]) -> Result<Vec<u8>, String> {
        match self {
            Self::Live(engine) => engine.decrypt_opus(user_id, packet),
            Self::Scripted(engine) => engine.decrypt_opus(user_id, packet),
        }
    }
    fn encrypt_video(&mut self, packet: &[u8]) -> Result<Vec<u8>, String> {
        match self {
            Self::Live(engine) => engine.encrypt_video(packet),
            Self::Scripted(engine) => engine.encrypt_video(packet),
        }
    }
    fn decrypt_video(&mut self, user_id: u64, packet: &[u8]) -> Result<Vec<u8>, String> {
        match self {
            Self::Live(engine) => engine.decrypt_video(user_id, packet),
            Self::Scripted(engine) => engine.decrypt_video(user_id, packet),
        }
    }
    fn can_encrypt(&self) -> bool {
        match self {
            Self::Live(engine) => engine.can_encrypt(),
            Self::Scripted(engine) => engine.can_encrypt(),
        }
    }
}

impl LiveDave {
    pub fn new(user_id: u64, channel_id: u64) -> Result<Self, String> {
        let version = NonZeroU16::new(1).expect("1");
        let session =
            DaveSession::new(version, user_id, channel_id, None).map_err(|err| err.to_string())?;
        Ok(Self { session })
    }

    pub fn session_mut(&mut self) -> &mut DaveSession {
        &mut self.session
    }
}

impl DaveOps for LiveDave {
    fn on_external_sender(&mut self, data: &[u8]) -> Result<(), String> {
        self.session
            .set_external_sender(data)
            .map_err(|err| err.to_string())
    }

    fn on_proposals(
        &mut self,
        append: bool,
        data: &[u8],
    ) -> Result<Option<OutgoingCommit>, String> {
        let kind = if append {
            ProposalsOperationType::APPEND
        } else {
            ProposalsOperationType::REVOKE
        };
        match self
            .session
            .process_proposals(kind, data, None)
            .map_err(|err| err.to_string())?
        {
            Some(commit) => Ok(Some(OutgoingCommit {
                commit: commit.commit,
                welcome: commit.welcome,
            })),
            None => Ok(None),
        }
    }

    fn on_welcome(&mut self, data: &[u8], recognized: &[u64]) -> Result<(), String> {
        self.session
            .process_welcome(data, Some(recognized))
            .map_err(|err| err.to_string())
    }

    fn on_commit(&mut self, data: &[u8]) -> Result<(), String> {
        self.session
            .process_commit(data)
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    fn prepare_transition(&mut self, id: u16, version: u16) -> Result<(), String> {
        self.session
            .prepare_transition(id, version)
            .map_err(|err| err.to_string())
    }

    fn execute_transition(&mut self, id: u16) -> Result<bool, String> {
        self.session
            .execute_transition(id)
            .map_err(|err| err.to_string())
    }

    fn encrypt_opus(&mut self, packet: &[u8]) -> Result<Vec<u8>, String> {
        if !self.session.can_encrypt() {
            return Err("DAVE session cannot encrypt yet".into());
        }
        self.session
            .encrypt(MediaType::AUDIO, Codec::OPUS, packet)
            .map(|frame| frame.into_owned())
            .map_err(|err| err.to_string())
    }

    fn decrypt_opus(&mut self, user_id: u64, packet: &[u8]) -> Result<Vec<u8>, String> {
        self.session
            .decrypt(user_id, MediaType::AUDIO, packet)
            .map_err(|err| err.to_string())
    }

    fn encrypt_video(&mut self, packet: &[u8]) -> Result<Vec<u8>, String> {
        if !self.session.can_encrypt() {
            return Err("DAVE session cannot encrypt yet".into());
        }
        self.session
            .encrypt(MediaType::VIDEO, Codec::H264, packet)
            .map(|frame| frame.into_owned())
            .map_err(|err| err.to_string())
    }

    fn decrypt_video(&mut self, user_id: u64, packet: &[u8]) -> Result<Vec<u8>, String> {
        self.session
            .decrypt(user_id, MediaType::VIDEO, packet)
            .map_err(|err| err.to_string())
    }

    fn can_encrypt(&self) -> bool {
        self.session.can_encrypt()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScriptedDave {
    pub external_senders: u32,
    pub proposals: u32,
    pub welcomes: u32,
    pub commits: u32,
    pub prepares: Vec<(u16, u16)>,
    pub executes: Vec<u16>,
    pub ready: bool,
}

impl DaveOps for ScriptedDave {
    fn on_external_sender(&mut self, _data: &[u8]) -> Result<(), String> {
        self.external_senders += 1;
        Ok(())
    }
    fn on_proposals(
        &mut self,
        _append: bool,
        _data: &[u8],
    ) -> Result<Option<OutgoingCommit>, String> {
        self.proposals += 1;
        Ok(None)
    }
    fn on_welcome(&mut self, _data: &[u8], _recognized: &[u64]) -> Result<(), String> {
        self.welcomes += 1;
        Ok(())
    }
    fn on_commit(&mut self, _data: &[u8]) -> Result<(), String> {
        self.commits += 1;
        Ok(())
    }
    fn prepare_transition(&mut self, id: u16, version: u16) -> Result<(), String> {
        self.prepares.push((id, version));
        if id == 0 && version > 0 {
            self.ready = true;
        }
        Ok(())
    }
    fn execute_transition(&mut self, id: u16) -> Result<bool, String> {
        self.executes.push(id);
        self.ready = true;
        Ok(true)
    }
    fn encrypt_opus(&mut self, packet: &[u8]) -> Result<Vec<u8>, String> {
        if !self.ready {
            return Err("scripted dave is not ready".into());
        }
        let mut out = vec![0xDA];
        out.extend_from_slice(packet);
        Ok(out)
    }
    fn decrypt_opus(&mut self, _user_id: u64, packet: &[u8]) -> Result<Vec<u8>, String> {
        packet
            .strip_prefix(&[0xDA])
            .map(|rest| rest.to_vec())
            .ok_or_else(|| "not a scripted dave frame".into())
    }
    fn encrypt_video(&mut self, packet: &[u8]) -> Result<Vec<u8>, String> {
        if !self.ready {
            return Err("scripted dave is not ready".into());
        }
        let mut out = vec![0xDB];
        out.extend_from_slice(packet);
        Ok(out)
    }
    fn decrypt_video(&mut self, _user_id: u64, packet: &[u8]) -> Result<Vec<u8>, String> {
        packet
            .strip_prefix(&[0xDB])
            .map(|rest| rest.to_vec())
            .ok_or_else(|| "not a scripted dave video frame".into())
    }
    fn can_encrypt(&self) -> bool {
        self.ready
    }
}
