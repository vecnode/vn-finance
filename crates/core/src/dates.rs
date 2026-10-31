//! Calendar arithmetic for Portuguese tax deadlines.
//!
//! All arithmetic is done on plain ISO date strings, in a civil (timezone-free)
//! calendar, so a machine in any timezone produces the same deadline. Nothing
//! here reads the clock: the caller always passes `today` explicitly, which is
//! what makes the engine testable and reproducible.
//!
//! The arithmetic is written out rather than delegated to a date crate, for two
//! reasons. The reference implementation has a **deliberately lenient**
//! [`parse_iso`] — it accepts `2026-02-31`, because it only checks that a month is
//! 1–12 and a day is 1–31 — and reaching for a stricter parser would silently
//! reject profiles and rule-pack dates that work today. And the month arithmetic
//! reproduces the reference implementation's clamping order exactly, which is a
//! behaviour the published 2026 dates depend on.

use core::fmt;

use crate::types::IsoDate;

/// Portuguese month names, January first.
pub const PT_MONTHS: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];

/// The three parts of a parsed ISO date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateParts {
    /// Four-digit year.
    pub year: i64,
    /// Month, 1–12.
    pub month: u32,
    /// Day of the month, 1–31 (not checked against the month's length).
    pub day: u32,
}

/// Why a date could not be read or built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateError {
    /// The text is not `YYYY-MM-DD`.
    Malformed {
        /// The text as it arrived.
        input: String,
    },
    /// The text has the shape, but a part is out of range.
    OutOfRange {
        /// The text as it arrived.
        input: String,
    },
    /// A quarter outside 1–4.
    Quarter {
        /// The quarter that was asked for.
        quarter: i64,
    },
    /// A month outside 1–12.
    Month {
        /// The month that was asked for.
        month: i64,
    },
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { input } => write!(f, "invalid ISO date: \"{input}\""),
            Self::OutOfRange { input } => write!(f, "ISO date out of range: {input}"),
            Self::Quarter { quarter } => write!(f, "quarter out of range: {quarter}"),
            Self::Month { month } => write!(f, "month out of range: {month}"),
        }
    }
}

impl std::error::Error for DateError {}

/// A start and an end date, inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateSpan {
    /// First day of the span.
    pub start: IsoDate,
    /// Last day of the span.
    pub end: IsoDate,
}

/// Read `YYYY-MM-DD` into its parts.
///
/// Deliberately lenient, and identical to the reference implementation: the day is
/// checked to be 1–31 and nothing more, so `2026-02-31` parses. The stricter
/// calendar check lives where a date is used as a date — the `fatura-recibo`
/// parser validates the day against the month, and so does anything that turns a
/// printed Portuguese date into an ISO one.
///
/// # Errors
///
/// [`DateError::Malformed`] when the text is not `YYYY-MM-DD`, and
/// [`DateError::OutOfRange`] when a part is out of range.
pub fn parse_iso(iso: &str) -> Result<DateParts, DateError> {
    let malformed = || DateError::Malformed {
        input: iso.to_owned(),
    };
    let bytes = iso.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(malformed());
    }
    // The four year digits, then two for the month, then two for the day: the
    // separators occupy 4 and 7 and are checked above.
    let mut parts = [0_u32; 3];
    for (slot, index) in [0_usize, 1, 2, 3, 5, 6, 8, 9].into_iter().enumerate() {
        let byte = bytes[index];
        if !byte.is_ascii_digit() {
            return Err(malformed());
        }
        let target = match slot {
            0..=3 => 0,
            4..=5 => 1,
            _ => 2,
        };
        parts[target] = parts[target] * 10 + u32::from(byte - b'0');
    }
    let (year, month, day) = (i64::from(parts[0]), parts[1], parts[2]);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(DateError::OutOfRange {
            input: iso.to_owned(),
        });
    }
    Ok(DateParts { year, month, day })
}

/// Build `YYYY-MM-DD` from its parts, zero-padding as needed.
#[must_use]
pub fn make_iso(year: i64, month: u32, day: u32) -> IsoDate {
    format!("{year:04}-{month:02}-{day:02}")
}

/// How many days the month has. February follows the Gregorian leap rule.
#[must_use]
pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        // Not reachable through the public API: every caller holds a parsed
        // month. Answering 0 keeps the function total without inventing a day.
        _ => 0,
    }
}

