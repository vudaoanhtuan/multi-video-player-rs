//! Local date and time values.
//!
//! File name times are local times. The app never converts time zones, so a
//! [`Timestamp`] is a plain count of microseconds on a local calendar.

use std::fmt;

/// A duration or an offset in microseconds.
pub type Micros = i64;

pub const MICROS_PER_SEC: Micros = 1_000_000;
pub const MICROS_PER_DAY: Micros = 86_400 * MICROS_PER_SEC;

/// A full local date and time, in microseconds since 1970-01-01 00:00:00.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Timestamp(pub Micros);

impl Timestamp {
    /// Builds a timestamp from calendar fields. Returns `None` for a date or
    /// time that does not exist.
    pub fn from_civil(year: i64, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> Option<Self> {
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        if hour > 23 || min > 59 || sec > 59 {
            return None;
        }
        let days = days_from_civil(year, month, day);
        let secs = days * 86_400 + (hour * 3600 + min * 60 + sec) as i64;
        Some(Self(secs * MICROS_PER_SEC))
    }

    /// Parses the file name format `yyyyMMdd'T'HHmmss`, for example
    /// `20261019T213000`.
    pub fn parse_compact(text: &str) -> Option<Self> {
        let b = text.as_bytes();
        if b.len() != 15 || b[8] != b'T' {
            return None;
        }
        let num = |range: std::ops::Range<usize>| -> Option<u32> {
            let part = &b[range];
            if !part.iter().all(u8::is_ascii_digit) {
                return None;
            }
            Some(part.iter().fold(0u32, |acc, d| acc * 10 + (d - b'0') as u32))
        };
        Self::from_civil(
            num(0..4)? as i64,
            num(4..6)?,
            num(6..8)?,
            num(9..11)?,
            num(11..13)?,
            num(13..15)?,
        )
    }

    pub fn micros(self) -> Micros {
        self.0
    }

    pub fn offset(self, delta: Micros) -> Self {
        Self(self.0 + delta)
    }

    /// Midnight at the start of the day of this timestamp.
    pub fn day_start(self) -> Self {
        Self(self.0 - self.time_of_day())
    }

    /// Microseconds since midnight.
    pub fn time_of_day(self) -> Micros {
        self.0.rem_euclid(MICROS_PER_DAY)
    }

    /// The time of day as `HH:mm:ss`. The date is not shown.
    pub fn format_hms(self) -> String {
        let secs = self.time_of_day() / MICROS_PER_SEC;
        format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let days = self.0.div_euclid(MICROS_PER_DAY);
        let (y, m, d) = civil_from_days(days);
        write!(f, "{y:04}-{m:02}-{d:02}T{}", self.format_hms())
    }
}

/// Parses a time of day typed by the user: `HH:mm:ss` or `HH:mm`.
/// Returns microseconds since midnight.
pub fn parse_time_of_day(text: &str) -> Option<Micros> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return None;
    }
    let mut fields = [0i64; 3];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || part.len() > 2 || !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        fields[i] = part.parse().ok()?;
    }
    let [h, m, s] = fields;
    if h > 23 || m > 59 || s > 59 {
        return None;
    }
    Some((h * 3600 + m * 60 + s) * MICROS_PER_SEC)
}

/// Turns a typed time of day into a jump target inside `start..=end`.
///
/// The time is on the day of `start`. If the range passes midnight, a time
/// before the start time of day is on the next day. A result outside the
/// range is moved to the nearest end of the range.
pub fn resolve_time_of_day(time_of_day: Micros, start: Timestamp, end: Timestamp) -> Timestamp {
    let mut target = start.day_start().offset(time_of_day);
    let passes_midnight = end.day_start() > start.day_start();
    if passes_midnight && time_of_day < start.time_of_day() {
        target = target.offset(MICROS_PER_DAY);
    }
    Timestamp(target.0.clamp(start.0, end.0))
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

/// Days since 1970-01-01 for a calendar date (proleptic Gregorian).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(text: &str) -> Timestamp {
        Timestamp::parse_compact(text).unwrap()
    }

    #[test]
    fn parses_compact_time() {
        let t = ts("20261019T213000");
        assert_eq!(t.format_hms(), "21:30:00");
        assert_eq!(format!("{t:?}"), "2026-10-19T21:30:00");
        assert_eq!(Timestamp::parse_compact("19700101T000000"), Some(Timestamp(0)));
    }

    #[test]
    fn rejects_bad_compact_time() {
        for bad in [
            "20261019213000",
            "20261019T2130",
            "20261319T213000",
            "20260230T213000",
            "20261019T243000",
            "2026101xT213000",
            "20261019t213000",
        ] {
            assert_eq!(Timestamp::parse_compact(bad), None, "{bad}");
        }
    }

    #[test]
    fn keeps_the_date_across_midnight() {
        let a = ts("20261019T235950");
        let b = a.offset(20 * MICROS_PER_SEC);
        assert_eq!(b, ts("20261020T000010"));
        assert_eq!(b.format_hms(), "00:00:10");
    }

    #[test]
    fn leap_day() {
        assert!(Timestamp::parse_compact("20240229T000000").is_some());
        assert!(Timestamp::parse_compact("20260229T000000").is_none());
    }

    #[test]
    fn parses_typed_time() {
        assert_eq!(parse_time_of_day("21:45:10"), Some((21 * 3600 + 45 * 60 + 10) * MICROS_PER_SEC));
        assert_eq!(parse_time_of_day(" 21:45 "), Some((21 * 3600 + 45 * 60) * MICROS_PER_SEC));
        assert_eq!(parse_time_of_day("9:05"), Some((9 * 3600 + 5 * 60) * MICROS_PER_SEC));
        for bad in ["", "21", "24:00", "21:60", "21:45:60", "ab:cd", "21:45:10:00", "21::10", "-1:00"] {
            assert_eq!(parse_time_of_day(bad), None, "{bad}");
        }
    }

    #[test]
    fn typed_time_inside_range() {
        let (start, end) = (ts("20261019T213000"), ts("20261019T233000"));
        let tod = parse_time_of_day("21:45:10").unwrap();
        assert_eq!(resolve_time_of_day(tod, start, end), ts("20261019T214510"));
    }

    #[test]
    fn typed_time_is_limited_to_range() {
        let (start, end) = (ts("20261019T213000"), ts("20261019T233000"));
        let before = parse_time_of_day("08:00").unwrap();
        let after = parse_time_of_day("23:45").unwrap();
        assert_eq!(resolve_time_of_day(before, start, end), start);
        assert_eq!(resolve_time_of_day(after, start, end), end);
    }

    #[test]
    fn typed_time_after_midnight_is_next_day() {
        let (start, end) = (ts("20261019T213000"), ts("20261020T020000"));
        let tod = parse_time_of_day("01:15").unwrap();
        assert_eq!(resolve_time_of_day(tod, start, end), ts("20261020T011500"));
        let late = parse_time_of_day("03:00").unwrap();
        assert_eq!(resolve_time_of_day(late, start, end), end);
        let same_day = parse_time_of_day("22:00").unwrap();
        assert_eq!(resolve_time_of_day(same_day, start, end), ts("20261019T220000"));
    }
}
