//! The clock, in one place.
//!
//! The core crate never reads a clock: every function that depends on the date takes it
//! as a parameter, which is what makes the obligations reproducible in a test. This file
//! is the one place that asks the machine what day it is — and only the day, and only in
//! Lisbon, because a Portuguese tax deadline is a Portuguese calendar day.
//!
//! The offset comes from the European Union's own rule — summer time from the last
//! Sunday of March at 01:00 UTC to the last Sunday of October at 01:00 UTC — rather than
//! from a time-zone database, because one rule for one city is a dozen lines and a
//! database is a megabyte that must be kept current. If the Union ever abolishes the
//! change, it is one function.
//!
//! Timestamps for records live in [`vnf_store::clock`], in the storage layer, because an
//! audit line without a time is not a record.

use vnf_core::dates;

/// The date `YYYY-MM-DD` in Europe/Lisbon, right now.
#[must_use]
pub fn today_in_lisbon() -> String {
    iso_date(local_seconds(vnf_store::clock::now_epoch_millis() / 1000))
}

/// The seconds on the Lisbon wall clock, given seconds on the UTC clock.
#[must_use]
pub fn local_seconds(utc_seconds: i64) -> i64 {
    utc_seconds + summer_offset(utc_seconds)
}

/// One hour in summer, nothing in winter.
fn summer_offset(utc_seconds: i64) -> i64 {
    let Some(year) = year_of(iso_date(utc_seconds)) else {
        return 0;
    };
    let (Some(start), Some(end)) = (last_sunday(year, 3), last_sunday(year, 10)) else {
        return 0;
    };
    // Both boundaries are at 01:00 UTC on the day the clocks move.
    let start = epoch_day(&start).saturating_mul(86_400) + 3_600;
    let end = epoch_day(&end).saturating_mul(86_400) + 3_600;
    if utc_seconds >= start && utc_seconds < end {
        3_600
    } else {
        0
    }
}

/// The last Sunday of a month, as `YYYY-MM-DD`.
fn last_sunday(year: i64, month: i64) -> Option<String> {
    let month = u32::try_from(month).ok()?;
    let mut day = dates::make_iso(year, month, dates::days_in_month(year, month));
    for _ in 0..7 {
        if dates::day_of_week(&day).ok()? == 0 {
            return Some(day);
        }
        day = dates::add_days(&day, -1).ok()?;
    }
    None
}

/// Whole days from 1970-01-01 to an ISO date.
fn epoch_day(iso: &str) -> i64 {
    dates::days_between("1970-01-01", iso).unwrap_or(0)
}

/// The ISO date for a number of seconds since the epoch.
fn iso_date(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    dates::add_days("1970-01-01", days).unwrap_or_else(|_| "1970-01-01".to_owned())
}

/// The first four characters of an ISO date, as a year.
fn year_of(iso: String) -> Option<i64> {
    iso.get(..4)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(iso: &str, hour: i64, minute: i64, second: i64) -> i64 {
        epoch_day(iso) * 86_400 + hour * 3_600 + minute * 60 + second
    }

    #[test]
    fn winter_is_utc() {
        let instant = at("2026-01-15", 12, 0, 0);
        assert_eq!(summer_offset(instant), 0);
        assert_eq!(iso_date(local_seconds(instant)), "2026-01-15");
    }

    #[test]
    fn summer_is_one_hour_ahead() {
        let instant = at("2026-07-15", 12, 0, 0);
        assert_eq!(summer_offset(instant), 3_600);
        assert_eq!(iso_date(local_seconds(instant)), "2026-07-15");
    }

    #[test]
    fn the_change_happens_at_one_in_the_morning_utc() {
        // The last Sunday of March 2026 is the 29th.
        let before = at("2026-03-29", 0, 59, 0);
        let after = at("2026-03-29", 1, 0, 0);
        assert_eq!(summer_offset(before), 0);
        assert_eq!(summer_offset(after), 3_600);
        // And back, on the last Sunday of October, the 25th.
        let before = at("2026-10-25", 0, 59, 0);
        let after = at("2026-10-25", 1, 0, 0);
        assert_eq!(summer_offset(before), 3_600);
        assert_eq!(summer_offset(after), 0);
    }

    #[test]
    fn a_late_evening_in_utc_can_already_be_tomorrow_in_lisbon() {
        // 23:30 UTC in summer is 00:30 the next day in Lisbon.
        let instant = at("2026-07-15", 23, 30, 0);
        assert_eq!(iso_date(instant), "2026-07-15");
        assert_eq!(iso_date(local_seconds(instant)), "2026-07-16");
        // In winter it is still the same day.
        let winter = at("2026-01-15", 23, 30, 0);
        assert_eq!(iso_date(local_seconds(winter)), "2026-01-15");
    }

    #[test]
    fn the_boundaries_are_sundays() {
        for (year, month) in [
            (2024, 3),
            (2024, 10),
            (2025, 3),
            (2025, 10),
            (2026, 3),
            (2026, 10),
        ] {
            let day = last_sunday(year, month).unwrap_or_default();
            assert_eq!(dates::day_of_week(&day).unwrap_or(9), 0, "{day} must be a Sunday");
            // And it is the LAST one: the following Sunday is in the next month.
            let next = dates::add_days(&day, 7).unwrap_or_default();
            assert_ne!(next.get(5..7), day.get(5..7), "{day} must be the last Sunday");
        }
    }

    #[test]
    fn today_is_a_ten_character_date() {
        let today = today_in_lisbon();
        assert_eq!(today.len(), 10);
        assert!(dates::parse_iso(&today).is_ok());
    }
}
