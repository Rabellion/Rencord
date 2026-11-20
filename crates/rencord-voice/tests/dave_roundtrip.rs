//! Two-member DAVE group: encrypt an Opus packet with one session and decrypt
//! it with the other. The external sender stands in for Discord's voice
//! gateway, using the public OpenMLS types davey already depends on.

use std::num::NonZeroU16;

use davey::{DaveSession, ProposalsOperationType};
use openmls::prelude::{tls_codec::Serialize, *};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;

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
