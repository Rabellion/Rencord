//! RTP helpers and Discord `aead_xchacha20_poly1305_rtpsize`.
//!
//! Layout, matching the published voice encryption mode: the RTP header is
//! additional authenticated data, the Opus payload is ciphertext, a 16-byte
//! tag follows, and a 4-byte big-endian nonce suffix is appended. The header is
//! 12 bytes, plus 4 bytes per CSRC, plus the 4-byte extension header when the X
//! bit is set; the extension body is encrypted. The XChaCha20 nonce is the
//! suffix followed by twenty zero bytes.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

pub const TRANSPORT_MODE: &str = "aead_xchacha20_poly1305_rtpsize";
pub const RTP_HEADER_LEN: usize = 12;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("secret key must be 32 bytes, got {0}")]
    KeyLength(usize),
    #[error("packet is shorter than an encrypted RTP frame")]
    Truncated,
    #[error("authentication failed")]
    Auth,
}

pub struct TransportCipher {
    key: [u8; 32],
    counter: u32,
}

impl TransportCipher {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key, counter: 1 }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let key: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CryptoError::KeyLength(bytes.len()))?;
        Ok(Self::new(key))
    }

    pub fn seal(&mut self, seq: u16, timestamp: u32, ssrc: u32, opus: &[u8]) -> Vec<u8> {
        let header = rtp_header(seq, timestamp, ssrc);
        let counter = self.counter;
        self.counter = self.counter.wrapping_add(1);
        seal_with_counter(&self.key, counter, &header, opus)
    }

    pub fn open(&self, packet: &[u8]) -> Result<OpenedPacket, CryptoError> {
        open_packet(&self.key, packet)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct OpenedPacket {
    pub sequence: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub opus: Vec<u8>,
}

pub fn rtp_header(seq: u16, timestamp: u32, ssrc: u32) -> [u8; RTP_HEADER_LEN] {
    let mut header = [0u8; RTP_HEADER_LEN];
    header[0] = 0x80;
    header[1] = 0x78;
    header[2..4].copy_from_slice(&seq.to_be_bytes());
    header[4..8].copy_from_slice(&timestamp.to_be_bytes());
    header[8..12].copy_from_slice(&ssrc.to_be_bytes());
    header
}

fn seal_with_counter(key: &[u8; 32], counter: u32, header: &[u8], opus: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let suffix = counter.to_be_bytes();
    let mut nonce = XNonce::default();
    nonce[..4].copy_from_slice(&suffix);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: opus,
                aad: header,
            },
        )
        .expect("xchacha20poly1305 seal");
    let mut packet = Vec::with_capacity(header.len() + ciphertext.len() + 4);
    packet.extend_from_slice(header);
    packet.extend_from_slice(&ciphertext);
    packet.extend_from_slice(&suffix);
    packet
}

fn open_packet(key: &[u8; 32], packet: &[u8]) -> Result<OpenedPacket, CryptoError> {
    if packet.len() < RTP_HEADER_LEN + 16 + 4 {
        return Err(CryptoError::Truncated);
    }
    // Everything up to the extension body is authenticated header.
    let csrcs = usize::from(packet[0] & 0x0F);
    let has_extension = packet[0] & 0x10 != 0;
    let header_len = RTP_HEADER_LEN + 4 * csrcs + if has_extension { 4 } else { 0 };
    if packet.len() < header_len + 16 + 4 {
        return Err(CryptoError::Truncated);
    }
    let header = &packet[..header_len];
    let suffix = &packet[packet.len() - 4..];
    let ciphertext = &packet[header_len..packet.len() - 4];
    let mut nonce = XNonce::default();
    nonce[..4].copy_from_slice(suffix);
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut opus = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad: header,
            },
        )
        .map_err(|_| CryptoError::Auth)?;
    if has_extension {
        // The length is in 32-bit words and counts the body only.
        let words = u16::from_be_bytes([header[header_len - 2], header[header_len - 1]]);
        let body = usize::from(words) * 4;
        if opus.len() < body {
            return Err(CryptoError::Truncated);
        }
        opus.drain(..body);
    }
    let sequence = u16::from_be_bytes([header[2], header[3]]);
    let timestamp = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    let ssrc = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
    Ok(OpenedPacket {
        sequence,
        timestamp,
        ssrc,
        opus,
    })
}

/// Discord UDP IP discovery request. The response parser is tested locally.
pub fn discovery_request(ssrc: u32) -> [u8; 74] {
    let mut packet = [0u8; 74];
    packet[1] = 1;
    packet[3] = 70;
    packet[4..8].copy_from_slice(&ssrc.to_be_bytes());
    packet
}

