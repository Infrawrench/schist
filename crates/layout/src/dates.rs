//! Civil dates for date text variables, and the date format codes InDesign
//! writes in DateVariablePreference Format (Unicode-style patterns such as
//! `MMMM d, yyyy`). Names are English, as in the public sample.
use serde::{Deserialize, Serialize};

/// A date and wall-clock time, as written; no time zone conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DateTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

impl DateTime {
    pub fn new(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Option<Self> {
        let out = Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        };
        out.valid().then_some(out)
    }

    fn valid(&self) -> bool {
        (1..=12).contains(&self.month)
            && self.day >= 1
            && self.day <= days_in_month(self.year, self.month)
            && self.hour < 24
            && self.minute < 60
            && self.second < 60
    }

    /// An ISO 8601 date as XMP writes it: `YYYY`, `YYYY-MM`, `YYYY-MM-DD`, or
    /// with `THH:MM[:SS[.s]]` and an optional zone, which is ignored: the
    /// wall-clock time is kept as written.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (date, time) = text.split_once('T').unwrap_or((text, ""));
        let mut parts = date.split('-');
        let year: i32 = parts.next()?.parse().ok()?;
        let month: u8 = parts.next().map_or(Some(1), |m| m.parse().ok())?;
        let day: u8 = parts.next().map_or(Some(1), |d| d.parse().ok())?;
        if parts.next().is_some() {
            return None;
        }
        let time = time
            .split(['Z', '+'])
            .next()
            .unwrap_or_default()
            .split('-')
            .next()
            .unwrap_or_default();
        let mut clock = time.split(':');
        let hour: u8 = clock
            .next()
            .filter(|h| !h.is_empty())
            .map_or(Some(0), |h| h.parse().ok())?;
        let minute: u8 = clock.next().map_or(Some(0), |m| m.parse().ok())?;
        let second: u8 = clock
            .next()
            .map_or(Some(0), |s| s.split('.').next()?.parse().ok())?;
        Self::new(year, month, day, hour, minute, second)
    }

    /// From seconds since 1970-01-01T00:00:00 in the zone the caller means.
    pub fn from_unix(seconds: i64) -> Self {
        let days = seconds.div_euclid(86_400);
        let rest = seconds.rem_euclid(86_400);
        // Howard Hinnant's civil-from-days.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Self {
            year,
            month,
            day,
            hour: (rest / 3600) as u8,
            minute: (rest % 3600 / 60) as u8,
            second: (rest % 60) as u8,
        }
    }

    /// 0 for Sunday.
    pub fn weekday(&self) -> usize {
        // Sakamoto's method.
        const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let month = usize::from(self.month);
        let y = if month < 3 { self.year - 1 } else { self.year };
        (y + y.div_euclid(4) - y.div_euclid(100)
            + y.div_euclid(400)
            + T[month - 1]
            + i32::from(self.day))
        .rem_euclid(7) as usize
    }

    /// ISO 8601 with seconds and no zone, as XMP accepts.
    pub fn iso(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// `pattern` with each run of a format letter replaced: y or Y (two of
    /// them give two digits, otherwise the full year; InDesign's own default
    /// "Output Date and Time" writes YYYY), M (1–2 digits, MMM short and MMMM full
    /// names), d, E (EEE short and EEEE full weekday), h and H (12 and 24
    /// hours), m, s, a (AM or PM) and G (AD). Text in single quotes is literal,
    /// '' is a quote, and every other character is kept.
    pub fn format(&self, pattern: &str) -> String {
        let chars: Vec<char> = pattern.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\'' {
                if chars.get(i + 1) == Some(&'\'') {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        if chars.get(i + 1) == Some(&'\'') {
                            out.push('\'');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
                continue;
            }
            let mut run = 1;
            while chars.get(i + run) == Some(&c) {
                run += 1;
            }
            let two = |v: u8| {
                if run >= 2 {
                    format!("{v:02}")
                } else {
                    v.to_string()
                }
            };
            let hour12 = match self.hour % 12 {
                0 => 12,
                h => h,
            };
            match c {
                'y' | 'Y' if run == 2 => out.push_str(&format!("{:02}", self.year.rem_euclid(100))),
                'y' | 'Y' => out.push_str(&format!("{:0width$}", self.year, width = run.min(4))),
                'M' if run >= 4 => out.push_str(MONTHS[usize::from(self.month) - 1]),
                'M' if run == 3 => out.push_str(&MONTHS[usize::from(self.month) - 1][..3]),
                'M' => out.push_str(&two(self.month)),
                'd' => out.push_str(&two(self.day)),
                'E' if run >= 4 => out.push_str(DAYS[self.weekday()]),
                'E' => out.push_str(&DAYS[self.weekday()][..3]),
                'h' => out.push_str(&two(hour12)),
                'H' => out.push_str(&two(self.hour)),
                'm' => out.push_str(&two(self.minute)),
                's' => out.push_str(&two(self.second)),
                'a' => out.push_str(if self.hour < 12 { "AM" } else { "PM" }),
                'G' => out.push_str("AD"),
                _ => {
                    for _ in 0..run {
                        out.push(c);
                    }
                }
            }
            i += run;
        }
        out
    }
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
