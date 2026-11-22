//! What the message list shows, decided without any UI toolkit.
//!
//! Everything here takes data from other users, so it is written to cope with
//! anything: a missing name, a hash that is not a hash, a reply to a message
//! that no longer exists.

use std::collections::HashMap;

use rencord_model::{Embed, EmbedImage, Member, Message, Reply, Role, Snowflake, User};

/// A hash or name that is safe to put in a URL path: letters, digits, `_`.
fn url_safe(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// The colour of someone's name in a server: their highest role that has one.
/// A role with no colour is skipped, so it cannot hide a coloured role below it.
pub fn name_color(guild_roles: &[Role], member_roles: &[Snowflake]) -> Option<u32> {
    guild_roles
        .iter()
        .filter(|role| role.color & 0x00FF_FFFF != 0 && member_roles.contains(&role.id))
        .max_by_key(|role| (role.position, std::cmp::Reverse(role.id.get())))
        .map(|role| role.color & 0x00FF_FFFF)
}

/// The name drawn for an author and its colour, where the server has an opinion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorStyle {
    pub name: String,
    pub color: Option<u32>,
}

/// A nickname beats the account's display name, as in Discord. With no member
/// information the name is the display name and the colour is the default.
pub fn author_style(user: &User, member: Option<&Member>, guild_roles: &[Role]) -> AuthorStyle {
    AuthorStyle {
        name: member
            .and_then(Member::nickname)
            .unwrap_or_else(|| user.display_name())
            .to_owned(),
        color: member.and_then(|member| name_color(guild_roles, &member.roles)),
    }
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
    } else if reply.author.is_none() {
        "Original message".to_owned()
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
/// people are left for the markdown parser, which keeps their ids. Code, and a
/// backslash before the markup, are left as typed.
pub fn clean_content(content: &str, channel_name: impl Fn(Snowflake) -> Option<String>) -> String {
    let chars: Vec<char> = content.chars().collect();
    let mut out = String::with_capacity(content.len());
    let mut index = 0;
    let mut fenced = false;
    while index < chars.len() {
        if chars[index..].starts_with(&['`', '`', '`']) {
            fenced = !fenced;
            out.push_str("```");
            index += 3;
            continue;
        }
        if !fenced && chars[index] == '`' {
            out.push('`');
            index += 1;
            while index < chars.len() && chars[index] != '`' {
                out.push(chars[index]);
                index += 1;
            }
            if index < chars.len() {
                out.push('`');
                index += 1;
            }
            continue;
        }
        if !fenced && chars[index] == '\\' && index + 1 < chars.len() {
            out.push('\\');
            out.push(chars[index + 1]);
            index += 2;
            continue;
        }
        if !fenced && chars[index] == '<' {
            let tail: String = chars[index..].iter().collect();
            if let Some(end) = tail.find('>') {
                if let Some(text) = markup_text(&tail[1..end], &channel_name) {
                    out.push_str(&text);
                    index += end + 1;
                    continue;
                }
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// The picture an embed should show. An empty image does not hide the thumbnail.
pub fn embed_picture(embed: &Embed) -> Option<&EmbedImage> {
    embed
        .image
        .as_ref()
        .filter(|image| !image.best_url().is_empty())
        .or_else(|| {
            embed
                .thumbnail
                .as_ref()
                .filter(|image| !image.best_url().is_empty())
        })
}

/// Whether an embed has text, a picture, or a video the row can offer.
pub fn embed_has_body(embed: &Embed) -> bool {
    let has_text = embed.title.is_some() || embed.description.is_some();
    let video = embed.video.is_some() || matches!(embed.kind.as_deref(), Some("video" | "gifv"));
    has_text || embed_picture(embed).is_some() || video
}

/// One style per message, so two messages from one person can differ.
pub fn author_styles<'a>(
    rows: impl IntoIterator<Item = (&'a Message, Option<&'a Member>)>,
    roles: &'a [Role],
) -> HashMap<Snowflake, AuthorStyle> {
    rows.into_iter()
        .map(|(message, member)| (message.id, author_style(&message.author, member, roles)))
        .collect()
}

/// A mention's visible name. A resolved name is used as given, so a person
/// called "1" is not the demo user of that id.
pub fn mention_text(id: &str, names: &HashMap<String, String>) -> String {
    if let Some(name) = names.get(id) {
        return format!("@{name}");
    }
    match id {
        "1" => "@Ada".into(),
        "2" => "@Charles".into(),
        "3" => "@Lovelace".into(),
        other => format!("@{other}"),
    }
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

    fn role(id: u64, color: u32, position: i32) -> Role {
        Role {
            id: Snowflake::new(id),
            name: format!("r{id}"),
            color,
            position,
            hoist: false,
        }
    }

    #[test]
    fn a_name_takes_the_colour_of_the_highest_coloured_role() {
        let roles = [
            role(1, 0, 0),          // @everyone has no colour
            role(2, 16_711_680, 5), // red, low
            role(3, 65_280, 9),     // green, high
            role(4, 0, 20),         // highest of all but uncoloured
            role(5, 255, 30),       // blue, but this person does not have it
        ];
        let theirs = [1, 2, 3, 4].map(Snowflake::new);
        assert_eq!(name_color(&roles, &theirs), Some(65_280));
        assert_eq!(name_color(&roles, &[Snowflake::new(4)]), None);
        assert_eq!(name_color(&roles, &[]), None);
        assert_eq!(name_color(&[], &theirs), None);
        // Only the colour bits count.
        assert_eq!(
            name_color(&[role(7, 4_279_383_126, 1)], &[Snowflake::new(7)]),
            Some(1_193_046)
        );
    }

    #[test]
    fn a_nickname_beats_the_display_name_and_blank_does_not() {
        let roles = [role(2, 16_711_680, 5)];
        let ada = user(1, None);
        let boss = Member {
            nick: Some("The Boss".into()),
            roles: vec![Snowflake::new(2)],
        };
        assert_eq!(
            author_style(&ada, Some(&boss), &roles),
            AuthorStyle {
                name: "The Boss".into(),
                color: Some(16_711_680)
            }
        );
        let blank = Member {
            nick: Some(String::new()),
            roles: vec![],
        };
        assert_eq!(
            author_style(&ada, Some(&blank), &roles),
            AuthorStyle {
                name: "Ada".into(),
                color: None
            }
        );
        assert_eq!(
            author_style(&ada, None, &roles),
            AuthorStyle {
                name: "Ada".into(),
                color: None
            }
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
        // A code span is shown as typed, including channel markup.
        assert_eq!(
            clean_content("see <#7> but ` <#7> ` and \\<#7>", name),
            "see #general but ` <#7> ` and \\<#7>"
        );
        assert_eq!(
            clean_content("```\n<#7>\n```\nafter <#7>", name),
            "```\n<#7>\n```\nafter #general"
        );
    }

    #[test]
    fn an_empty_embed_image_does_not_hide_the_thumbnail_and_a_video_still_shows() {
        let thumb = Embed {
            thumbnail: Some(EmbedImage {
                url: "https://cdn.discordapp.com/thumb.png".into(),
                ..Default::default()
            }),
            image: Some(EmbedImage::default()),
            ..Default::default()
        };
        assert_eq!(
            embed_picture(&thumb).map(EmbedImage::best_url),
            Some("https://cdn.discordapp.com/thumb.png")
        );
        let video = Embed {
            kind: Some("video".into()),
            url: Some("https://example.test/clip".into()),
            ..Default::default()
        };
        assert!(embed_has_body(&video));
        assert!(!embed_has_body(&Embed::default()));
    }

    #[test]
    fn two_messages_from_one_person_keep_their_own_names() {
        let ada = user(1, None);
        let first = Message {
            id: Snowflake::new(10),
            author: ada.clone(),
            member: Some(Member {
                nick: Some("The Boss".into()),
                roles: Vec::new(),
            }),
            ..Default::default()
        };
        let second = Message {
            id: Snowflake::new(11),
            author: ada,
            ..Default::default()
        };
        let styles = author_styles([(&first, first.member.as_ref()), (&second, None)], &[]);
        assert_eq!(styles[&Snowflake::new(10)].name, "The Boss");
        assert_eq!(styles[&Snowflake::new(11)].name, "Ada");
    }

    #[test]
    fn a_mention_named_1_is_not_rewritten_as_the_demo_user() {
        let mut names = HashMap::new();
        names.insert("42".into(), "1".into());
        assert_eq!(mention_text("42", &names), "@1");
        assert_eq!(mention_text("1", &HashMap::new()), "@Ada");
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
