//! RSA-OAEP key used for one remote-auth attempt.
//!
//! Discord's remote-auth gateway encrypts with RSA-OAEP using SHA-256 for the
//! hash and for MGF1. The public key on the wire is the base64 SPKI encoding.
//! The fingerprint is the unpadded base64url SHA-256 of those SPKI bytes.

use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use rand::rngs::OsRng;
use rsa::pkcs8::EncodePublicKey;
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};

use crate::AuthError;

pub struct RemoteKey {
    private: RsaPrivateKey,
    spki: Vec<u8>,
}

impl RemoteKey {
    pub fn generate() -> Result<Self, AuthError> {
        let mut rng = OsRng;
        let private = RsaPrivateKey::new(&mut rng, 2048)
            .map_err(|_| AuthError::Message("Could not generate a login key.".into()))?;
        let public = RsaPublicKey::from(&private);
        let document = public
            .to_public_key_der()
            .map_err(|_| AuthError::Message("Could not encode the login key.".into()))?;
        Ok(Self {
            private,
            spki: document.as_bytes().to_vec(),
        })
    }

    pub fn encoded_public_key(&self) -> String {
        STANDARD.encode(&self.spki)
    }

    /// Base64url SHA-256 of the SPKI bytes, without padding.
    pub fn fingerprint(&self) -> String {
        fingerprint_of_spki(&self.spki)
    }

    pub fn spki(&self) -> &[u8] {
        &self.spki
    }

    pub fn decrypt_b64(&self, encoded: &str) -> Result<Vec<u8>, AuthError> {
        let cipher = decode_b64(encoded)?;
        self.private
            .decrypt(Oaep::new::<Sha256>(), &cipher)
            .map_err(|_| AuthError::Message("Could not decrypt a login payload.".into()))
    }
}

pub fn fingerprint_of_spki(spki: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(spki))
}

pub fn decode_b64(encoded: &str) -> Result<Vec<u8>, AuthError> {
    let trimmed = encoded.trim();
    STANDARD
        .decode(trimmed)
        .or_else(|_| URL_SAFE.decode(trimmed))
        .or_else(|_| URL_SAFE_NO_PAD.decode(trimmed))
        .map_err(|_| AuthError::Message("A login payload was not valid base64.".into()))
}

pub fn encode_nonce(nonce: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(nonce)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs8::DecodePublicKey;

    #[test]
    fn published_example_fingerprint_is_sha256_of_the_spki() {
        // Public example from the remote-auth gateway documentation. Not a secret.
        let encoded = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAo2PGAKj4v6r6sPJtgJe2eIDCM8uEHKpYCSDmp+pun9vqiqPt4pDToS1vGtwTwc5hKKqtIo+I/5veBpGWSD/veuB0xVb/JbkPn847Q+mXAb6c9vRMJVkA7l9GaZdN49U5bnGJi009aNBoy9cAcP/19H6TLpHmZ9RojnqGqlCUdyAiqceTDTzPqov4ST3GJSyKPydL3ZVpPf5P/PGyNfISuESKA2CxGCoBvB4H6/FH7cwSFelyqhwwHPZcyxBjF/3iXx+k1PdS01y0NoTRun4p76bE9rWnecIWONPFvCkby8Xs/OqQ8QcAoLkfVj5L29Ut1+Kmwwfg3nzc4glZa6RuTwIDAQAB";
        let spki = STANDARD.decode(encoded).unwrap();
        assert_eq!(
            fingerprint_of_spki(&spki),
            "UZ0-kOVzXDZTFVV5_QlpURSO2BQHrtkKWHNpIGoDI0k"
        );
    }

    #[test]
    fn oaep_sha256_roundtrip_and_nonce_encoding() {
        let key = RemoteKey::generate().unwrap();
        let public = RsaPublicKey::from_public_key_der(key.spki()).unwrap();
        let nonce = b"nonce-bytes-for-the-proof";
        let cipher = public
            .encrypt(&mut OsRng, Oaep::new::<Sha256>(), nonce)
            .unwrap();
        let recovered = key.decrypt_b64(&STANDARD.encode(cipher)).unwrap();
        assert_eq!(recovered, nonce);
        assert_eq!(encode_nonce(&recovered), URL_SAFE_NO_PAD.encode(nonce));
        assert!(!format!("{:?}", key.fingerprint()).contains("nonce-bytes"));
        let debug = format!("{:?}", AuthError::Message("plain".into()));
        assert!(!debug.contains("nonce-bytes"));
    }

    use crate::AuthError;
}
