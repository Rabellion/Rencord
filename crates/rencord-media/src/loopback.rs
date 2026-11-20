//! Local path from a captured frame back to a viewer frame.
//!
//! Worker frames are encoded, packetized, wrapped for DAVE video, unwrapped
//! by a mock receiver, depacketized, and decoded. No Discord socket is used.

use crate::encode::{decode_software, AccessUnit};
use crate::frame::Frame;
use crate::golive::GoLive;
use crate::rtp::{depacketize, RtpPacket};
use rencord_voice::dave::{DaveEngine, DaveOps, ScriptedDave};

pub fn loopback_frame(frame: &Frame) -> Result<Frame, String> {
    let mut live = GoLive::new();
    live.preset.width = frame.width;
    live.preset.height = frame.height;
    live.preset.fps = 30;
    let _ = live.create(None, None);
    let mut sender = DaveEngine::Scripted(ScriptedDave {
        ready: true,
        ..ScriptedDave::default()
    });
    let packets = live.send_frame(frame, Some(&mut sender))?;
    let mut receiver = DaveEngine::Scripted(ScriptedDave {
        ready: true,
        ..ScriptedDave::default()
    });
    let mut plain = Vec::with_capacity(packets.len());
    for packet in packets {
        let payload = receiver.decrypt_video(1, &packet.payload)?;
        plain.push(RtpPacket { payload, ..packet });
    }
    let unit = reassemble(&plain)?;
    decode_software(&unit)
}

fn reassemble(packets: &[RtpPacket]) -> Result<AccessUnit, String> {
    let mut nal_units = Vec::new();
    let mut index = 0;
    while index < packets.len() {
        let fragmented = packets[index]
            .payload
            .first()
            .is_some_and(|byte| byte & 0x1f == 28);
        if !fragmented {
            nal_units.push(packets[index].payload.clone());
            index += 1;
            continue;
        }
        let mut group_end = index + 1;
        while group_end < packets.len()
            && packets[group_end]
                .payload
                .first()
                .is_some_and(|byte| byte & 0x1f == 28)
            && packets[group_end].sequence == packets[group_end - 1].sequence.wrapping_add(1)
        {
            group_end += 1;
        }
        nal_units.push(depacketize(&packets[index..group_end])?);
        index = group_end;
    }
    Ok(AccessUnit { nal_units })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_survives_encode_rtp_dave_and_decode() {
        let mut frame = Frame::new(8, 4, 3);
        for pixel in frame.bgra.chunks_mut(4) {
            pixel[0] = 255;
            pixel[1] = 255;
            pixel[2] = 255;
            pixel[3] = 255;
        }
        let viewed = loopback_frame(&frame).unwrap();
        assert_eq!(viewed.width, 8);
        assert_eq!(viewed.height, 4);
        assert!(viewed.bgra[0] > 200);
        assert_eq!(viewed.bgra[3], 255);
    }
}
