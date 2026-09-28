//! The Captures section: how often the camera takes a picture.
//!
//! The shape of the `captures` row's JSON payload and the rules the page's form enforces.
//! `crate::models::app_settings` stores it; `crate::tasks::enqueue_capture` reads the
//! interval to decide when a capture is due.

use chrono::{NaiveTime, Timelike};
use serde::{Deserialize, Serialize};

/// The `section` value the Captures payload is stored under.
pub const SECTION: &str = "captures";

/// The slowest `every_minutes` interval: a day. Anything slower is `DailyAt`.
pub const MAX_MINUTES: u32 = 1440;

/// The form's three radio values, in the order the page shows them. The radios in
/// `assets/views/configuration/index.html` are these, spelled the same way.
pub const KINDS: [&str; 3] = ["every_minutes", "hourly", "daily_at"];

/// The Captures section's configuration. A key added later is a new field with
/// `#[serde(default)]`, so a row written before the key existed still reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureSettings {
    pub interval: CaptureInterval,
}

/// How often the camera takes a picture, stored as `{"kind": …}`.
///
/// `Hourly` is spelled out rather than left as `every_minutes` with 60 minutes: it is one of the
/// three choices the page offers, and a row should say which one the reader picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaptureInterval {
    /// Every `minutes` minutes, 1..=`MAX_MINUTES`.
    EveryMinutes { minutes: u32 },
    /// Once an hour.
    Hourly,
    /// Once a day at `hour`:`minute`, on the server's own clock.
    DailyAt { hour: u8, minute: u8 },
}

impl Default for CaptureInterval {
    /// What an unsaved Captures section runs on: hourly. Cheap on a Pi, frequent enough to be
    /// visibly working.
    fn default() -> Self {
        Self::Hourly
    }
}

impl CaptureInterval {
    /// The form radio this interval is chosen by — always one of [`KINDS`].
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::EveryMinutes { .. } => "every_minutes",
            Self::Hourly => "hourly",
            Self::DailyAt { .. } => "daily_at",
        }
    }

    /// The number input's value, `None` when the interval is not an `every_minutes` one.
    #[must_use]
    pub const fn minutes(&self) -> Option<u32> {
        match *self {
            Self::EveryMinutes { minutes } => Some(minutes),
            _ => None,
        }
    }

    /// The `time` input's value, `None` when the interval is not a `daily_at` one.
    #[must_use]
    pub fn at(&self) -> Option<String> {
        match *self {
            Self::DailyAt { hour, minute } => Some(format!("{hour:02}:{minute:02}")),
            _ => None,
        }
    }

    /// One line for the Captures section, e.g. `Every 5 minutes`.
    #[must_use]
    pub fn describe(&self) -> String {
        match *self {
            Self::EveryMinutes { minutes: 1 } => "Every minute".to_string(),
            Self::EveryMinutes { minutes } => format!("Every {minutes} minutes"),
            Self::Hourly => "Every hour".to_string(),
            Self::DailyAt { hour, minute } => format!("Every day at {hour:02}:{minute:02}"),
        }
    }

    /// Whether a capture is due at `now`, for a heartbeat that ticks once a minute.
    ///
    /// The interval is whole minutes on the server's own clock — the clock the page's
    /// `Every day at 03:00` is written against. Minutes count from midnight, so an interval
    /// that does not divide a day (7 minutes, say) restarts at midnight rather than drifting
    /// across it. A tick that arrives more than a minute late misses its minute; the
    /// scheduler is expected to run once a minute, not once an hour.
    #[must_use]
    pub fn due_at(&self, now: NaiveTime) -> bool {
        let minute_of_day = now.hour() * 60 + now.minute();
        match *self {
            Self::EveryMinutes { minutes } => minute_of_day.is_multiple_of(minutes),
            Self::Hourly => now.minute() == 0,
            Self::DailyAt { hour, minute } => {
                now.hour() == u32::from(hour) && now.minute() == u32::from(minute)
            }
        }
    }
}

/// The Captures section's form, as submitted.
#[derive(Debug, Deserialize)]
pub struct CaptureParams {
    /// The radio: one of [`KINDS`]. Empty when a hand-made POST names no radio.
    #[serde(default)]
    pub kind: String,
    /// The number input — read only when `kind` is `every_minutes`.
    pub minutes: Option<String>,
    /// The `time` input, `HH:MM` — read only when `kind` is `daily_at`.
    pub at: Option<String>,
}

impl CaptureParams {
    /// The interval the form describes.
    ///
    /// The field rules live here, not in the handler, so the page and any future reader reject the
    /// same input with the same words.
    ///
    /// # Errors
    /// The message to show on the page when the submission cannot be read as an interval.
    pub fn interval(&self) -> Result<CaptureInterval, String> {
        match self.kind.as_str() {
            "hourly" => Ok(CaptureInterval::Hourly),
            "every_minutes" => {
                let minutes = self
                    .minutes
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| "Enter a whole number of minutes.".to_string())?;
                if !(1..=MAX_MINUTES).contains(&minutes) {
                    return Err(format!("Minutes must be between 1 and {MAX_MINUTES}."));
                }
                Ok(CaptureInterval::EveryMinutes { minutes })
            }
            "daily_at" => parse_time(self.at.as_deref().unwrap_or_default()),
            _ => Err("Choose how often the camera should capture.".to_string()),
        }
    }
}

