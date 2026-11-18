//! UTC timestamps without a date crate.
//!
//! Filenames must sort chronologically and reports must be reproducible, so the
//! format is fixed: `YYYYMMDD-HHMMSS` for files, RFC 3339 for `generated_at`.

use std::time::{SystemTime, UNIX_EPOCH};

/// `20261117-223344`
pub fn file_stamp() -> String {
    let (year, month, day, hour, minute, second) = now_parts();
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

/// `2026-11-17T22:33:44Z`
pub fn rfc3339() -> String {
    let (year, month, day, hour, minute, second) = now_parts();
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn now_parts() -> (i64, u32, u32, u32, u32, u32) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64;
    let remainder = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = (remainder / 3_600) as u32;
    let minute = ((remainder % 3_600) / 60) as u32;
    let second = (remainder % 60) as u32;
    (year, month, day, hour, minute, second)
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a proleptic
/// Gregorian date. Valid for the whole range this tool will ever run in.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn known_dates_round_trip() {
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // 2026-11-17: 2000-01-01 + 9_817 days (26 years incl. 6 leap days).
        assert_eq!(civil_from_days(10_957 + 9_817), (2026, 11, 17));
    }

    #[test]
    fn stamps_have_the_documented_shape() {
        let stamp = file_stamp();
        assert_eq!(stamp.len(), 15);
        assert_eq!(&stamp[8..9], "-");
        assert!(stamp[..8].chars().all(|c| c.is_ascii_digit()));

        let rfc = rfc3339();
        assert!(rfc.ends_with('Z'));
        assert_eq!(&rfc[4..5], "-");
        assert_eq!(&rfc[10..11], "T");
    }
}
