//! RTP packetization for one H.264 NAL, following RFC 6184.
//!
//! A short NAL is one packet. A longer NAL is fragmented with FU-A. The
//! marker bit is set on the packet that finishes the access unit.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RtpPacket {
    pub sequence: u16,
    pub timestamp: u32,
    pub marker: bool,
    pub ssrc: u32,
    pub payload: Vec<u8>,
}

pub fn packetize(
    nal: &[u8],
    mtu: usize,
    mut sequence: u16,
    timestamp: u32,
    ssrc: u32,
) -> Result<Vec<RtpPacket>, String> {
    if nal.is_empty() {
        return Err("empty NAL".into());
    }
    if mtu < 2 {
        return Err("mtu is smaller than an FU-A header".into());
    }
    if nal.len() <= mtu {
        return Ok(vec![RtpPacket {
            sequence,
            timestamp,
            marker: true,
            ssrc,
            payload: nal.to_vec(),
        }]);
    }
    let nal_type = nal[0] & 0x1f;
    let indicator = (nal[0] & 0xe0) | 28;
    let body = &nal[1..];
    let chunk = mtu - 2;
    let mut packets = Vec::new();
    let mut offset = 0;
    while offset < body.len() {
        let end = (offset + chunk).min(body.len());
        let start = offset == 0;
        let finish = end == body.len();
        let mut header = 0u8;
        if start {
            header |= 0x80;
        }
        if finish {
            header |= 0x40;
        }
        header |= nal_type;
        let mut payload = vec![indicator, header];
        payload.extend_from_slice(&body[offset..end]);
        packets.push(RtpPacket {
            sequence,
            timestamp,
            marker: finish,
            ssrc,
            payload,
        });
        sequence = sequence.wrapping_add(1);
        offset = end;
    }
    Ok(packets)
}

pub fn depacketize(packets: &[RtpPacket]) -> Result<Vec<u8>, String> {
    if packets.is_empty() {
        return Err("no packets".into());
    }
    if packets.len() == 1
        && packets[0]
            .payload
            .first()
            .is_some_and(|byte| byte & 0x1f != 28)
    {
        return Ok(packets[0].payload.clone());
    }
    let mut nal = Vec::new();
    for (index, packet) in packets.iter().enumerate() {
        if packet.payload.len() < 2 || packet.payload[0] & 0x1f != 28 {
            return Err("expected an FU-A packet".into());
        }
        let header = packet.payload[1];
        let start = header & 0x80 != 0;
        let end = header & 0x40 != 0;
        if index == 0 && !start {
            return Err("FU-A does not start".into());
        }
        if index == 0 {
            let nal_header = (packet.payload[0] & 0xe0) | (header & 0x1f);
            nal.push(nal_header);
        }
        nal.extend_from_slice(&packet.payload[2..]);
        if end && index + 1 != packets.len() {
            return Err("FU-A ended early".into());
        }
    }
    Ok(nal)
}

pub fn write_rtp(packet: &RtpPacket) -> Vec<u8> {
    let mut bytes = vec![0x80, if packet.marker { 0x80 | 96 } else { 96 }];
    bytes.extend_from_slice(&packet.sequence.to_be_bytes());
    bytes.extend_from_slice(&packet.timestamp.to_be_bytes());
    bytes.extend_from_slice(&packet.ssrc.to_be_bytes());
    bytes.extend_from_slice(&packet.payload);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_nal_is_one_packet_and_a_long_nal_roundtrips() {
        let short = packetize(&[5, 1, 2, 3], 1200, 7, 90_000, 42).unwrap();
        assert_eq!(short.len(), 1);
        assert!(short[0].marker);
        assert_eq!(depacketize(&short).unwrap(), vec![5, 1, 2, 3]);

        let nal: Vec<u8> = (0..50).map(|index| (index % 29) as u8).collect();
        let mut owned = vec![5];
        owned.extend_from_slice(&nal);
        let packets = packetize(&owned, 10, 1, 100, 9).unwrap();
        assert!(packets.len() > 1);
        assert!(packets.last().unwrap().marker);
        assert!(!packets[0].marker);
        assert_eq!(depacketize(&packets).unwrap(), owned);
        assert_eq!(write_rtp(&packets[0]).len(), 12 + packets[0].payload.len());
    }
}
