//! The channel list of a server, ordered the way Discord shows it.
//!
//! Channels with no category come first, then each category with its channels.
//! Inside either, text channels come before voice channels, and each group is in
//! the server's own order. A category with nothing visible in it is not shown.

use std::collections::HashSet;

use rencord_model::{Channel, ChannelKind, Snowflake};

/// One block of the channel list: a category header (if any) and its channels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelSection {
    pub category: Option<Channel>,
    pub channels: Vec<Channel>,
}

fn is_listed(kind: ChannelKind) -> bool {
    // A forum has no message view here, so it is not a sidebar row.
    !matches!(
        kind,
        ChannelKind::GuildCategory | ChannelKind::PublicThread | ChannelKind::GuildForum
    )
}

/// Text before voice, then the server's own position, then id for a stable tie.
fn order(channels: &mut [Channel]) {
    channels.sort_by_key(|channel| (channel.kind.is_voice(), channel.position, channel.id.get()));
}

pub fn channel_sections(channels: &[Channel]) -> Vec<ChannelSection> {
    let mut categories: Vec<&Channel> = channels
        .iter()
        .filter(|channel| channel.kind == ChannelKind::GuildCategory)
        .collect();
    categories.sort_by_key(|category| (category.position, category.id.get()));
    let known: HashSet<Snowflake> = categories.iter().map(|category| category.id).collect();

    // A channel whose category is not in the list is shown with the loose ones.
    let mut loose: Vec<Channel> = channels
        .iter()
        .filter(|channel| is_listed(channel.kind))
        .filter(|channel| {
            channel
                .parent_id
                .is_none_or(|parent| !known.contains(&parent))
        })
        .cloned()
        .collect();
    order(&mut loose);

    let mut sections = Vec::new();
    if !loose.is_empty() {
        sections.push(ChannelSection {
            category: None,
            channels: loose,
        });
    }
    for category in categories {
        let mut children: Vec<Channel> = channels
            .iter()
            .filter(|channel| is_listed(channel.kind) && channel.parent_id == Some(category.id))
            .cloned()
            .collect();
        if children.is_empty() {
            continue;
        }
        order(&mut children);
        sections.push(ChannelSection {
            category: Some(category.clone()),
            channels: children,
        });
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(
        id: u64,
        name: &str,
        kind: ChannelKind,
        parent: Option<u64>,
        position: i32,
    ) -> Channel {
        Channel {
            id: Snowflake::new(id),
            name: name.into(),
            kind,
            parent_id: parent.map(Snowflake::new),
            position,
            ..Default::default()
        }
    }

    fn names(section: &ChannelSection) -> Vec<&str> {
        section.channels.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn loose_channels_come_first_then_categories_in_position_order() {
        let list = vec![
            channel(30, "Community", ChannelKind::GuildCategory, None, 2),
            channel(10, "Start here", ChannelKind::GuildCategory, None, 1),
            channel(11, "rules", ChannelKind::GuildText, Some(10), 1),
            channel(31, "general", ChannelKind::GuildText, Some(30), 0),
            channel(50, "server-hub", ChannelKind::GuildText, None, 0),
        ];
        let sections = channel_sections(&list);
        assert_eq!(sections.len(), 3);
        assert!(sections[0].category.is_none());
        assert_eq!(names(&sections[0]), ["server-hub"]);
        assert_eq!(sections[1].category.as_ref().unwrap().name, "Start here");
        assert_eq!(sections[2].category.as_ref().unwrap().name, "Community");
    }

    #[test]
    fn text_channels_come_before_voice_inside_a_category() {
        let list = vec![
            channel(1, "Voice", ChannelKind::GuildCategory, None, 0),
            channel(2, "Lobby 1", ChannelKind::GuildVoice, Some(1), 0),
            channel(3, "chat", ChannelKind::GuildText, Some(1), 5),
            channel(4, "Lobby 2", ChannelKind::GuildVoice, Some(1), 1),
            channel(5, "afk-chat", ChannelKind::GuildText, Some(1), 2),
        ];
        let sections = channel_sections(&list);
        assert_eq!(
            names(&sections[0]),
            ["afk-chat", "chat", "Lobby 1", "Lobby 2"]
        );
    }

    #[test]
    fn an_empty_category_threads_and_a_missing_category_are_handled() {
        let list = vec![
            channel(1, "Empty", ChannelKind::GuildCategory, None, 0),
            channel(2, "Busy", ChannelKind::GuildCategory, None, 1),
            channel(3, "talk", ChannelKind::GuildText, Some(2), 0),
            channel(4, "a thread", ChannelKind::PublicThread, Some(3), 0),
            // Its category is not in the list, so it is shown with the loose ones.
            channel(5, "orphan", ChannelKind::GuildText, Some(999), 0),
            channel(6, "ideas", ChannelKind::GuildForum, Some(2), 1),
        ];
        let sections = channel_sections(&list);
        assert_eq!(sections.len(), 2);
        assert_eq!(names(&sections[0]), ["orphan"]);
        assert_eq!(sections[1].category.as_ref().unwrap().name, "Busy");
        assert_eq!(names(&sections[1]), ["talk"]);
        assert!(channel_sections(&[]).is_empty());
    }

    #[test]
    fn equal_positions_keep_a_stable_order_by_id() {
        let list = vec![
            channel(9, "b", ChannelKind::GuildText, None, 0),
            channel(3, "a", ChannelKind::GuildText, None, 0),
        ];
        assert_eq!(names(&channel_sections(&list)[0]), ["a", "b"]);
    }
}
