//! Minimal date formatting — just what the registry's history entries need.
//!
//! We hand-roll civil-from-days (Hinnant's algorithm) rather than pull in a
//! date crate, since `YYYY-MM-DD` is the only format we need and the
//! algorithm is ~15 lines of proven integer math.

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

/// Convert days-since-epoch to (year, month, day) using Hinnant's algorithm.
/// <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>
fn civil_from_days(days: i64) -> (i64, u64, u64) {
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
}
