use serde::Serialize;

use crate::captures::{CaptureInterval, CaptureParams, KINDS, MAX_MINUTES};

/// What the number input shows when the stored interval is not an `every_minutes` one: switching
/// the radio lands on a usable interval instead of an empty box.
const DEFAULT_MINUTES: u32 = 15;
/// The same, for the `time` input.
const DEFAULT_AT: &str = "12:00";

/// The Configuration page's Captures section: the interval in effect, and the values its form
/// opens with.
#[derive(Debug, Serialize)]
pub struct CapturesView {
    /// One line describing the stored interval, e.g. `Every hour`.
    pub current: String,
    /// The radio to check — always one of `captures::KINDS`.
    pub kind: String,
    /// The `number` input's value.
    pub minutes: u32,
    /// The `number` input's `min` attribute.
    pub minutes_min: u32,
    /// The `number` input's `max` attribute.
    pub minutes_max: u32,
    /// The `time` input's value, `HH:MM`.
    pub at: String,
}

impl CapturesView {
    /// The section as the page opens it: the stored interval fills the form.
    #[must_use]
    pub fn from_interval(interval: &CaptureInterval) -> Self {
        Self {
            current: interval.describe(),
            kind: interval.kind().to_string(),
            minutes: interval.minutes().unwrap_or(DEFAULT_MINUTES),
            minutes_min: 1,
            minutes_max: MAX_MINUTES,
            at: interval.at().unwrap_or_else(|| DEFAULT_AT.to_string()),
        }
    }

    /// The section after a rejected save: the *stored* interval is still what `current` describes,
    /// and the form keeps what was typed, so the reader edits rather than retypes. A submission
    /// naming no known radio leaves the stored one checked, so the radios always have a selection.
    #[must_use]
    pub fn rejected(stored: &CaptureInterval, params: &CaptureParams) -> Self {
        let mut view = Self::from_interval(stored);

        if KINDS.contains(&params.kind.as_str()) {
            view.kind.clone_from(&params.kind);
        }
        if let Some(minutes) = params
            .minutes
            .as_deref()
            .and_then(|minutes| minutes.trim().parse::<u32>().ok())
        {
            view.minutes = minutes;
        }
        if let Some(at) = params.at.as_deref().filter(|at| !at.trim().is_empty()) {
            view.at = at.to_string();
        }

        view
    }
}
