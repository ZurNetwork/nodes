//! Calendar time without a calendar crate: the `charted` date — a plain `YYYY-MM-DD` day,
//! validated on parse — and the instant a file was last modified.

use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The calendar day a node was last charted, `YYYY-MM-DD`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ChartedDate {
    year: u16,
    month: u8,
    day: u8,
}

/// Why a string is not a `ChartedDate`.
#[derive(Debug, PartialEq, Eq)]
pub enum DateError {
    /// Not shaped `YYYY-MM-DD`.
    Shape(String),
    /// A month outside 1–12.
    Month(u8),
    /// A day outside the month's length.
    Day { month: u8, day: u8 },
}

impl ChartedDate {
    /// A validated calendar day.
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, DateError> {
        if !(1..=12).contains(&month) {
            return Err(DateError::Month(month));
        }
        if day == 0 || day > days_in_month(year, month) {
            return Err(DateError::Day { month, day });
        }
        Ok(Self { year, month, day })
    }

    /// Today in UTC, from the system clock.
    pub fn today_utc() -> Self {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the system clock is after 1970");
        let days = i64::try_from(since_epoch.as_secs() / 86_400).expect("fits in i64");
        Self::from_days_since_epoch(days)
    }

    /// The civil date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
    pub fn from_days_since_epoch(days: i64) -> Self {
        let shifted = days + 719_468;
        let era = if shifted >= 0 {
            shifted
        } else {
            shifted - 146_096
        } / 146_097;
        let day_of_era = shifted - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted_month = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
        let month = if shifted_month < 10 {
            shifted_month + 3
        } else {
            shifted_month - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Self {
            year: u16::try_from(year).expect("a year within u16"),
            month: u8::try_from(month).expect("a month within u8"),
            day: u8::try_from(day).expect("a day within u8"),
        }
    }
}

/// The instant a file was last modified, to the second. Printed RFC 3339 in UTC
/// (`2026-09-20T14:03:11Z`) and ordered by time, so a listing sorts by what it prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(into = "String")]
pub struct ModifiedTime {
    seconds_since_epoch: i64,
}

/// A clock reading outside the years 0001–9999: no calendar day to print it as.
#[derive(Debug, PartialEq, Eq)]
pub struct TimeOutOfRange;

impl ModifiedTime {
    /// 0001-01-01T00:00:00Z.
    const EARLIEST: i64 = -62_135_596_800;
    /// 9999-12-31T23:59:59Z.
    const LATEST: i64 = 253_402_300_799;
}

impl TryFrom<SystemTime> for ModifiedTime {
    type Error = TimeOutOfRange;

    /// A file system may hand back any 64-bit reading; only one a calendar can print is kept.
    /// The fraction of a second is dropped by rounding down, on both sides of the epoch: a
    /// millisecond before 1970 is 23:59:59, not midnight.
    fn try_from(time: SystemTime) -> Result<Self, TimeOutOfRange> {
        let seconds_since_epoch = match time.duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_secs()),
            Err(before) => {
                let until_epoch = before.duration();
                let started_second = u64::from(until_epoch.subsec_nanos() > 0);
                let whole_seconds = until_epoch.as_secs().saturating_add(started_second);
                i64::try_from(whole_seconds).map(|seconds| -seconds)
            }
        }
        .map_err(|_| TimeOutOfRange)?;
        if !(Self::EARLIEST..=Self::LATEST).contains(&seconds_since_epoch) {
            return Err(TimeOutOfRange);
        }
        Ok(Self {
            seconds_since_epoch,
        })
    }
}

impl From<ModifiedTime> for String {
    fn from(time: ModifiedTime) -> Self {
        time.to_string()
    }
}

impl fmt::Display for ModifiedTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let day = ChartedDate::from_days_since_epoch(self.seconds_since_epoch.div_euclid(86_400));
        let second_of_day = self.seconds_since_epoch.rem_euclid(86_400);
        let hour = second_of_day / 3_600;
        let minute = second_of_day % 3_600 / 60;
        let second = second_of_day % 60;
        write!(f, "{day}T{hour:02}:{minute:02}:{second:02}Z")
    }
}

impl fmt::Display for TimeOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a time outside the years 0001–9999")
    }
}

impl std::error::Error for TimeOutOfRange {}

fn days_in_month(year: u16, month: u8) -> u8 {
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        _ => 28,
    }
}

impl FromStr for ChartedDate {
    type Err = DateError;