/// The Gregorian leap rule.
#[must_use]
pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days since 1970-01-01, the arithmetic every other function is built on.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let month = i64::from(month);
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shift = days + 719_468;
    let era = if shift >= 0 { shift } else { shift - 146_096 } / 146_097;
    let day_of_era = shift - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

/// Shift a date by a whole number of days, in either direction.
///
/// # Errors
///
/// Whatever [`parse_iso`] refuses.
pub fn add_days(iso: &str, days: i64) -> Result<IsoDate, DateError> {
    let parts = parse_iso(iso)?;
    let shifted = days_from_civil(parts.year, parts.month, parts.day) + days;
    let (year, month, day) = civil_from_days(shifted);
    Ok(make_iso(year, month, day))
}

/// Add months, **clamping** the day to the length of the target month.
///
/// `2026-01-31` plus one month is `2026-02-28`, not a March date and not an error.
///
/// # Errors
///
/// Whatever [`parse_iso`] refuses.
pub fn add_months(iso: &str, months: i64) -> Result<IsoDate, DateError> {
    let parts = parse_iso(iso)?;
    let zero_based = i64::from(parts.month) - 1 + months;
    let target_year = parts.year + floor_div(zero_based, 12);
    let target_month = u32::try_from(((zero_based % 12) + 12) % 12 + 1).unwrap_or(1);
    let day = parts.day.min(days_in_month(target_year, target_month));
    Ok(make_iso(target_year, target_month, day))
}

/// Integer division that rounds toward negative infinity, as `Math.floor` does.
fn floor_div(numerator: i64, denominator: i64) -> i64 {
    let quotient = numerator / denominator;
    if numerator % denominator != 0 && (numerator < 0) != (denominator < 0) {
        quotient - 1
    } else {
        quotient
    }
}

/// Day of the week, 0 = Sunday through 6 = Saturday.
///
/// # Errors
///
/// Whatever [`parse_iso`] refuses.
pub fn day_of_week(iso: &str) -> Result<u32, DateError> {
    let parts = parse_iso(iso)?;
    let days = days_from_civil(parts.year, parts.month, parts.day);
    u32::try_from((days + 4).rem_euclid(7)).map_err(|_| DateError::OutOfRange {
        input: iso.to_owned(),
    })
}

/// Whether the date falls on a Saturday or a Sunday.
///
/// # Errors
///
/// Whatever [`parse_iso`] refuses.
pub fn is_weekend(iso: &str) -> Result<bool, DateError> {
    let dow = day_of_week(iso)?;
    Ok(dow == 0 || dow == 6)
}

/// Move a date forward to the next business day.
///
/// AT note (a) on the annual calendar: an obligation falling on a weekend or a
/// public holiday may be met on the following business day. Holidays are **not
/// guessed here** — pass them in, because an application that guesses holidays is
/// an application that misses a deadline.
///
/// # Errors
///
/// Whatever [`parse_iso`] refuses.
pub fn next_business_day(iso: &str, holidays: &[IsoDate]) -> Result<IsoDate, DateError> {
    let mut candidate = iso.to_owned();
    // Ten iterations is far beyond any real Portuguese holiday run (Christmas).
    for _guard in 0..10 {
        if !is_weekend(&candidate)? && !holidays.iter().any(|holiday| holiday == &candidate) {
            return Ok(candidate);
        }
        candidate = add_days(&candidate, 1)?;
    }
    Ok(candidate)
}

/// Compare two ISO dates. ISO strings sort lexicographically, so this is a string
/// comparison and needs no parsing.
#[must_use]
pub fn compare_iso(a: &str, b: &str) -> core::cmp::Ordering {
    a.cmp(b)
}

/// Whole days from `from` to `to`; negative when `to` is in the past.
///
/// # Errors
///
/// Whatever [`parse_iso`] refuses, for either side.
pub fn days_between(from: &str, to: &str) -> Result<i64, DateError> {
    let start = parse_iso(from)?;
    let end = parse_iso(to)?;
    Ok(days_from_civil(end.year, end.month, end.day) - days_from_civil(start.year, start.month, start.day))
}

/// The first and last day of a quarter.
///
/// # Errors
///
/// [`DateError::Quarter`] when the quarter is outside 1–4.
pub fn quarter_range(year: i64, quarter: i64) -> Result<DateSpan, DateError> {
    if !(1..=4).contains(&quarter) {
        return Err(DateError::Quarter { quarter });
    }
    let first_month = u32::try_from((quarter - 1) * 3 + 1).unwrap_or(1);
    let last_month = first_month + 2;
    Ok(DateSpan {
        start: make_iso(year, first_month, 1),
        end: make_iso(year, last_month, days_in_month(year, last_month)),
    })
}

