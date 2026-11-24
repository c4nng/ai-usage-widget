//! Minimal UTC date math, enough for RFC 3339 timestamps and month boundaries,
//! without pulling a date library into the core crate.

const SECS_PER_DAY: i64 = 86_400;

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of [`days_from_civil`].
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)` into Unix seconds.
/// A missing offset is treated as UTC.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.trim().as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't' | b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> { std::str::from_utf8(&b[from..to]).ok()?.parse().ok() };
    let (year, month, day) = (num(0, 4)?, num(5, 7)? as u32, num(8, 10)? as u32);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    let mut i = 19;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    let offset = match b.get(i) {
        None | Some(b'Z') | Some(b'z') => 0,
        Some(sign @ (b'+' | b'-')) => {
            if b.len() < i + 6 || b[i + 3] != b':' {
                return None;
            }
            let secs = num(i + 1, i + 3)? * 3600 + num(i + 4, i + 6)? * 60;
            if *sign == b'+' {
                secs
            } else {
                -secs
            }
        }
        Some(_) => return None,
    };

    Some(days_from_civil(year, month, day) * SECS_PER_DAY + hour * 3600 + minute * 60 + second - offset)
}

/// Unix seconds → `YYYY-MM-DDTHH:MM:SSZ`.
pub fn format_rfc3339(unix: i64) -> String {
    let (year, month, day) = civil_from_days(unix.div_euclid(SECS_PER_DAY));
    let secs = unix.rem_euclid(SECS_PER_DAY);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Start of the current UTC month and start of the next one, as Unix seconds.
pub fn month_bounds(now: i64) -> (i64, i64) {
    let (year, month, _) = civil_from_days(now.div_euclid(SECS_PER_DAY));
    let start = days_from_civil(year, month, 1) * SECS_PER_DAY;
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    (start, days_from_civil(ny, nm, 1) * SECS_PER_DAY)
}

/// Accepts a timestamp as Unix seconds, Unix milliseconds, or an RFC 3339 string.
pub fn parse_any(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => {
            let x = n.as_f64()?;
            // Anything past year ~5138 in seconds is really milliseconds.
            Some(if x > 1e11 { (x / 1000.0) as i64 } else { x as i64 })
        }
        serde_json::Value::String(s) => {
            parse_rfc3339(s).or_else(|| s.parse::<f64>().ok().and_then(|x| parse_any(&x.into())))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip() {
        for days in [-800_000, -1, 0, 1, 11_016, 20_354, 1_000_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }

    #[test]
    fn parses_rfc3339_variants() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2025-09-23T15:00:00Z"), Some(1_758_639_600));
        assert_eq!(parse_rfc3339("2025-09-23T15:00:00.123456+00:00"), Some(1_758_639_600));
        assert_eq!(parse_rfc3339("2025-09-23T18:00:00+03:00"), Some(1_758_639_600));
        assert_eq!(parse_rfc3339("2025-09-23T10:30:00-04:30"), Some(1_758_639_600));
        assert_eq!(parse_rfc3339("2025-09-23T15:00:00"), Some(1_758_639_600));
        assert_eq!(parse_rfc3339("not a date"), None);
        assert_eq!(parse_rfc3339("2025-13-01T00:00:00Z"), None);
    }

    #[test]
    fn formats_rfc3339() {
        assert_eq!(format_rfc3339(1_758_639_600), "2025-09-23T15:00:00Z");
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn month_bounds_handle_year_end() {
        let now = parse_rfc3339("2025-12-31T23:59:59Z").unwrap();
        let (start, next) = month_bounds(now);
        assert_eq!(format_rfc3339(start), "2025-12-01T00:00:00Z");
        assert_eq!(format_rfc3339(next), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn parse_any_accepts_seconds_millis_and_strings() {
        assert_eq!(parse_any(&serde_json::json!(1_758_639_600)), Some(1_758_639_600));
        assert_eq!(parse_any(&serde_json::json!(1_758_639_600_000i64)), Some(1_758_639_600));
        assert_eq!(parse_any(&serde_json::json!("2025-09-23T15:00:00Z")), Some(1_758_639_600));
        assert_eq!(parse_any(&serde_json::json!(null)), None);
    }
}
