//! Channel, guild, and DM search.
//!
//! Discord's query language (`from:`, `in:`, `has:`, `before:`, `after:`) is
//! parsed here. The same query filters the local cache. A guild search URL is
//! built for the REST client when the user is signed in.

use rencord_model::{Message, Snowflake};
use rencord_store::Session;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageQuery {
    pub free: String,
    pub from: Option<String>,
    pub in_channel: Option<String>,
    pub has: Vec<String>,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub channel_id: Snowflake,
    pub channel_name: String,
    pub message: Message,
}

pub fn parse_message_query(input: &str) -> MessageQuery {
    let mut query = MessageQuery {
        free: String::new(),
        from: None,
        in_channel: None,
        has: Vec::new(),
        before: None,
        after: None,
    };
    let mut free = Vec::new();
    for token in input.split_whitespace() {
        if let Some(value) = token.strip_prefix("from:") {
            if !value.is_empty() {
                query.from = Some(value.trim_start_matches('@').to_owned());
            }
        } else if let Some(value) = token.strip_prefix("in:") {
            if !value.is_empty() {
                query.in_channel = Some(value.trim_start_matches('#').to_owned());
            }
        } else if let Some(value) = token.strip_prefix("has:") {
            if !value.is_empty() {
                query.has.push(value.to_ascii_lowercase());
            }
        } else if let Some(value) = token.strip_prefix("before:") {
            if !value.is_empty() {
                query.before = Some(value.to_owned());
            }
        } else if let Some(value) = token.strip_prefix("after:") {
            if !value.is_empty() {
                query.after = Some(value.to_owned());
            }
        } else {
            free.push(token);
        }
    }
    query.free = free.join(" ").to_ascii_lowercase();
    query
}

pub fn local_search(session: &Session, raw: &str) -> Vec<SearchHit> {
    let query = parse_message_query(raw);
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let mut channels: Vec<_> = session.channels.values().collect();
    channels.sort_by_key(|channel| channel.id.get());
    for channel in channels {
        if let Some(name) = &query.in_channel {
            if !channel.name.eq_ignore_ascii_case(name) {
                continue;
            }
        }
        for message in session.messages_for(channel.id) {
            if hit_matches(&query, message) {
                hits.push(SearchHit {
                    channel_id: channel.id,
                    channel_name: channel.name.clone(),
                    message: message.clone(),
                });
            }
        }
    }
    hits
}

fn hit_matches(query: &MessageQuery, message: &Message) -> bool {
    if let Some(from) = &query.from {
        let name = message.author.display_name();
        let id = message.author.id.get().to_string();
        if !name.eq_ignore_ascii_case(from)
            && !message.author.username.eq_ignore_ascii_case(from)
            && id != *from
        {
            return false;
        }
    }
    if let Some(before) = &query.before {
        if message.timestamp.as_str() >= before.as_str() {
            return false;
        }
    }
    if let Some(after) = &query.after {
        if message.timestamp.as_str() <= after.as_str() {
            return false;
        }
    }
    for kind in &query.has {
        let ok = match kind.as_str() {
            "link" => {
                message.content.contains("http://")
                    || message.content.contains("https://")
                    || message.content.contains("](")
            }
            "code" => message.content.contains("```") || message.content.contains('`'),
            "embed" | "file" | "image" | "video" | "sound" => false,
            other => message.content.to_ascii_lowercase().contains(other),
        };
        if !ok {
            return false;
        }
    }
    if query.free.is_empty() {
        return true;
    }
    message.content.to_ascii_lowercase().contains(&query.free)
        || message
            .author
            .display_name()
            .to_ascii_lowercase()
            .contains(&query.free)
}

/// Path Discord documents for guild message search. Author and channel ids are
/// filled by the caller when a `from:` or `in:` name resolved to an id.
pub fn guild_search_path(
    guild_id: Snowflake,
    query: &MessageQuery,
    author_id: Option<Snowflake>,
    channel_id: Option<Snowflake>,
) -> String {
    let mut pairs = Vec::new();
    if !query.free.is_empty() {
        pairs.push(format!("content={}", url_encode(&query.free)));
    }
    if let Some(id) = author_id {
        pairs.push(format!("author_id={id}"));
    }
    if let Some(id) = channel_id {
        pairs.push(format!("channel_id={id}"));
    }
    if query.has.iter().any(|item| item == "link") {
        pairs.push("has=link".into());
    }
    if query.has.iter().any(|item| item == "embed") {
        pairs.push("has=embed".into());
    }
    if query.has.iter().any(|item| item == "file") {
        pairs.push("has=file".into());
    }
    if let Some(after) = &query.after {
        pairs.push(format!("min_id={}", url_encode(after)));
    }
    if let Some(before) = &query.before {
        pairs.push(format!("max_id={}", url_encode(before)));
    }
    format!(
        "/guilds/{guild_id}/messages/search?{}",
        if pairs.is_empty() {
            "content=".to_owned()
        } else {
            pairs.join("&")
        }
    )
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rencord_model::{Channel, ChannelKind, User};
    use rencord_store::Session;

    fn session() -> Session {
        let mut session = Session::demo();
        session.insert_message(Message {
            id: Snowflake::new(3000),
            channel_id: Snowflake::new(110),
            author: User {
                id: Snowflake::new(2),
                username: "charles".into(),
                global_name: Some("Charles".into()),
                discriminator: None,
                bot: false,
            },
            content: "notes mention https://example.com and `code`".into(),
            timestamp: "2026-11-19T00:00:00Z".into(),
        });
        let _ = ChannelKind::GuildText;
        session
    }

    #[test]
    fn filters_span_channels() {
        let session = session();
        let hits = local_search(&session, "from:charles has:link in:notes");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].channel_name, "notes");
        assert!(local_search(&session, "from:ada before:2026-11-19T12:00:00Z").is_empty());
        let general = local_search(&session, "in:general **Rencord**");
        assert!(general
            .iter()
            .any(|hit| hit.message.content.contains("Rencord")));
    }

    #[test]
    fn guild_path_encodes_filters() {
        let query = parse_message_query("from:ada hello in:general has:link before:99");
        let path = guild_search_path(
            Snowflake::new(10),
            &query,
            Some(Snowflake::new(1)),
            Some(Snowflake::new(100)),
        );
        assert!(path.starts_with("/guilds/10/messages/search?"));
        assert!(path.contains("content=hello"));
        assert!(path.contains("author_id=1"));
        assert!(path.contains("channel_id=100"));
        assert!(path.contains("has=link"));
        assert!(path.contains("max_id=99"));
        let _ = Channel {
            id: Snowflake::new(1),
            name: "x".into(),
            kind: ChannelKind::GuildText,
            guild_id: None,
            parent_id: None,
            position: 0,
            recipient_ids: Vec::new(),
        };
    }
}