/// The first and last day of a month.
///
/// # Errors
///
/// [`DateError::Month`] when the month is outside 1–12.
pub fn month_range(year: i64, month: i64) -> Result<DateSpan, DateError> {
    if !(1..=12).contains(&month) {
        return Err(DateError::Month { month });
    }
    let month_u32 = u32::try_from(month).unwrap_or(1);
    Ok(DateSpan {
        start: make_iso(year, month_u32, 1),
        end: make_iso(year, month_u32, days_in_month(year, month_u32)),
    })
}

/// Format an ISO date the Portuguese way, `15/03/2026`.
///
/// # Errors
///
/// Whatever [`parse_iso`] refuses.
pub fn format_pt_date(iso: &str) -> Result<String, DateError> {
    let parts = parse_iso(iso)?;
    Ok(format!("{:02}/{:02}/{}", parts.day, parts.month, parts.year))
}

/// Format a month and year as `março de 2026`.
///
/// # Errors
///
/// [`DateError::Month`] when the month is outside 1–12.
pub fn format_pt_month_year(month: i64, year: i64) -> Result<String, DateError> {
    let index = usize::try_from(month - 1).ok().filter(|index| *index < 12);
    let name = index
        .and_then(|index| PT_MONTHS.get(index))
        .ok_or(DateError::Month { month })?;
    Ok(format!("{name} de {year}"))
}

