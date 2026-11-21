//! What the message list shows, decided without any UI toolkit.
//!
//! Everything here takes data from other users, so it is written to cope with
//! anything: a missing name, a hash that is not a hash, a reply to a message
//! that no longer exists.

use rencord_model::{Message, Reply, Snowflake, User};

/// A hash or name that is safe to put in a URL path: letters, digits, `_`.
fn url_safe(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Discord only serves avatar sizes that are powers of two.
fn image_size(wanted: u32) -> u32 {
    wanted.clamp(16, 256).next_power_of_two()
}

/// The user's avatar, or the default one Discord assigns when there is none.
pub fn avatar_url(user: &User, wanted: u32) -> String {
    match user.avatar.as_deref() {
        Some(hash) if url_safe(hash) => format!(
            "https://cdn.discordapp.com/avatars/{}/{hash}.png?size={}",
            user.id,
            image_size(wanted)
        ),
        _ => format!(
            "https://cdn.discordapp.com/embed/avatars/{}.png",
            (user.id.get() >> 22) % 6
        ),
    }
}

/// A server's icon, if it has one.
pub fn guild_icon_url(guild: Snowflake, hash: Option<&str>, wanted: u32) -> Option<String> {
    let hash = hash.filter(|hash| url_safe(hash))?;
    Some(format!(
        "https://cdn.discordapp.com/icons/{guild}/{hash}.png?size={}",
        image_size(wanted)
    ))
}

/// The line shown for a message that has no text of its own, such as a join.
pub fn system_text(message: &Message) -> String {
    let name = message.author.display_name();
    match message.kind {
        1 => format!("{name} added someone to the group."),
        2 => format!("{name} removed someone from the group."),
        3 => format!("{name} started a call."),
        4 => format!("{name} changed the channel name: {}", message.content),
        5 => format!("{name} changed the channel icon."),
        6 => format!("{name} pinned a message to this channel."),
        7 => format!("{name} joined the server."),
        8..=11 => format!("{name} boosted the server."),
        18 => format!("{name} started a thread: {}", message.content),
        _ => format!("{name} sent a system message."),
    }
}

/// The one-line "replying to" header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyPreview {
    /// Absent when the original message was deleted.
    pub who: Option<String>,
    pub text: String,
}

pub fn reply_preview(reply: &Reply) -> ReplyPreview {
    let who = reply
        .author
        .as_ref()
        .map(|author| author.display_name().to_owned());
    let line = reply
        .content
        .as_deref()
        // Skip blank lines and the fences around a code block.
        .and_then(|content| {
            content
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with("```"))
        })
        .unwrap_or("");
    let text = if reply.author.is_none() && reply.content.is_none() {
        "Original message was deleted".to_owned()
    } else if line.is_empty() {
        "Click to see attachment".to_owned()
    } else if line.chars().count() > 90 {
        let cut: String = line.chars().take(90).collect();
        format!("{}\u{2026}", cut.trim_end())
    } else {
        line.to_owned()
    };
    ReplyPreview { who, text }
}

