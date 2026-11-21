//! Local REST and gateway stand-in for `--demo --stub`.
//!
//! It binds to 127.0.0.1 only. Nothing here dials Discord.

use std::sync::OnceLock;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;

/// Which signed-in moment the local servers should hold still for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StubScene {
    /// Named guilds, channels, and a short history.
    Guilds,
    /// Fifty messages, then a slow older page so the loading row stays up.
    Scroll,
    /// History responds 500 so the retry row stays up.
    Retry,
    /// Categories, join lines, several days, and a direct message.
    Layout,
}

static SCENE: OnceLock<StubScene> = OnceLock::new();
static HTTP_ORIGIN: OnceLock<String> = OnceLock::new();

pub const TOKEN: &str = "stub-local-token";

pub fn current() -> Option<StubScene> {
    SCENE.get().copied()
}

static OPEN_HOME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn request_home() {
    OPEN_HOME.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn take_home() -> bool {
    OPEN_HOME.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// Binds the HTTP API and the gateway, then returns their base URLs.
pub async fn serve(scene: StubScene) -> (String, String) {
    let _ = SCENE.set(scene);
    let http = TcpListener::bind("127.0.0.1:0").await.expect("stub http");
    let gateway = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("stub gateway");
    let http_addr = http.local_addr().expect("http addr");
    let _ = HTTP_ORIGIN.set(format!("http://{http_addr}"));
    let gateway_addr = gateway.local_addr().expect("gateway addr");
    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = http.accept().await {
                tokio::spawn(handle_http(stream, scene));
            }
        }
    });
    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = gateway.accept().await {
                tokio::spawn(handle_gateway(stream));
            }
        }
    });
    (
        format!("http://{http_addr}/api/v10"),
        format!("ws://{gateway_addr}/?v=10&encoding=json"),
    )
}

async fn handle_http(mut stream: TcpStream, scene: StubScene) {
    let mut buf = vec![0u8; 4096];
    let mut got = Vec::new();
    loop {
        let Ok(n) = stream.read(&mut buf).await else {
            return;
        };
        if n == 0 {
            break;
        }
        got.extend_from_slice(&buf[..n]);
        if got.windows(4).any(|window| window == b"\r\n\r\n") || got.len() > 8192 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&got);
    let Some(line) = text.lines().next() else {
        return;
    };
    let path = line.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path);
    if path.contains("/avatars/")
        || path.contains("/icons/")
        || path.contains("/banners/")
        || path.contains("/emojis/")
        || path.contains("/attachments/")
    {
        let bytes = tinted_png(path);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        );
        let _ = stream.write_all(header.as_bytes()).await;
        let _ = stream.write_all(&bytes).await;
        return;
    }
    if path.contains("before=") {
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
    let (status, body) = respond(scene, path);
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
}

