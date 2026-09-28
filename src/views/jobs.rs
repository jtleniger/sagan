//! The `/jobs` page's view objects.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::models::job_runs;

/// Everything `assets/views/jobs/index.html` reads beyond the shell's `user`/`active`.
#[derive(Debug, Serialize)]
pub struct JobsView {
    /// One row per registered job: what it is, when it runs, and what last happened.
    pub jobs: Vec<JobSummaryView>,
    /// The page of `job_runs`, newest first.
    pub runs: Vec<RunRowView>,
    pub page: u64,
    pub total_pages: u64,
    pub total_items: u64,
    pub prev_url: Option<String>,
    pub next_url: Option<String>,
    pub first_url: Option<String>,
    pub last_url: Option<String>,
}

/// One registered job, as the page's summary table shows it.
#[derive(Debug, Serialize)]
pub struct JobSummaryView {
    /// The stable job name, as `job_runs.job` stores it.
    pub name: String,
    /// Why the job exists, one line.
    pub detail: String,
    /// The cadence as configured right now, e.g. `Every 5 minutes`.
    pub interval: String,
    /// `never` before the first run, else the newest run's status.
    pub last_status: String,
    /// The badge colour for that status.
    pub last_status_css: &'static str,
    /// When the newest run started, ISO for `local-time.js`; empty when there is none.
    pub last_utc: String,
    /// The ascii fallback for the same instant.
    pub last_label: String,
    /// How long ago the newest run started; empty when there is none.
    pub last_age: String,
    /// The next slot, ISO; empty when the cadence has no next slot.
    pub next_utc: String,
    /// The ascii fallback for the same instant.
    pub next_label: String,
    /// The newest run belongs to an older slot than the one due now — the job is behind.
    pub overdue: bool,
}

impl JobSummaryView {
    /// The row for one job, from what the job and its newest run say.
    ///
    /// `latest_slot` is the slot that is due now, so `overdue` is the due rule itself: a job
    /// whose newest run is at an older slot (or which has never run) has missed one.
    #[must_use]
    pub fn new(
        name: &str,
        detail: &str,
        interval: String,
        latest_slot: Option<DateTime<Utc>>,
        next_slot: Option<DateTime<Utc>>,
        last: Option<&job_runs::Model>,
        now: DateTime<Utc>,
    ) -> Self {
        let (last_status, last_utc, last_label, last_age) = last.map_or_else(
            || {
                (
                    "never".to_string(),
                    String::new(),
                    String::new(),
                    String::new(),
                )
            },
            |run| {
                let started = run.started_at.with_timezone(&Utc);
                (
                    run.status.clone(),
                    iso(Some(started)),
                    utc_label(Some(started)),
                    duration_label(run.age(now).num_milliseconds()),
                )
            },
        );

        let overdue = latest_slot
            .is_some_and(|slot| last.is_none_or(|run| run.slot_at.with_timezone(&Utc) < slot));

        Self {
            name: name.to_string(),
            detail: detail.to_string(),
            interval,
            last_status_css: status_css(&last_status),
            last_status,
            last_utc,
            last_label,
            last_age,
            next_utc: iso(next_slot),
            next_label: utc_label(next_slot),
            overdue,
        }
    }
}

/// One row of the page's run history.
#[derive(Debug, Serialize)]
pub struct RunRowView {
    /// The job the run belongs to.
    pub job: String,
    /// `running`, `succeeded` or `failed`.
    pub status: String,
    /// The badge colour for that status.
    pub status_css: &'static str,
    /// The schedule slot the run belongs to, ISO.
    pub slot_utc: String,
    /// The ascii fallback for the same instant.
    pub slot_label: String,
    /// When the run started, ISO.
    pub started_utc: String,
    /// The ascii fallback for the same instant.
    pub started_label: String,
    /// How long it took, or has been taking.
    pub elapsed_label: String,
    /// The page/log detail: what the run produced, or the error.
    pub detail: String,
}

impl RunRowView {
    /// The row for `run`, measured as of `now` while it is still running.
    #[must_use]
    pub fn new(run: &job_runs::Model, now: DateTime<Utc>) -> Self {
        let started = run.started_at.with_timezone(&Utc);
        let slot = run.slot_at.with_timezone(&Utc);
        let elapsed = run.finished_at.map_or_else(
            || run.age(now).num_milliseconds(),
            |finished| (finished.with_timezone(&Utc) - started).num_milliseconds(),
        );

        Self {
            job: run.job.clone(),
            status: run.status.clone(),
            status_css: status_css(&run.status),
            slot_utc: iso(Some(slot)),
            slot_label: utc_label(Some(slot)),
            started_utc: iso(Some(started)),
            started_label: utc_label(Some(started)),
            elapsed_label: duration_label(elapsed),
            detail: run.detail.clone().unwrap_or_default(),
        }
    }
}

/// The badge colour for a run's status. One colour per state, so a table scans by colour.
///
/// `never` — a job with no runs — and any status a later change adds read as neutral grey
/// rather than failing to render the page.
#[must_use]
pub fn status_css(status: &str) -> &'static str {
    match status {
        job_runs::RUNNING => "bg-sky-100 text-sky-800",
        job_runs::SUCCEEDED => "bg-emerald-100 text-emerald-800",
        job_runs::FAILED => "bg-red-100 text-red-800",
        _ => "bg-slate-100 text-slate-700",
    }
}

/// `at` as an ISO instant for `local-time.js`, empty when there is no such instant.
fn iso(at: Option<DateTime<Utc>>) -> String {
    at.map(|at| at.to_rfc3339()).unwrap_or_default()
}

/// The fallback text under a `<time>`: the same instant, UTC, in the shape the logs page uses.
fn utc_label(at: Option<DateTime<Utc>>) -> String {
    at.map(|at| at.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_default()
}

/// A duration for a table cell: milliseconds, seconds, or minutes and seconds.
#[must_use]
pub fn duration_label(milliseconds: i64) -> String {
    let milliseconds = milliseconds.max(0);
    if milliseconds < 1_000 {
        return format!("{milliseconds} ms");
    }
    let seconds = milliseconds / 1_000;
    if seconds < 60 {
        return format!("{}.{} s", seconds, (milliseconds % 1_000) / 100);
    }
    format!("{}m {:02}s", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_switch_unit_where_the_reader_needs_them_to() {
        assert_eq!(duration_label(0), "0 ms");
        assert_eq!(duration_label(320), "320 ms");
        assert_eq!(duration_label(1_000), "1.0 s");
        assert_eq!(duration_label(1_540), "1.5 s");
        assert_eq!(duration_label(59_999), "59.9 s");
        assert_eq!(duration_label(60_000), "1m 00s");
        assert_eq!(duration_label(125_000), "2m 05s");
        // A clock that went backwards is not a negative duration on the page.
        assert_eq!(duration_label(-5), "0 ms");
    }

    #[test]
    fn every_run_status_gets_its_own_badge_colour() {
        let css = [
            status_css(job_runs::RUNNING),
            status_css(job_runs::SUCCEEDED),
            status_css(job_runs::FAILED),
        ];
        for (index, colour) in css.iter().enumerate() {
            assert!(
                !css[index + 1..].contains(colour),
                "{colour} is used twice, so the badge stops distinguishing states"
            );
        }
        // An unknown status and a job that never ran are both neutral, not a failure.
        assert_eq!(status_css("never"), status_css("something-new"));
    }
}