/// Turns Discord's `<#channel>` and `<:emoji:id>` markup into text. Mentions of
/// people are left for the markdown parser, which keeps their ids.
pub fn clean_content(content: &str, channel_name: impl Fn(Snowflake) -> Option<String>) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            return out;
        };
        match markup_text(&tail[1..end], &channel_name) {
            Some(text) => {
                out.push_str(&text);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn markup_text(token: &str, channel_name: &impl Fn(Snowflake) -> Option<String>) -> Option<String> {
    if let Some(id) = token.strip_prefix('#') {
        let id: u64 = id.parse().ok()?;
        let name = channel_name(Snowflake::new(id)).unwrap_or_else(|| "unknown-channel".into());
        return Some(format!("#{name}"));
    }
    let custom = token
        .strip_prefix("a:")
        .or_else(|| token.strip_prefix(':'))?;
    let (name, id) = custom.split_once(':')?;
    (url_safe(name) && id.parse::<u64>().is_ok()).then(|| format!(":{name}:"))
}

/// How a reaction's emoji is written. Custom emoji arrive as a bare name.
pub fn emoji_label(name: &str) -> String {
    let custom = name.len() > 1
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if custom {
        format!(":{name}:")
    } else {
        name.to_owned()
    }
}

/// Scales a picture down to fit a box, keeping its shape. It is never enlarged.
pub fn fit_size(width: u32, height: u32, max_width: f32, max_height: f32) -> (f32, f32) {
    if width == 0 || height == 0 {
        return (max_width, max_height.min(max_width * 0.5625));
    }
    let (width, height) = (width as f32, height as f32);
    let scale = (max_width / width).min(max_height / height).min(1.0);
    ((width * scale).max(1.0), (height * scale).max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u64, avatar: Option<&str>) -> User {
        User {
            id: Snowflake::new(id),
            username: "ada".into(),
            global_name: Some("Ada".into()),
            avatar: avatar.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn avatars_use_the_hash_or_discords_default() {
        assert_eq!(
            avatar_url(&user(42, Some("a_1b2c3")), 40),
            "https://cdn.discordapp.com/avatars/42/a_1b2c3.png?size=64"
        );
        assert_eq!(
            avatar_url(&user(42, Some("a_1b2c3")), 100),
            "https://cdn.discordapp.com/avatars/42/a_1b2c3.png?size=128"
        );
        // 4194304 >> 22 is 1, so the second default avatar.
        assert_eq!(
            avatar_url(&user(4_194_304, None), 40),
            "https://cdn.discordapp.com/embed/avatars/1.png"
        );
    }

    #[test]
    fn a_hash_that_is_not_url_safe_is_not_used() {
        for bad in ["../../x", "a b", "x?y=1", "x/y", "", "x#y"] {
            let url = avatar_url(&user(7, Some(bad)), 64);
            assert!(url.contains("/embed/avatars/"), "{bad}: {url}");
        }
        assert_eq!(guild_icon_url(Snowflake::new(1), Some("../x"), 64), None);
        assert_eq!(guild_icon_url(Snowflake::new(1), None, 64), None);
        assert_eq!(
            guild_icon_url(Snowflake::new(1), Some("abc"), 48).unwrap(),
            "https://cdn.discordapp.com/icons/1/abc.png?size=64"
        );
    }

    #[test]
    fn system_messages_say_what_happened() {
        let mut message = Message {
            author: user(1, None),
            ..Default::default()
        };
        message.kind = 7;
        assert_eq!(system_text(&message), "Ada joined the server.");
        message.kind = 3;
        assert_eq!(system_text(&message), "Ada started a call.");
        message.kind = 6;
        assert!(system_text(&message).contains("pinned a message"));
        message.kind = 18;
        message.content = "Plans".into();
        assert_eq!(system_text(&message), "Ada started a thread: Plans");
        message.kind = 200;
        assert_eq!(system_text(&message), "Ada sent a system message.");
    }

    #[test]
    fn a_reply_names_its_target_or_says_it_was_deleted() {
        let reply = Reply {
            message_id: Snowflake::new(5),
            author: Some(user(1, None)),
            content: Some("\n  first line  \nsecond".into()),
        };
        assert_eq!(
            reply_preview(&reply),
            ReplyPreview {
                who: Some("Ada".into()),
                text: "first line".into()
            }
        );
        let long = Reply {
            content: Some("x".repeat(300)),
            ..reply.clone()
        };
        assert_eq!(reply_preview(&long).text.chars().count(), 91);
        let attachment_only = Reply {
            content: Some(String::new()),
            ..reply.clone()
        };
        assert_eq!(
            reply_preview(&attachment_only).text,
            "Click to see attachment"
        );
        let code = Reply {
            content: Some("```rust\nfn main() {\n}\n```".into()),
            ..reply.clone()
        };
        assert_eq!(reply_preview(&code).text, "fn main() {");
        let gone = Reply {
            message_id: Snowflake::new(5),
            author: None,
            content: None,
        };
        let preview = reply_preview(&gone);
        assert_eq!(preview.who, None);
        assert_eq!(preview.text, "Original message was deleted");
    }

    #[test]
    fn channel_and_emoji_markup_becomes_text() {
        let name = |id: Snowflake| (id.get() == 7).then(|| "general".to_owned());
        assert_eq!(
            clean_content("go to <#7> or <#8>, thanks <:pepe:123> <a:dance:456>", name),
            "go to #general or #unknown-channel, thanks :pepe: :dance:"
        );
        // Mentions of people are left for the markdown parser.
        assert_eq!(
            clean_content("hi <@42> and <@!43>", name),
            "hi <@42> and <@!43>"
        );
        // Not markup, or broken markup: kept as it was.
        assert_eq!(clean_content("a < b > c", name), "a < b > c");
        assert_eq!(
            clean_content("<:bad name:1> <:x:notanumber> <#abc>", name),
            "<:bad name:1> <:x:notanumber> <#abc>"
        );
        assert_eq!(clean_content("unclosed <#7", name), "unclosed <#7");
        assert_eq!(clean_content("", name), "");
    }

    #[test]
    fn reactions_name_custom_emoji_and_leave_real_ones() {
        assert_eq!(emoji_label("pepe"), ":pepe:");
        assert_eq!(emoji_label("\u{1F44D}"), "\u{1F44D}");
        assert_eq!(emoji_label("a"), "a");
    }

    #[test]
    fn pictures_fit_their_box_without_being_enlarged() {
        assert_eq!(fit_size(800, 400, 400.0, 300.0), (400.0, 200.0));
        assert_eq!(fit_size(400, 800, 400.0, 300.0), (150.0, 300.0));
        assert_eq!(fit_size(100, 50, 400.0, 300.0), (100.0, 50.0));
        let (width, height) = fit_size(0, 0, 400.0, 300.0);
        assert!(width > 0.0 && height > 0.0);
    }
}
