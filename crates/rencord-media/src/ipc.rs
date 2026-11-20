//! Length-prefixed JSON between the desktop process and `rencord-stream`.
//!
//! The worker owns capture. If it stalls or exits, the desktop keeps its
//! voice connection and reads `Failed` instead of blocking the UI thread.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClientMessage {
    ListSources,
    Start {
        source_id: String,
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
    },
    Stop,
    Stats,
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WorkerMessage {
    Sources {
        sources: Vec<crate::capture::CaptureSource>,
    },
    Started,
    Stopped,
    Stats {
        frames: u64,
        last_capture_us: u64,
        width: u32,
        height: u32,
        sequence: u64,
    },
    Failed {
        message: String,
    },
}

pub fn write_message(stream: &mut impl Write, message: &impl Serialize) -> std::io::Result<()> {
    let body = serde_json::to_vec(message)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    stream.write_all(&(body.len() as u32).to_le_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

pub fn read_message<T: for<'de> Deserialize<'de>>(stream: &mut impl Read) -> std::io::Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 8 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "ipc message is too large",
        ));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn messages_roundtrip() {
        let message = ClientMessage::Start {
            source_id: "synthetic:window".into(),
            width: 1280,
            height: 720,
            fps: 30,
            bitrate_kbps: 2_500,
        };
        let mut buffer = Vec::new();
        write_message(&mut buffer, &message).unwrap();
        let read: ClientMessage = read_message(&mut Cursor::new(buffer)).unwrap();
        assert_eq!(read, message);
    }
}
