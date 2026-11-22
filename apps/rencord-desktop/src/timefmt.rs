//! Local presentation of a message timestamp.
//!
//! Discord sends RFC3339. The timeline shows Today, Yesterday, or a local
//! date, and the hover text is the full local date and time. A string that is
//! already prose (the demo fixture) is left as it is.

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Timelike};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presented {
    pub short: String,
    pub full: String,
    pub day: String,
}

pub fn now_local() -> DateTime<FixedOffset> {
    chrono::Local::now().fixed_offset()
}

pub fn present_time(raw: &str, now: DateTime<FixedOffset>) -> Presented {
    let Ok(parsed) = DateTime::parse_from_rfc3339(raw) else {
        let day = if raw.starts_with("Yesterday") {
            "Yesterday".to_owned()
        } else if raw.starts_with("Today") {
            "Today".to_owned()
        } else {
            String::new()
        };
        return Presented {
            short: raw.to_owned(),
            full: raw.to_owned(),
            day,
        };
    };
    let local = parsed.with_timezone(now.offset());
    let day = day_label(local.date_naive(), now.date_naive());
    let clock = clock_label(local.hour(), local.minute());
    let short = if day == "Today" {
        clock.clone()
    } else if day == "Yesterday" {
        format!("Yesterday at {clock}")
    } else {
        format!("{}/{}/{} {clock}", local.month(), local.day(), local.year())
    };
    let full = format!(
        "{}, {} {}, {} at {clock}",
        weekday(local.weekday()),
        month(local.month()),
        local.day(),
        local.year(),
    );
    Presented { short, full, day }
}

/// Same author, both ordinary messages, less than seven minutes apart.
pub const GROUP_WINDOW_MS: i64 = 7 * 60 * 1000;

pub fn groups_with(
    previous_author: u64,
    previous_timestamp: &str,
    previous_kind: u8,
    next_author: u64,
    next_timestamp: &str,
    next_kind: u8,
) -> bool {
    if previous_author != next_author || previous_kind != 0 || next_kind != 0 {
        return false;
    }
    let (Ok(previous), Ok(next)) = (
        DateTime::parse_from_rfc3339(previous_timestamp),
        DateTime::parse_from_rfc3339(next_timestamp),
    ) else {
        return false;
    };
    (next.timestamp_millis() - previous.timestamp_millis()).unsigned_abs()
        < u64::try_from(GROUP_WINDOW_MS).unwrap_or(0)
}

/// A divider belongs between two messages when their local days differ.
pub fn day_changed(previous: Option<&str>, current: &str, now: DateTime<FixedOffset>) -> bool {
    let current = present_time(current, now);
    if current.day.is_empty() {
        return false;
    }
    match previous {
        None => true,
        Some(previous) => present_time(previous, now).day != current.day,
    }
}

fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        "Today".to_owned()
    } else if day.succ_opt() == Some(today) {
        "Yesterday".to_owned()
    } else {
        format!("{} {}, {}", month(day.month()), day.day(), day.year())
    }
}

fn clock_label(hour: u32, minute: u32) -> String {
    let suffix = if hour < 12 { "AM" } else { "PM" };
    let hour = match hour % 12 {
        0 => 12,
        hour => hour,
    };
    format!("{hour}:{minute:02} {suffix}")
}

fn weekday(day: chrono::Weekday) -> &'static str {
    use chrono::Weekday::*;
    match day {
        Mon => "Monday",
        Tue => "Tuesday",
        Wed => "Wednesday",
        Thu => "Thursday",
        Fri => "Friday",
        Sat => "Saturday",
        Sun => "Sunday",
    }
}

fn month(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        _ => "December",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn an_older_utc_day_is_not_labeled_today() {
        let now = at(2026, 11, 21, 18, 0);
        let stamp = present_time("2026-11-20T00:00:00Z", now);
        assert_eq!(stamp.day, "Yesterday");
        assert_eq!(stamp.short, "Yesterday at 12:00 AM");
        assert_eq!(stamp.full, "Friday, November 20, 2026 at 12:00 AM");
        assert!(day_changed(None, "2026-11-20T00:00:00Z", now));
        assert!(!day_changed(
            Some("2026-11-20T00:00:00Z"),
            "2026-11-20T22:04:00Z",
            now
        ));
        assert!(day_changed(
            Some("2026-11-20T22:04:00Z"),
            "2026-11-21T22:04:00Z",
            now
        ));
        let today = present_time("2026-11-21T22:04:00Z", now);
        assert_eq!(today.short, "10:04 PM");
        assert!(!groups_with(
            1,
            "2026-11-21T22:00:00Z",
            0,
            1,
            "2026-11-21T22:08:00Z",
            0,
        ));
        assert!(groups_with(
            1,
            "2026-11-21T22:00:00Z",
            0,
            1,
            "2026-11-21T22:06:00Z",
            0,
        ));
        assert!(!groups_with(
            1,
            "2026-11-21T22:00:00Z",
            0,
            1,
            "2026-11-21T22:06:00Z",
            7,
        ));
    }

    #[test]
    fn a_negative_offset_keeps_the_local_evening() {
        let zone = FixedOffset::west_opt(5 * 3600).unwrap();
        let now = zone.with_ymd_and_hms(2026, 11, 21, 18, 0, 0).unwrap();
        let stamp = present_time("2026-11-21T22:04:00Z", now);
        assert_eq!(stamp.short, "5:04 PM");
        assert_eq!(stamp.full, "Saturday, November 21, 2026 at 5:04 PM");
    }
}
