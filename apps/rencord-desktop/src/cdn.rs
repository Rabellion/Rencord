//! Discord CDN paths.
//!
//! Production requests `https://cdn.discordapp.com`. Tests and the local stub
//! point the base at a loopback server. Nothing in this module opens a socket.

use std::sync::OnceLock;

static CDN_BASE: OnceLock<String> = OnceLock::new();

pub fn set_cdn_base(base: impl Into<String>) {
    let _ = CDN_BASE.set(base.into().trim_end_matches('/').to_owned());
}

pub fn cdn_base() -> String {
    CDN_BASE
        .get()
        .cloned()
        .unwrap_or_else(|| "https://cdn.discordapp.com".to_owned())
}

/// New accounts use `(id >> 22) % 6`. A legacy discriminator above 0 uses `% 5`.
pub fn default_avatar_index(user_id: u64, discriminator: Option<&str>) -> u8 {
    if let Some(discriminator) = discriminator {
        if let Ok(value) = discriminator.parse::<u64>() {
            if value > 0 {
                return (value % 5) as u8;
            }
        }
    }
    ((user_id >> 22) % 6) as u8
}

pub fn default_avatar_url(user_id: u64, discriminator: Option<&str>) -> String {
    format!(
        "{}/embed/avatars/{}.png",
        cdn_base(),
        default_avatar_index(user_id, discriminator)
    )
}

/// Still frame. Animated hashes (`a_…`) are requested as PNG so the first frame shows.
pub fn user_avatar_url(user_id: u64, hash: &str) -> String {
    format!("{}/avatars/{user_id}/{hash}.png?size=128", cdn_base())
}

pub fn member_avatar_url(guild_id: u64, user_id: u64, hash: &str) -> String {
    format!(
        "{}/guilds/{guild_id}/users/{user_id}/avatars/{hash}.png?size=128",
        cdn_base()
    )
}

pub fn guild_icon_url(guild_id: u64, hash: &str) -> String {
    format!("{}/icons/{guild_id}/{hash}.png?size=128", cdn_base())
}

pub fn guild_banner_url(guild_id: u64, hash: &str) -> String {
    format!("{}/banners/{guild_id}/{hash}.png?size=480", cdn_base())
}

pub fn emoji_url(emoji_id: u64) -> String {
    format!("{}/emojis/{emoji_id}.webp", cdn_base())
}

/// Prefer Discord's proxy when the message includes one.
pub fn attachment_url(url: &str, proxy_url: Option<&str>) -> String {
    proxy_url
        .filter(|proxy| !proxy.is_empty())
        .unwrap_or(url)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_urls_use_the_cdn_paths_and_a_test_base() {
        set_cdn_base("http://127.0.0.1:9");
        assert_eq!(default_avatar_index(1 << 22, None), 1);
        assert_eq!(default_avatar_index(4, Some("3")), 3);
        assert_eq!(
            user_avatar_url(9, "a_abc"),
            "http://127.0.0.1:9/avatars/9/a_abc.png?size=128"
        );
        assert_eq!(
            member_avatar_url(10, 9, "mem"),
            "http://127.0.0.1:9/guilds/10/users/9/avatars/mem.png?size=128"
        );
        assert_eq!(
            guild_icon_url(10, "icon"),
            "http://127.0.0.1:9/icons/10/icon.png?size=128"
        );
        assert_eq!(
            guild_banner_url(10, "ban"),
            "http://127.0.0.1:9/banners/10/ban.png?size=480"
        );
        assert_eq!(
            default_avatar_url(1 << 22, None),
            "http://127.0.0.1:9/embed/avatars/1.png"
        );
        assert_eq!(
            attachment_url(
                "https://cdn.example/a.png",
                Some("https://media.example/a.png")
            ),
            "https://media.example/a.png"
        );
    }
}
