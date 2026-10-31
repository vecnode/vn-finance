//! The clock, as far as a record is concerned.
//!
//! The core crate never reads a clock: every function that depends on the date takes it
//! as a parameter, which is what makes the obligations reproducible in a test. This
//! module is the one place in the storage layer that asks the machine what time it is,
//! and it exists because an audit line and a completion record have to carry a
//! timestamp whether or not anybody passed one in.
//!
//! The shape is the one `Date.toISOString()` produces — `2026-09-20T10:03:44.882Z` — so
//! a line written by either implementation reads the same.

/// The current instant as `YYYY-MM-DDTHH:MM:SS.mmmZ`, in UTC.
#[must_use]
pub fn now_iso8601() -> String {
    iso8601(now_epoch_millis())
}

/// Format an instant, given as milliseconds since the Unix epoch.
///
/// Separated from [`now_iso8601`] so a test can pin the instant instead of racing the
/// wall clock.
#[must_use]
pub fn iso8601(epoch_millis: i64) -> String {
    let seconds = epoch_millis.div_euclid(1_000);
    let date = iso_date(seconds);
    let seconds_of_day = seconds.rem_euclid(86_400);
    format!(
        "{date}T{:02}:{:02}:{:02}.{:03}Z",
        seconds_of_day / 3600,
        (seconds_of_day % 3600) / 60,
        seconds_of_day % 60,
        epoch_millis.rem_euclid(1_000)
    )
}

/// Milliseconds since the Unix epoch.
///
/// A clock set before 1970 is not a reason to refuse to open a vault; it is a reason to
/// write an absurd date, which the person will notice.
#[must_use]
pub fn now_epoch_millis() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(elapsed) => i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

/// The ISO date for a number of seconds since the epoch.
fn iso_date(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    vnf_core::dates::add_days("1970-01-01", days).unwrap_or_else(|_| "1970-01-01".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The epoch day of a date, for building instants in a test.
    fn at(iso: &str, hour: i64, minute: i64, second: i64) -> i64 {
        let days = vnf_core::dates::days_between("1970-01-01", iso).unwrap_or(0);
        days * 86_400 + hour * 3_600 + minute * 60 + second
    }

    #[test]
    fn a_timestamp_has_the_shape_javascript_produces() {
        let millis = at("2026-09-20", 10, 3, 44) * 1_000 + 882;
        assert_eq!(iso8601(millis), "2026-09-20T10:03:44.882Z");
    }

    #[test]
    fn the_milliseconds_are_three_digits() {
        let millis = at("2026-01-01", 0, 0, 0) * 1_000 + 7;
        assert_eq!(iso8601(millis), "2026-01-01T00:00:00.007Z");
        let millis = at("2026-01-01", 0, 0, 0) * 1_000 + 990;
        assert_eq!(iso8601(millis), "2026-01-01T00:00:00.990Z");
    }

    #[test]
    fn midnight_and_the_end_of_a_day_are_written_whole() {
        assert_eq!(
            iso8601(at("2026-03-01", 0, 0, 0) * 1_000),
            "2026-03-01T00:00:00.000Z"
        );
        assert_eq!(
            iso8601(at("2026-03-01", 23, 59, 59) * 1_000),
            "2026-03-01T23:59:59.000Z"
        );
    }

    #[test]
    fn a_real_instant_is_well_formed() {
        let now = now_iso8601();
        assert_eq!(now.len(), 24, "{now}");
        assert!(now.ends_with('Z'));
        assert!(vnf_core::dates::parse_iso(&now[..10]).is_ok());
        // And it sorts as a string for any instant in this century, which is what the
        // ledger relies on when it reads lines back.
        assert!(now.starts_with("20"));
    }
}
