//! Account and guild upload caps.
//!
//! Discord rejects an attachment on the server. The numbers below are the
//! ones this client plans against, checked in November 2026:
//!
//! - API reference: the default limit is 20 MiB, and `attachment_size_limit`
//!   on an interaction is the max of the account cap and the boost cap.
//! - File Attachments FAQ (30 Sep 2026): no Nitro 20 MB, Nitro Basic 50 MB,
//!   Nitro up to 1 GB. The same FAQ says the free cap was 10 MB before
//!   August 2026. An older account-caps table still says 10 MB.
//! - Server Boosting FAQ: level 2 is 50 MB and level 3 is 100 MB, for members
//!   of that server only. Level 1 does not raise the cap. A separate
//!   experiment can raise a server to 250 MB.
//!
//! When the API sends `attachment_size_limit`, that value wins. A file under
//! the planned cap can still be rejected if Discord is running a different
//! experiment for that account.

use crate::UploadError;
use serde::Deserialize;

/// 20 × 1024 × 1024. The API reference calls this 20 MiB.
pub const FREE_LIMIT_BYTES: u64 = 20 * 1024 * 1024;
/// Nitro Basic, and the legacy Nitro Classic premium type.
pub const NITRO_BASIC_LIMIT_BYTES: u64 = 50 * 1024 * 1024;
/// Nitro. Marketing says "up to 1 GB"; the client plans against 1 GiB.
pub const NITRO_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;
pub const BOOST_TIER2_BYTES: u64 = 50 * 1024 * 1024;
pub const BOOST_TIER3_BYTES: u64 = 100 * 1024 * 1024;
/// Optional "Larger File Uploads" perk some servers can buy. Off unless the
/// caller says the guild has it.
pub const BOOST_PERK_BYTES: u64 = 250 * 1024 * 1024;

/// Where a planned cap came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitSource {
    /// The API sent `attachment_size_limit`.
    Api,
    /// Direct message, or a guild whose boost does not raise the cap.
    Account { premium_type: u8 },
    /// The guild boost is what raised the cap.
    Guild { tier: u8 },
    /// Both were considered. `bytes` is the larger of the two.
    Max { account: u64, guild: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadLimit {
    pub bytes: u64,
    pub source: LimitSource,
    pub premium_type: u8,
    pub guild_tier: Option<u8>,
    pub in_guild: bool,
}

impl UploadLimit {
    pub fn label(self) -> String {
        match self.premium_type {
            1 => "Nitro Classic".to_owned(),
            2 => "Nitro".to_owned(),
            3 => "Nitro Basic".to_owned(),
            _ => "no Nitro".to_owned(),
        }
    }
}

/// Inputs for [`detect_limit`]. `premium_type` is Discord's user field:
/// 0 none, 1 Nitro Classic, 2 Nitro, 3 Nitro Basic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitInput {
    pub premium_type: u8,
    pub guild_tier: Option<u8>,
    pub in_guild: bool,
    pub larger_file_perk: bool,
    pub attachment_size_limit: Option<u64>,
}

impl LimitInput {
    pub fn free_dm() -> Self {
        Self {
            premium_type: 0,
            guild_tier: None,
            in_guild: false,
            larger_file_perk: false,
            attachment_size_limit: None,
        }
    }
}

pub fn account_limit(premium_type: u8) -> u64 {
    match premium_type {
        1 | 3 => NITRO_BASIC_LIMIT_BYTES,
        2 => NITRO_LIMIT_BYTES,
        _ => FREE_LIMIT_BYTES,
    }
}

pub fn guild_limit(tier: u8, larger_file_perk: bool) -> u64 {
    let tier_cap = match tier {
        2 => BOOST_TIER2_BYTES,
        3 => BOOST_TIER3_BYTES,
        _ => FREE_LIMIT_BYTES,
    };
    if larger_file_perk {
        tier_cap.max(BOOST_PERK_BYTES)
    } else {
        tier_cap
    }
}

/// The cap this client will plan against. Discord still makes the real decision.
pub fn detect_limit(input: LimitInput) -> UploadLimit {
    if let Some(bytes) = input.attachment_size_limit {
        return UploadLimit {
            bytes,
            source: LimitSource::Api,
            premium_type: input.premium_type,
            guild_tier: input.guild_tier,
            in_guild: input.in_guild,
        };
    }
    let account = account_limit(input.premium_type);
    if !input.in_guild {
        return UploadLimit {
            bytes: account,
            source: LimitSource::Account {
                premium_type: input.premium_type,
            },
            premium_type: input.premium_type,
            guild_tier: input.guild_tier,
            in_guild: false,
        };
    }
    let tier = input.guild_tier.unwrap_or(0);
    let guild = guild_limit(tier, input.larger_file_perk);
    let bytes = account.max(guild);
    let source = if account == guild {
        LimitSource::Max { account, guild }
    } else if bytes == guild && guild > account {
        LimitSource::Guild { tier }
    } else if guild > 0 && account > guild {
        LimitSource::Max { account, guild }
    } else {
        LimitSource::Account {
            premium_type: input.premium_type,
        }
    };
    UploadLimit {
        bytes,
        source,
        premium_type: input.premium_type,
        guild_tier: Some(tier),
        in_guild: true,
    }
}