fn respond(scene: StubScene, path: &str) -> (&'static str, String) {
    if scene == StubScene::Layout {
        if path.contains("/channels/") && path.contains("/messages") {
            return ("200 OK", layout_messages());
        }
        if path.contains("/users/@me/channels") {
            return (
                "200 OK",
                r#"[{"id":"200","type":1,"recipients":[{"id":"2","username":"munch","global_name":"Munch","avatar":"u2"}]}]"#
                    .into(),
            );
        }
        if path.contains("/guilds/") && path.contains("/channels") {
            return (
                "200 OK",
                r#"[{"id":"1","type":4,"name":"Start here","guild_id":"10","position":0},{"id":"100","type":0,"name":"welcome","guild_id":"10","parent_id":"1","position":0},{"id":"101","type":0,"name":"general","guild_id":"10","parent_id":"1","position":1},{"id":"2","type":4,"name":"Voice","guild_id":"10","position":1},{"id":"110","type":2,"name":"Lounge","guild_id":"10","parent_id":"2","position":0}]"#
                    .into(),
            );
        }
        if path.contains("/users/@me/guilds") {
            return (
                "200 OK",
                r#"[{"id":"10","name":"Analytical Engines","icon":"g10","banner":"b10"},{"id":"11","name":"Difference Club","icon":"g11","banner":null}]"#
                    .into(),
            );
        }
        if path.contains("/users/@me") {
            return (
                "200 OK",
                r#"{"id":"1","username":"agentprivate","global_name":"Agent Private","bot":false}"#
                    .into(),
            );
        }
    }
    if path.contains("/channels/") && path.contains("/messages") {
        if scene == StubScene::Retry && !path.contains("before=") {
            return (
                "500 Internal Server Error",
                r#"{"message":"stub history failed"}"#.into(),
            );
        }
        if path.contains("before=") {
            return ("200 OK", messages(100, 2900, 10));
        }
        let count = if scene == StubScene::Scroll { 50 } else { 3 };
        return ("200 OK", messages(100, 3000 - count + 1, count));
    }
    if path.contains("/users/@me/guilds") {
        return (
            "200 OK",
            r#"[{"id":"10","name":"Analytical Engines","icon":"abc"},{"id":"11","name":"Difference Club","icon":null}]"#
                .into(),
        );
    }
    if path.contains("/users/@me/channels") {
        return ("200 OK", "[]".into());
    }
    if path.contains("/guilds/") && path.contains("/channels") {
        return (
            "200 OK",
            r#"[{"id":"100","type":0,"name":"general","guild_id":"10","position":0},{"id":"110","type":0,"name":"notes","guild_id":"10","position":1}]"#
                .into(),
        );
    }
    if path.contains("/users/@me") {
        return (
            "200 OK",
            r#"{"id":"1","username":"ada","global_name":"Ada","bot":false}"#.into(),
        );
    }
    ("404 Not Found", r#"{"message":"stub"}"#.into())
}

fn tinted_png(seed: &str) -> Vec<u8> {
    let mut hash = 0u32;
    for byte in seed.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(byte));
    }
    // Bright, distinct fills so a stub screenshot cannot collapse to one dark disc.
    let palette = [
        [88u8, 101, 242],
        [87, 242, 135],
        [254, 231, 92],
        [237, 66, 69],
        [235, 69, 158],
        [59, 165, 93],
        [255, 163, 72],
        [79, 184, 255],
    ];
    let color = palette[(hash as usize) % palette.len()];
    let mark = ((hash >> 8) % 3) as u8;
    let image = image::RgbaImage::from_fn(64, 64, |x, y| {
        let ink = match mark {
            0 => x > 18 && x < 46 && y > 18 && y < 46,
            1 => (x as i32 - 32).abs() + (y as i32 - 32).abs() < 16,
            _ => (x as i32 - 32).pow(2) + (y as i32 - 32).pow(2) < 14 * 14,
        };
        if ink {
            image::Rgba([255, 255, 255, 255])
        } else {
            image::Rgba([color[0], color[1], color[2], 255])
        }
    });
    let mut bytes = Vec::new();
    let _ = image::DynamicImage::ImageRgba8(image).write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    );
    bytes
}

fn layout_messages() -> String {
    let origin = HTTP_ORIGIN
        .get()
        .cloned()
        .unwrap_or_else(|| "http://127.0.0.1".to_owned());
    format!(
        r#"[
        {{"id":"6","channel_id":"100","type":0,"content":"Still here, <@1>. <:wave:42>","edited_timestamp":"2026-11-21T18:07:00Z","timestamp":"2026-11-21T18:06:00Z","mentions":[{{"id":"1","username":"agentprivate","global_name":"Agent Private"}}],"attachments":[{{"url":"{origin}/attachments/note.png","proxy_url":"{origin}/attachments/note.png","content_type":"image/png"}}],"author":{{"id":"2","username":"munch","global_name":"Munch","avatar":"u2"}}}},
        {{"id":"5","channel_id":"100","type":0,"content":"Hello from the lobby.","timestamp":"2026-11-21T18:04:00Z","author":{{"id":"2","username":"munch","global_name":"Munch","avatar":"u2"}}}},
        {{"id":"4","channel_id":"100","type":0,"content":"The notes are in <#101>.","timestamp":"2026-11-20T15:00:00Z","mention_channels":[{{"id":"101","name":"general"}}],"author":{{"id":"1","username":"agentprivate","global_name":"Agent Private","avatar":"u1"}}}},
        {{"id":"3","channel_id":"100","type":7,"content":"","timestamp":"2026-11-19T12:00:00Z","author":{{"id":"3","username":"nosh","global_name":"Nosh","avatar":"u3"}}}},
        {{"id":"2","channel_id":"100","type":7,"content":"","timestamp":"2026-11-18T12:00:00Z","author":{{"id":"2","username":"munch","global_name":"Munch","avatar":"u2"}}}}
    ]"#
    )
}