    fn from_str(text: &str) -> Result<Self, DateError> {
        let shape_error = || DateError::Shape(text.to_owned());
        let bytes = text.as_bytes();
        let shaped = bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit());
        if !shaped {
            return Err(shape_error());
        }
        let year = text[0..4].parse::<u16>().map_err(|_| shape_error())?;
        let month = text[5..7].parse::<u8>().map_err(|_| shape_error())?;
        let day = text[8..10].parse::<u8>().map_err(|_| shape_error())?;
        Self::new(year, month, day)
    }
}

impl TryFrom<&str> for ChartedDate {
    type Error = DateError;

    fn try_from(text: &str) -> Result<Self, DateError> {
        text.parse()
    }
}

impl TryFrom<String> for ChartedDate {
    type Error = DateError;

    fn try_from(text: String) -> Result<Self, DateError> {
        text.parse()
    }
}

impl From<ChartedDate> for String {
    fn from(date: ChartedDate) -> Self {
        date.to_string()
    }
}

impl fmt::Display for ChartedDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape(text) => write!(f, "`{text}` is not a YYYY-MM-DD date"),
            Self::Month(month) => write!(f, "month {month} is not 01–12"),
            Self::Day { month, day } => write!(f, "day {day} does not exist in month {month}"),
        }
    }
}

impl std::error::Error for DateError {}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn parses_and_prints_a_date() {
        let date: ChartedDate = "2026-09-12".parse().expect("valid");
        assert_eq!(date.to_string(), "2026-09-12");
    }

    #[test]
    fn rejects_bad_shapes_and_bad_days() {
        let bad_shape = "2026-9-12".parse::<ChartedDate>();
        assert_eq!(bad_shape, Err(DateError::Shape("2026-9-12".to_owned())));
        let bad_month = "2026-13-01".parse::<ChartedDate>();
        assert_eq!(bad_month, Err(DateError::Month(13)));
        let bad_day = "2025-02-29".parse::<ChartedDate>();
        assert_eq!(bad_day, Err(DateError::Day { month: 2, day: 29 }));
        let leap_day = "2024-02-29".parse::<ChartedDate>();
        assert!(leap_day.is_ok());
    }

    #[test]
    fn counts_days_since_the_epoch() {
        let epoch = ChartedDate::from_days_since_epoch(0);
        assert_eq!(epoch.to_string(), "1970-01-01");
        let leap_day_2000 = ChartedDate::from_days_since_epoch(11_016);
        assert_eq!(leap_day_2000.to_string(), "2000-02-29");
        let september_2026 = ChartedDate::from_days_since_epoch(20_709);
        assert_eq!(september_2026.to_string(), "2026-09-13");
    }

    #[test]
    fn a_modified_time_prints_rfc_3339_in_utc_and_refuses_what_no_calendar_holds() {
        let printed = |time: SystemTime| ModifiedTime::try_from(time).map(|time| time.to_string());
        let after = |seconds: u64| UNIX_EPOCH + Duration::from_secs(seconds);
        assert_eq!(printed(after(0)), Ok("1970-01-01T00:00:00Z".to_owned()));
        assert_eq!(
            printed(after(1_789_912_991)),
            Ok("2026-09-20T14:03:11Z".to_owned())
        );
        assert_eq!(
            printed(after(951_868_799)),
            Ok("2000-02-29T23:59:59Z".to_owned())
        );
        let a_second_before = UNIX_EPOCH - Duration::from_secs(1);
        assert_eq!(
            printed(a_second_before),
            Ok("1969-12-31T23:59:59Z".to_owned())
        );
        let a_millisecond_before = UNIX_EPOCH - Duration::from_millis(1);
        assert_eq!(
            printed(a_millisecond_before),
            Ok("1969-12-31T23:59:59Z".to_owned()),
            "a fraction rounds down before the epoch too, never towards it"
        );
        let almost_two_seconds_after = UNIX_EPOCH + Duration::from_millis(1_999);
        assert_eq!(
            printed(almost_two_seconds_after),
            Ok("1970-01-01T00:00:01Z".to_owned())
        );
        assert_eq!(
            printed(after(253_402_300_799)),
            Ok("9999-12-31T23:59:59Z".to_owned())
        );
        assert_eq!(printed(after(253_402_300_800)), Err(TimeOutOfRange));
        let earlier = ModifiedTime::try_from(after(10)).expect("in range");
        let later = ModifiedTime::try_from(after(11)).expect("in range");
        assert!(earlier < later, "ordered by time");
    }
}
