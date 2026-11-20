//! Two-member DAVE group: encrypt an Opus packet with one session and decrypt
//! it with the other. The external sender stands in for Discord's voice
//! gateway, using the public OpenMLS types davey already depends on.

use std::num::NonZeroU16;

use davey::{DaveSession, ProposalsOperationType};
use openmls::prelude::{tls_codec::Serialize, *};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use rencord_protocol::VoiceOpcode;
use rencord_voice::dave::{DaveEngine, DaveOps, LiveDave};
use rencord_voice::session::{commit_welcome_payload, VoiceSession};
use serde_json::json;

const VERSION: NonZeroU16 = match NonZeroU16::new(1) {
    Some(value) => value,
    None => unreachable!(),
};

struct ExternalSenderHarness {
    provider: OpenMlsRustCrypto,
    group_id: GroupId,
    signer: SignatureKeyPair,
    external_sender: ExternalSender,
}

impl ExternalSenderHarness {
    fn new(channel_id: u64) -> Self {
        let ciphersuite = Ciphersuite::MLS_128_DHKEMP256_AES128GCM_SHA256_P256;
        let signer = SignatureKeyPair::new(ciphersuite.signature_algorithm()).unwrap();
        let external_sender = ExternalSender::new(
            signer.public().into(),
            BasicCredential::new(vec![0x00, 0x01, 0x01, 0x00]).into(),
        );
        Self {
            provider: OpenMlsRustCrypto::default(),
            group_id: GroupId::from_slice(&channel_id.to_be_bytes()),
            signer,
            external_sender,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        self.external_sender.tls_serialize_detached().unwrap()
    }

    fn propose_add(&self, key_package: &[u8]) -> Vec<u8> {
        let key_package = KeyPackageIn::tls_deserialize_exact_bytes(key_package)
            .unwrap()
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .unwrap();
        let message = ExternalProposal::new_add::<OpenMlsRustCrypto>(
            key_package,
            self.group_id.clone(),
            GroupEpoch::from(0u64),
            &self.signer,
            SenderExtensionIndex::new(0),
        )
        .unwrap();
        VLBytes::new(message.tls_serialize_detached().unwrap())
            .tls_serialize_detached()
            .unwrap()
    }
}

#[test]
fn two_members_roundtrip_an_opus_frame() {
    let channel = 4242u64;
    let user_a = 111u64;
    let user_b = 222u64;
    let gateway = ExternalSenderHarness::new(channel);
    let mut alice = DaveSession::new(VERSION, user_a, channel, None).unwrap();
    let mut bob = DaveSession::new(VERSION, user_b, channel, None).unwrap();
    alice.set_external_sender(&gateway.bytes()).unwrap();
    bob.set_external_sender(&gateway.bytes()).unwrap();

    let key_package = bob.create_key_package().unwrap();
    let proposal = gateway.propose_add(&key_package);
    let commit = alice
        .process_proposals(
            ProposalsOperationType::APPEND,
            &proposal,
            Some(&[user_a, user_b]),
        )
        .unwrap()
        .expect("commit");
    assert!(alice.process_commit(&commit.commit).unwrap());
    bob.process_welcome(commit.welcome.as_ref().unwrap(), Some(&[user_a, user_b]))
        .unwrap();

    alice.prepare_transition(0, 1).unwrap();
    bob.prepare_transition(0, 1).unwrap();
    assert!(alice.can_encrypt());

    let plaintext = b"\xf8\xff\xfe-not-silence-opus";
    let encrypted = alice.encrypt_opus(plaintext).unwrap();
    assert_ne!(encrypted.as_ref(), plaintext);
    let decrypted = bob
        .decrypt(user_a, davey::MediaType::AUDIO, encrypted.as_ref())
        .unwrap();
    assert_eq!(decrypted, plaintext);
}

fn frame(op: VoiceOpcode, data: serde_json::Value) -> rencord_protocol::VoiceFrame {
    rencord_protocol::VoiceFrame {
        op,
        data,
        seq: Some(1),
    }
}

fn bytes_json(bytes: &[u8]) -> serde_json::Value {
    json!({ "data": bytes })
}

/// The voice session, not davey directly, must strip the binary prefixes and
/// send a key package. Feeding the prefixed bytes to the old parser makes
/// MLS reject the message, so this test fails before the prefix fix.
#[test]
fn binary_dave_sequence_sends_a_key_package_parses_prefixes_and_decrypts() {
    let channel = 5150u64;
    let user_a = 111u64;
    let user_b = 222u64;
    let gateway = ExternalSenderHarness::new(channel);
    let mut alice = VoiceSession::new(
        channel.to_string(),
        user_a.to_string(),
        "sess-a",
        "token",
        DaveEngine::Live(Box::new(LiveDave::new(user_a, channel).unwrap())),
    );
    let mut bob = VoiceSession::new(
        channel.to_string(),
        user_b.to_string(),
        "sess-b",
        "token",
        DaveEngine::Live(Box::new(LiveDave::new(user_b, channel).unwrap())),
    );
    let described = |version: u16| {
        frame(
            VoiceOpcode::SessionDescription,
            json!({
                "mode": "aead_xchacha20_poly1305_rtpsize",
                "secret_key": vec![7u8; 32],
                "dave_protocol_version": version
            }),
        )
    };
    alice.handle(described(1));
    bob.handle(described(1));
    let alice_package = alice.take_key_package().expect("alice key package");
    let bob_package = bob.take_key_package().expect("bob key package");
    assert!(!alice_package.is_empty());
    assert!(!bob_package.is_empty());

    let sender = frame(VoiceOpcode::MlsExternalSender, bytes_json(&gateway.bytes()));
    alice.handle(sender.clone());
    bob.handle(sender);
    assert!(alice.failures.is_empty(), "{:?}", alice.failures);
    assert!(bob.failures.is_empty(), "{:?}", bob.failures);

    for user in [user_a, user_b] {
        let connect = frame(
            VoiceOpcode::ClientsConnect,
            json!({"user_ids": [user.to_string()]}),
        );
        alice.handle(connect.clone());
        bob.handle(connect);
    }

    let proposal = gateway.propose_add(&bob_package);
    let mut prefixed = vec![0u8];
    prefixed.extend_from_slice(&proposal);
    alice.handle(frame(VoiceOpcode::MlsProposals, bytes_json(&prefixed)));
    assert!(
        alice.failures.is_empty(),
        "prefix leaked into MLS: {:?}",
        alice.failures
    );
    let commit = alice.commits_to_send.pop().expect("commit");
    let wire = commit_welcome_payload(&commit.commit, commit.welcome.as_deref());
    assert!(wire.starts_with(&commit.commit));
    assert!(
        wire.len() > commit.commit.len(),
        "add commit includes a welcome"
    );

    let mut announced = 7u16.to_be_bytes().to_vec();
    announced.extend_from_slice(&commit.commit);
    let ready = alice.handle(frame(
        VoiceOpcode::MlsAnnounceCommitTransition,
        bytes_json(&announced),
    ));
    assert_eq!(ready[0]["op"], VoiceOpcode::DaveTransitionReady as u8);
    assert_eq!(ready[0]["d"]["transition_id"], 7);
    assert!(alice.failures.is_empty(), "{:?}", alice.failures);

    let mut welcomed = 7u16.to_be_bytes().to_vec();
    welcomed.extend_from_slice(commit.welcome.as_ref().unwrap());
    let ready = bob.handle(frame(VoiceOpcode::MlsWelcome, bytes_json(&welcomed)));
    assert_eq!(ready[0]["d"]["transition_id"], 7);
    assert!(bob.failures.is_empty(), "{:?}", bob.failures);

    for voice in [&mut alice, &mut bob] {
        voice.handle(frame(
            VoiceOpcode::DavePrepareTransition,
            json!({"transition_id": 7, "protocol_version": 1}),
        ));
        voice.handle(frame(
            VoiceOpcode::DaveExecuteTransition,
            json!({"transition_id": 7}),
        ));
    }

    let DaveEngine::Live(alice_live) = &mut alice.dave else {
        panic!("live");
    };
    let encrypted = alice_live
        .encrypt_opus(b"epoch-media")
        .expect("alice can encrypt");
    let DaveEngine::Live(bob_live) = &mut bob.dave else {
        panic!("live");
    };
    let decrypted = bob_live
        .decrypt_opus(user_a, &encrypted)
        .expect("bob decrypts with the epoch key");
    assert_eq!(decrypted, b"epoch-media");
}
