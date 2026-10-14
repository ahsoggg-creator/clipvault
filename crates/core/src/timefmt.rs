//! Calendar helpers without a date-time dependency (UTC only).

/// `(year, month, day, hour, minute, second)` of a Unix time.
pub fn civil(unix: u64) -> (i64, u32, u32, u32, u32, u32) {
    let (days, rem) = (unix / 86_400, unix % 86_400);
    // civil-from-days (Howard Hinnant)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

/// `2026-10-13 18:04:09` (UTC).
pub fn format_utc(unix: u64) -> String {
    let (y, m, d, h, mi, s) = civil(unix);
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// `20261013-180409`, safe in file names.
pub fn file_stamp(unix: u64) -> String {
    let (y, m, d, h, mi, s) = civil(unix);
    format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(1_760_000_000), "2025-10-09 08:53:20");
        assert_eq!(format_utc(951_782_400 + 86_399), "2000-02-29 23:59:59");
        assert_eq!(file_stamp(1_760_000_000), "20251009-085320");
    }
}