fn messages(channel: u64, first: u64, count: u64) -> String {
    let mut list = Vec::new();
    for offset in 0..count {
        let id = first + offset;
        list.push(format!(
            r#"{{"id":"{id}","channel_id":"{channel}","content":"Note {id}","timestamp":"2026-11-20T00:00:00Z","author":{{"id":"1","username":"ada","global_name":"Ada"}}}}"#
        ));
    }
    // Discord returns newest first.
    list.reverse();
    format!("[{}]", list.join(","))
}

async fn handle_gateway(stream: TcpStream) {
    let Ok(mut socket) = accept_async(stream).await else {
        return;
    };
    let hello = r#"{"op":10,"d":{"heartbeat_interval":60000},"s":null,"t":null}"#;
    if socket
        .send(tokio_tungstenite::tungstenite::Message::Text(hello.into()))
        .await
        .is_err()
    {
        return;
    }
    // Identify (or a heartbeat) arrives before READY.
    let _ = socket.next().await;
    let ready = r#"{"op":0,"t":"READY","s":1,"d":{"session_id":"stub","user":{"id":"1","username":"ada","global_name":"Ada"},"guilds":[{"id":"10","name":"Analytical Engines","icon":"abc","channels":[{"id":"100","type":0,"name":"general","guild_id":"10"}]},{"id":"11","name":"Difference Club","icon":null,"channels":[{"id":"111","type":0,"name":"lounge"}]}],"user_settings":{"theme":"dark"}}}"#;
    if socket
        .send(tokio_tungstenite::tungstenite::Message::Text(ready.into()))
        .await
        .is_err()
    {
        return;
    }
    while let Some(Ok(message)) = socket.next().await {
        let text = message.into_text().unwrap_or_default();
        if text.contains("\"op\":14") {
            let update = r#"{"op":0,"t":"GUILD_MEMBER_LIST_UPDATE","s":2,"d":{"guild_id":"10","id":"100","ops":[{"op":"SYNC","range":[0,99],"items":[{"group":{"id":"Online","count":1}},{"member":{"user":{"id":"2","global_name":"Munch","avatar":"u2"},"presence":{"status":"online","activities":[{"type":0,"name":"THE FINALS"}]}}},{"group":{"id":"Ranked Players","count":2}},{"member":{"user":{"id":"3","global_name":"Nosh","avatar":"u3"},"presence":{"status":"online","activities":[{"name":"In voice"}]}}},{"member":{"user":{"id":"4","global_name":"Boyka","avatar":"u4"},"presence":{"status":"idle","activities":[{"type":0,"name":"ROBLOX"}]}}}]}]}}"#;
            if socket
                .send(tokio_tungstenite::tungstenite::Message::Text(update.into()))
                .await
                .is_err()
            {
                break;
            }
        }
        if text.contains("\"op\":1") {
            let ack = r#"{"op":11,"d":null}"#;
            if socket
                .send(tokio_tungstenite::tungstenite::Message::Text(ack.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_history_fails_and_guilds_keep_their_names() {
        let (status, _) = respond(StubScene::Retry, "/api/v10/channels/100/messages?limit=50");
        assert!(status.starts_with("500"), "{status}");
        let (_, guilds) = respond(StubScene::Guilds, "/api/v10/users/@me/guilds");
        assert!(guilds.contains("Analytical Engines"), "{guilds}");
        let (_, page) = respond(
            StubScene::Scroll,
            "/api/v10/channels/100/messages?before=2951&limit=50",
        );
        assert!(page.contains("\"2900\""), "{page}");
        assert!(!page.contains("\"3000\""), "{page}");
    }
}
