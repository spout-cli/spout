//! Minimal date formatting — just what the registry's history entries need.
//!
//! We hand-roll Hinnant's civil_from_days and days_from_civil rather than
//! pull in a date crate, since `YYYY-MM-DD` is the only format we need and
//! each algorithm is ~10 lines of proven integer math.

// The parse/diff helpers below ship one commit ahead of their first
// production caller in `commands::prune`; keep them reachable to tests
// without tripping dead-code warnings in the meantime.
#![cfg_attr(not(test), allow(dead_code))]

/// Today's date as `YYYY-MM-DD`.
pub fn today_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days_since_epoch = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days_since_epoch);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse `YYYY-MM-DD` into (year, month, day). Strict: exactly 10 chars,
/// digits in the right slots, dashes at 4 and 7, 1..=12 month, 1..=31 day.
/// Does not validate day-of-month per-month (the registry writes via
/// `today_iso`, so callers only receive valid dates unless someone edited
/// the file by hand).
pub fn parse_iso_date(s: &str) -> Option<(i64, u64, u64)> {
    let bytes = s.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let m: u64 = s.get(5..7)?.parse().ok()?;
    let d: u64 = s.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((y, m, d))
}

/// Whole days between two ISO dates (`to - from`). Negative if `to < from`.
/// Returns `None` if either input fails to parse.
pub fn days_between(from: &str, to: &str) -> Option<i64> {
    let (y1, m1, d1) = parse_iso_date(from)?;
    let (y2, m2, d2) = parse_iso_date(to)?;
    Some(days_from_civil(y2, m2, d2) - days_from_civil(y1, m1, d1))
}

/// Whole days from `from` to today. `None` if `from` doesn't parse.
pub fn days_ago(from: &str) -> Option<i64> {
    days_between(from, &today_iso())
}

/// Convert days-since-epoch to (year, month, day) using Hinnant's algorithm.
/// <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>
pub(crate) fn civil_from_days(days: i64) -> (i64, u64, u64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Inverse of `civil_from_days` — Hinnant's algorithm forward direction.
/// <https://howardhinnant.github.io/date_algorithms.html#days_from_civil>
fn days_from_civil(y: i64, m: u64, d: u64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_from_days_y2k() {
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
    }

    #[test]
    fn civil_from_days_leap_day_2000() {
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn civil_from_days_2026_04_20() {
        assert_eq!(civil_from_days(20_563), (2026, 4, 20));
    }

    #[test]
    fn today_iso_is_well_formed() {
        let s = today_iso();
        assert_eq!(s.len(), 10);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
    }

    #[test]
    fn parse_iso_date_round_trips_civil_values() {
        assert_eq!(parse_iso_date("1970-01-01"), Some((1970, 1, 1)));
        assert_eq!(parse_iso_date("2000-02-29"), Some((2000, 2, 29)));
        assert_eq!(parse_iso_date("2026-04-22"), Some((2026, 4, 22)));
    }

    #[test]
    fn parse_iso_date_rejects_malformed_input() {
        assert!(parse_iso_date("").is_none());
        assert!(parse_iso_date("2026-04").is_none());
        assert!(parse_iso_date("2026-04-2").is_none());
        assert!(parse_iso_date("2026/04/22").is_none());
        assert!(parse_iso_date("abcd-ef-gh").is_none());
    }

    #[test]
    fn parse_iso_date_rejects_out_of_range_month_and_day() {
        assert!(parse_iso_date("2026-13-01").is_none());
        assert!(parse_iso_date("2026-00-01").is_none());
        assert!(parse_iso_date("2026-04-32").is_none());
        assert!(parse_iso_date("2026-04-00").is_none());
    }

    #[test]
    fn days_from_civil_inverts_civil_from_days() {
        for days in [0_i64, 10_957, 11_016, 20_563, -500, 100_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "round-trip at {days}");
        }
    }

    #[test]
    fn days_between_same_date_is_zero() {
        assert_eq!(days_between("2026-04-22", "2026-04-22"), Some(0));
    }

    #[test]
    fn days_between_is_signed() {
        assert_eq!(days_between("2026-04-20", "2026-04-22"), Some(2));
        assert_eq!(days_between("2026-04-22", "2026-04-20"), Some(-2));
    }

    #[test]
    fn days_between_spans_month_and_year_boundaries() {
        assert_eq!(days_between("2025-12-31", "2026-01-01"), Some(1));
        assert_eq!(days_between("2000-02-28", "2000-03-01"), Some(2));
        assert_eq!(days_between("2001-02-28", "2001-03-01"), Some(1));
    }

    #[test]
    fn days_between_malformed_input_is_none() {
        assert!(days_between("not-a-date", "2026-04-22").is_none());
        assert!(days_between("2026-04-22", "bad").is_none());
    }

    #[test]
    fn days_ago_on_today_is_zero() {
        assert_eq!(days_ago(&today_iso()), Some(0));
    }
}