/// The period label an obligation carries, `3.º trimestre 2026`.
#[must_use]
pub fn quarter_label(year: i64, quarter: i64) -> String {
    format!("{quarter}.º trimestre {year}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: &str, end: &str) -> DateSpan {
        DateSpan {
            start: start.to_owned(),
            end: end.to_owned(),
        }
    }

    #[test]
    fn a_well_formed_date_reads_into_its_parts() {
        assert_eq!(
            parse_iso("2026-09-20"),
            Ok(DateParts {
                year: 2026,
                month: 9,
                day: 20
            })
        );
        assert_eq!(
            parse_iso("2024-02-29"),
            Ok(DateParts {
                year: 2024,
                month: 2,
                day: 29
            })
        );
    }

    #[test]
    fn a_malformed_date_is_refused() {
        for input in [
            "",
            "2026-9-20",
            "20260920",
            "2026/09/20",
            "2026-09-2O",
            "20-09-2026",
        ] {
            assert!(
                matches!(parse_iso(input), Err(DateError::Malformed { .. })),
                "{input:?} must be malformed"
            );
        }
    }

    #[test]
    fn the_reference_laxness_is_kept_deliberately() {
        // The reference implementation accepts a day that the month does not
        // have, because it validates the shape and the ranges and leaves the real
        // calendar to the code that turns a printed date into an ISO one. Being
        // stricter here would reject values that work today.
        assert_eq!(
            parse_iso("2026-02-31"),
            Ok(DateParts {
                year: 2026,
                month: 2,
                day: 31
            })
        );
        for input in ["2026-00-10", "2026-13-10", "2026-12-00", "2026-12-32"] {
            assert!(
                matches!(parse_iso(input), Err(DateError::OutOfRange { .. })),
                "{input:?} must be out of range"
            );
        }
    }

    #[test]
    fn month_lengths_follow_the_gregorian_leap_rule() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 12), 31);
        assert_eq!(days_in_month(2026, 4), 30);
    }

    #[test]
    fn adding_days_crosses_months_and_years() {
        assert_eq!(add_days("2026-12-31", 1), Ok("2027-01-01".to_owned()));
        assert_eq!(add_days("2026-01-01", -1), Ok("2025-12-31".to_owned()));
        assert_eq!(add_days("2024-02-28", 1), Ok("2024-02-29".to_owned()));
        assert_eq!(add_days("2026-02-28", 1), Ok("2026-03-01".to_owned()));
        assert_eq!(add_days("2026-09-20", 100), Ok("2026-12-29".to_owned()));
    }

    #[test]
    fn adding_months_clamps_the_day_to_the_target_month() {
        assert_eq!(add_months("2026-01-31", 1), Ok("2026-02-28".to_owned()));
        assert_eq!(add_months("2024-01-31", 1), Ok("2024-02-29".to_owned()));
        assert_eq!(add_months("2026-03-31", -1), Ok("2026-02-28".to_owned()));
        assert_eq!(add_months("2026-12-15", 1), Ok("2027-01-15".to_owned()));
        assert_eq!(add_months("2026-01-15", -1), Ok("2025-12-15".to_owned()));
        assert_eq!(add_months("2026-08-20", 24), Ok("2028-08-20".to_owned()));
        assert_eq!(add_months("2026-08-20", -24), Ok("2024-08-20".to_owned()));
    }

    #[test]
    fn the_weekday_of_a_known_date_is_the_expected_one() {
        // 1970-01-01 was a Thursday; that is the anchor the arithmetic uses.
        assert_eq!(day_of_week("1970-01-01"), Ok(4));
        // 2024-01-01 was a Monday.
        assert_eq!(day_of_week("2024-01-01"), Ok(1));
        // The published 2026 calendar shifts a Saturday deadline to the Monday.
        assert_eq!(day_of_week("2026-06-20"), Ok(6));
        assert_eq!(day_of_week("2026-06-22"), Ok(1));
        assert_eq!(is_weekend("2026-06-20"), Ok(true));
        assert_eq!(is_weekend("2026-06-22"), Ok(false));
    }

    #[test]
    fn weekend_deadlines_move_to_the_next_business_day() {
        assert_eq!(next_business_day("2026-06-20", &[]), Ok("2026-06-22".to_owned()));
        assert_eq!(next_business_day("2026-06-22", &[]), Ok("2026-06-22".to_owned()));
        // A holiday that lands on a Monday pushes to the Tuesday, and is supplied
        // by the caller rather than guessed.
        let holidays = vec!["2026-06-22".to_owned()];
        assert_eq!(
            next_business_day("2026-06-20", &holidays),
            Ok("2026-06-23".to_owned())
        );
    }

    #[test]
    fn day_counts_are_inclusive_of_both_ends_only_by_difference() {
        assert_eq!(days_between("2026-01-01", "2026-12-31"), Ok(364));
        assert_eq!(days_between("2026-12-31", "2026-01-01"), Ok(-364));
        assert_eq!(days_between("2026-09-20", "2026-09-20"), Ok(0));
        assert_eq!(days_between("2024-02-28", "2024-03-01"), Ok(2));
    }

    #[test]
    fn iso_dates_compare_as_strings() {
        assert_eq!(compare_iso("2026-01-01", "2026-01-02"), core::cmp::Ordering::Less);
        assert_eq!(
            compare_iso("2026-02-01", "2026-01-31"),
            core::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_iso("2026-01-01", "2026-01-01"),
            core::cmp::Ordering::Equal
        );
    }

    #[test]
    fn quarters_and_months_span_their_first_and_last_day() {
        assert_eq!(quarter_range(2026, 1), Ok(span("2026-01-01", "2026-03-31")));
        assert_eq!(quarter_range(2026, 2), Ok(span("2026-04-01", "2026-06-30")));
        assert_eq!(quarter_range(2026, 4), Ok(span("2026-10-01", "2026-12-31")));
        assert_eq!(quarter_range(2026, 0), Err(DateError::Quarter { quarter: 0 }));
        assert_eq!(quarter_range(2026, 5), Err(DateError::Quarter { quarter: 5 }));
        assert_eq!(month_range(2026, 2), Ok(span("2026-02-01", "2026-02-28")));
        assert_eq!(month_range(2024, 2), Ok(span("2024-02-01", "2024-02-29")));
        assert_eq!(month_range(2026, 13), Err(DateError::Month { month: 13 }));
    }

    #[test]
    fn display_strings_are_portuguese() {
        assert_eq!(format_pt_date("2026-03-15"), Ok("15/03/2026".to_owned()));
        assert_eq!(format_pt_month_year(3, 2026), Ok("março de 2026".to_owned()));
        assert_eq!(
            format_pt_month_year(13, 2026),
            Err(DateError::Month { month: 13 })
        );
        assert_eq!(quarter_label(2026, 3), "3.º trimestre 2026");
    }

    #[test]
    fn every_day_of_a_decade_round_trips_through_the_civil_arithmetic() {
        // A ten-year sweep: each date must come back from its own day number, and
        // tomorrow must be exactly one day after today.
        let start = days_from_civil(2020, 1, 1);
        let end = days_from_civil(2030, 1, 1);
        for days in start..end {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
            let today = make_iso(year, month, day);
            let tomorrow = add_days(&today, 1).unwrap_or_default();
            assert_eq!(days_between(&today, &tomorrow), Ok(1), "{today} + 1 day");
        }
        assert_eq!(end - start, 3_653, "ten civil years, three of them leaps");
    }
}