/// `HH:MM` on the server's clock → a `daily_at` interval.
fn parse_time(text: &str) -> Result<CaptureInterval, String> {
    const MESSAGE: &str = "Enter a time of day as HH:MM.";
    let (hour, minute) = text
        .trim()
        .split_once(':')
        .ok_or_else(|| MESSAGE.to_string())?;
    let hour = hour.trim().parse::<u8>().map_err(|_| MESSAGE.to_string())?;
    let minute = minute
        .trim()
        .parse::<u8>()
        .map_err(|_| MESSAGE.to_string())?;
    if hour > 23 || minute > 59 {
        return Err(MESSAGE.to_string());
    }
    Ok(CaptureInterval::DailyAt { hour, minute })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A form as the page would submit it.
    fn params(kind: &str, minutes: Option<&str>, at: Option<&str>) -> CaptureParams {
        CaptureParams {
            kind: kind.to_string(),
            minutes: minutes.map(ToString::to_string),
            at: at.map(ToString::to_string),
        }
    }

    #[test]
    fn stored_shape_is_the_on_disk_contract() {
        assert_eq!(
            serde_json::to_string(&CaptureSettings {
                interval: CaptureInterval::Hourly
            })
            .expect("the settings serialize"),
            r#"{"interval":{"kind":"hourly"}}"#
        );
        assert_eq!(
            serde_json::to_string(&CaptureSettings {
                interval: CaptureInterval::EveryMinutes { minutes: 5 }
            })
            .expect("the settings serialize"),
            r#"{"interval":{"kind":"every_minutes","minutes":5}}"#
        );
        assert_eq!(
            serde_json::to_string(&CaptureSettings {
                interval: CaptureInterval::DailyAt { hour: 3, minute: 0 }
            })
            .expect("the settings serialize"),
            r#"{"interval":{"kind":"daily_at","hour":3,"minute":0}}"#
        );
    }

    #[test]
    fn an_unsaved_section_runs_hourly() {
        assert_eq!(CaptureSettings::default().interval, CaptureInterval::Hourly);
    }

    #[test]
    fn every_kind_is_one_of_the_form_radios() {
        for interval in [
            CaptureInterval::EveryMinutes { minutes: 5 },
            CaptureInterval::Hourly,
            CaptureInterval::DailyAt { hour: 3, minute: 0 },
        ] {
            assert!(
                KINDS.contains(&interval.kind()),
                "{} is not a radio value",
                interval.kind()
            );
        }
    }

    #[test]
    fn describe_reads_as_the_page_shows_it() {
        assert_eq!(
            CaptureInterval::EveryMinutes { minutes: 1 }.describe(),
            "Every minute"
        );
        assert_eq!(
            CaptureInterval::EveryMinutes { minutes: 5 }.describe(),
            "Every 5 minutes"
        );
        assert_eq!(CaptureInterval::Hourly.describe(), "Every hour");
        assert_eq!(
            CaptureInterval::DailyAt { hour: 3, minute: 0 }.describe(),
            "Every day at 03:00"
        );
    }

    #[test]
    fn accepts_the_intervals_the_form_offers() {
        assert_eq!(
            params("every_minutes", Some("5"), None)
                .interval()
                .expect("5 minutes is valid"),
            CaptureInterval::EveryMinutes { minutes: 5 }
        );
        assert_eq!(
            params("hourly", None, None)
                .interval()
                .expect("hourly needs no fields"),
            CaptureInterval::Hourly
        );
        assert_eq!(
            params("daily_at", None, Some("03:00"))
                .interval()
                .expect("03:00 is a valid time"),
            CaptureInterval::DailyAt { hour: 3, minute: 0 }
        );
        assert_eq!(
            params("daily_at", None, Some("23:59"))
                .interval()
                .expect("23:59 is a valid time"),
            CaptureInterval::DailyAt {
                hour: 23,
                minute: 59
            }
        );
    }

    #[test]
    fn rejects_what_the_form_cannot_read() {
        assert_eq!(
            params("", Some(""), None)
                .interval()
                .expect_err("no radio is not an interval"),
            "Choose how often the camera should capture."
        );
        for minutes in ["", "abc"] {
            assert_eq!(
                params("every_minutes", Some(minutes), None)
                    .interval()
                    .expect_err("a non-number is not a minute count"),
                "Enter a whole number of minutes."
            );
        }
        for minutes in ["0", "1441"] {
            assert_eq!(
                params("every_minutes", Some(minutes), None)
                    .interval()
                    .expect_err("an out-of-range count is rejected"),
                "Minutes must be between 1 and 1440."
            );
        }
        for at in ["24:00", "03:60", "3"] {
            assert_eq!(
                params("daily_at", None, Some(at))
                    .interval()
                    .expect_err("an invalid time is rejected"),
                "Enter a time of day as HH:MM."
            );
        }
    }

    /// A time of day on the server's clock, for `due_at`.
    fn time(hour: u32, minute: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(hour, minute, 0).expect("a valid time of day")
    }

    #[test]
    fn due_at_fires_on_the_minutes_the_interval_names() {
        let every_5 = CaptureInterval::EveryMinutes { minutes: 5 };
        assert!(every_5.due_at(time(0, 0)));
        assert!(every_5.due_at(time(13, 35)));
        assert!(!every_5.due_at(time(13, 36)));

        // Minutes count from midnight: an interval longer than an hour still lands.
        let every_120 = CaptureInterval::EveryMinutes { minutes: 120 };
        assert!(every_120.due_at(time(0, 0)));
        assert!(every_120.due_at(time(4, 0)));
        assert!(!every_120.due_at(time(1, 0)));

        let hourly = CaptureInterval::Hourly;
        assert!(hourly.due_at(time(9, 0)));
        assert!(!hourly.due_at(time(9, 1)));

        let daily = CaptureInterval::DailyAt {
            hour: 3,
            minute: 30,
        };
        assert!(daily.due_at(time(3, 30)));
        assert!(!daily.due_at(time(3, 31)));
        assert!(!daily.due_at(time(15, 30)));
    }
}