/// A discovery reply is type `0x0002`, length field 70, and the SSRC we sent.
/// Anything else is a stray packet, not a failed discovery.
pub fn valid_discovery_response(packet: &[u8], ssrc: u32) -> Option<(String, u16)> {
    if packet.len() < 74 {
        return None;
    }
    let kind = u16::from_be_bytes([packet[0], packet[1]]);
    let length = u16::from_be_bytes([packet[2], packet[3]]);
    let echoed = u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]);
    if kind != 0x0002 || length != 70 || echoed != ssrc {
        return None;
    }
    parse_discovery_response(packet)
}

pub fn parse_discovery_response(packet: &[u8]) -> Option<(String, u16)> {
    if packet.len() < 74 {
        return None;
    }
    let ip_bytes = &packet[8..72];
    let end = ip_bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(ip_bytes.len());
    let ip = std::str::from_utf8(&ip_bytes[..end]).ok()?.to_owned();
    if ip.is_empty() {
        return None;
    }
    let port = u16::from_be_bytes([packet[72], packet[73]]);
    Some((ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_roundtrip_rejects_tampering() {
        let mut cipher = TransportCipher::new([7u8; 32]);
        let packet = cipher.seal(4, 960, 55, b"opus-frame");
        let opened = cipher.open(&packet).unwrap();
        assert_eq!(opened.sequence, 4);
        assert_eq!(opened.timestamp, 960);
        assert_eq!(opened.ssrc, 55);
        assert_eq!(opened.opus, b"opus-frame");
        let mut tampered = packet.clone();
        let last = tampered.len() - 5;
        tampered[last] ^= 0x01;
        assert_eq!(cipher.open(&tampered), Err(CryptoError::Auth));
    }

    /// Seals `plaintext` after an RTP header with `csrcs` CSRC entries and,
    /// when `ext_words` is `Some`, an extension header. The extension body
    /// is the first bytes of the plaintext, as Discord sends it.
    fn packet_with_extension(csrcs: u8, ext_words: Option<u16>, plaintext: &[u8]) -> Vec<u8> {
        let mut header = rtp_header(9, 1920, 55).to_vec();
        header[0] = 0x80 | csrcs | if ext_words.is_some() { 0x10 } else { 0 };
        header.extend(std::iter::repeat_n(0xCC, 4 * usize::from(csrcs)));
        if let Some(words) = ext_words {
            header.extend_from_slice(&[0xBE, 0xDE]);
            header.extend_from_slice(&words.to_be_bytes());
        }
        seal_with_counter(&[7u8; 32], 3, &header, plaintext)
    }

    #[test]
    fn extension_header_is_authenticated_and_its_body_is_stripped() {
        let cipher = TransportCipher::new([7u8; 32]);
        // First byte 0x90: version 2, X bit set. Body is one word.
        let mut plaintext = vec![0x11, 0x22, 0x33, 0x44];
        plaintext.extend_from_slice(b"opus-frame");
        let packet = packet_with_extension(0, Some(1), &plaintext);
        assert_eq!(packet[0], 0x90);
        let opened = cipher.open(&packet).unwrap();
        assert_eq!(opened.opus, b"opus-frame");
        assert_eq!(
            (opened.sequence, opened.timestamp, opened.ssrc),
            (9, 1920, 55)
        );
    }

    #[test]
    fn csrc_entries_are_part_of_the_authenticated_header() {
        let cipher = TransportCipher::new([7u8; 32]);
        let packet = packet_with_extension(2, None, b"opus-frame");
        assert_eq!(packet[0], 0x82);
        assert_eq!(cipher.open(&packet).unwrap().opus, b"opus-frame");
        // CSRCs and an extension together.
        let mut plaintext = vec![0; 8];
        plaintext.extend_from_slice(b"opus-frame");
        let both = packet_with_extension(1, Some(2), &plaintext);
        assert_eq!(cipher.open(&both).unwrap().opus, b"opus-frame");
    }

    #[test]
    fn an_extension_longer_than_the_payload_is_rejected() {
        let cipher = TransportCipher::new([7u8; 32]);
        // Declares a 40-byte body but carries only 6 bytes of plaintext.
        let packet = packet_with_extension(0, Some(10), b"short!");
        assert_eq!(cipher.open(&packet), Err(CryptoError::Truncated));
    }

    #[test]
    fn discovery_response_parses_ip_and_port() {
        let mut packet = discovery_request(99);
        packet[1] = 2;
        let ip = b"203.0.113.10";
        packet[8..8 + ip.len()].copy_from_slice(ip);
        packet[72..74].copy_from_slice(&40000u16.to_be_bytes());
        let (address, port) = valid_discovery_response(&packet, 99).unwrap();
        assert_eq!(address, "203.0.113.10");
        assert_eq!(port, 40000);
        assert!(valid_discovery_response(&packet, 100).is_none());
        packet[1] = 1;
        assert!(valid_discovery_response(&packet, 99).is_none());
    }
}