#[derive(Debug, Deserialize)]
struct UserPremium {
    #[serde(default)]
    premium_type: u8,
    #[serde(default)]
    attachment_size_limit: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GuildPremium {
    #[serde(default)]
    premium_tier: u8,
    #[serde(default)]
    attachment_size_limit: Option<u64>,
}

/// Read `premium_type` and `premium_tier` out of `/users/@me` and `/guilds/{id}` JSON.
pub fn limit_from_json(
    user_json: &str,
    guild_json: Option<&str>,
    in_guild: bool,
    larger_file_perk: bool,
) -> Result<UploadLimit, UploadError> {
    let user: UserPremium = serde_json::from_str(user_json)
        .map_err(|err| UploadError::message(format!("user JSON: {err}")))?;
    let (tier, guild_override) = if let Some(guild_json) = guild_json {
        let guild: GuildPremium = serde_json::from_str(guild_json)
            .map_err(|err| UploadError::message(format!("guild JSON: {err}")))?;
        (Some(guild.premium_tier), guild.attachment_size_limit)
    } else {
        (None, None)
    };
    let attachment_size_limit = user.attachment_size_limit.or(guild_override);
    Ok(detect_limit(LimitInput {
        premium_type: user.premium_type,
        guild_tier: tier,
        in_guild,
        larger_file_perk,
        attachment_size_limit,
    }))
}

/// One decimal "MB" where MB means 1024 × 1024 bytes, matching the cap constants.
pub fn format_bytes(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    if bytes >= MIB {
        let whole = bytes / MIB;
        let frac = (bytes % MIB) * 10 / MIB;
        format!("{whole}.{frac} MB")
    } else if bytes >= KIB {
        format!("{} KB", bytes / KIB)
    } else {
        format!("{bytes} B")
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_account_is_twenty_mib_and_nitro_is_higher() {
        let free = detect_limit(LimitInput::free_dm());
        assert_eq!(free.bytes, FREE_LIMIT_BYTES);
        assert_eq!(free.label(), "no Nitro");
        assert_eq!(account_limit(3), NITRO_BASIC_LIMIT_BYTES);
        assert_eq!(account_limit(1), NITRO_BASIC_LIMIT_BYTES);
        assert_eq!(account_limit(2), NITRO_LIMIT_BYTES);
        assert_eq!(account_limit(9), FREE_LIMIT_BYTES);
    }

    #[test]
    fn guild_boost_raises_the_cap_only_inside_the_guild() {
        let tier3 = detect_limit(LimitInput {
            premium_type: 0,
            guild_tier: Some(3),
            in_guild: true,
            larger_file_perk: false,
            attachment_size_limit: None,
        });
        assert_eq!(tier3.bytes, BOOST_TIER3_BYTES);
        assert_eq!(tier3.source, LimitSource::Guild { tier: 3 });

        let dm = detect_limit(LimitInput {
            premium_type: 0,
            guild_tier: Some(3),
            in_guild: false,
            larger_file_perk: false,
            attachment_size_limit: None,
        });
        assert_eq!(dm.bytes, FREE_LIMIT_BYTES);

        let nitro_in_tier2 = detect_limit(LimitInput {
            premium_type: 2,
            guild_tier: Some(2),
            in_guild: true,
            larger_file_perk: false,
            attachment_size_limit: None,
        });
        assert_eq!(nitro_in_tier2.bytes, NITRO_LIMIT_BYTES);

        let basic_in_tier3 = detect_limit(LimitInput {
            premium_type: 3,
            guild_tier: Some(3),
            in_guild: true,
            larger_file_perk: false,
            attachment_size_limit: None,
        });
        assert_eq!(basic_in_tier3.bytes, BOOST_TIER3_BYTES);
    }

    #[test]
    fn level_one_does_not_raise_the_cap_and_the_perk_does() {
        let tier1 = detect_limit(LimitInput {
            premium_type: 0,
            guild_tier: Some(1),
            in_guild: true,
            larger_file_perk: false,
            attachment_size_limit: None,
        });
        assert_eq!(tier1.bytes, FREE_LIMIT_BYTES);
        let perk = detect_limit(LimitInput {
            premium_type: 0,
            guild_tier: Some(1),
            in_guild: true,
            larger_file_perk: true,
            attachment_size_limit: None,
        });
        assert_eq!(perk.bytes, BOOST_PERK_BYTES);
    }

    #[test]
    fn api_attachment_size_limit_overrides_the_table() {
        let limit = detect_limit(LimitInput {
            premium_type: 2,
            guild_tier: Some(3),
            in_guild: true,
            larger_file_perk: false,
            attachment_size_limit: Some(1234),
        });
        assert_eq!(limit.bytes, 1234);
        assert_eq!(limit.source, LimitSource::Api);
    }

    #[test]
    fn json_user_and_guild_feed_the_same_table() {
        let limit = limit_from_json(
            r#"{"id":"1","premium_type":0}"#,
            Some(r#"{"id":"9","premium_tier":2}"#),
            true,
            false,
        )
        .unwrap();
        assert_eq!(limit.bytes, BOOST_TIER2_BYTES);
        let overridden = limit_from_json(
            r#"{"premium_type":0,"attachment_size_limit":4096}"#,
            None,
            false,
            false,
        )
        .unwrap();
        assert_eq!(overridden.bytes, 4096);
        assert_eq!(overridden.source, LimitSource::Api);
    }

    #[test]
    fn format_bytes_uses_1024() {
        assert_eq!(format_bytes(FREE_LIMIT_BYTES), "20.0 MB");
        assert_eq!(format_bytes(1536), "1 KB");
        assert_eq!(format_bytes(12), "12 B");
    }
}
