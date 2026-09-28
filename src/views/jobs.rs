//! The `/jobs` page's view objects.

use chrono::{DateTime, Utc};
use loco_rs::{
    bgworker::{Job, JobStatus},
    config::WorkerMode,
    scheduler,
};
use serde::Serialize;

use crate::{
    models::runtime_heartbeats::{self, Liveness, Source},
    workers::WorkerEntry,
};

/// The live queue provider, as `controllers::jobs` read it.
#[derive(Debug)]
pub struct ProviderStatus {
    /// The provider's own `describe()` text, e.g. `sqlite queue`.
    pub name: String,
    /// `Ok(())` when `ping()` answered; the failure message when it did not.
    pub ping: Result<(), String>,
}

/// The queue the workers run against, as the page shows it.
#[derive(Debug, Serialize)]
pub struct QueueView {
    /// The configured mode, spelled the way `workers.mode` is spelled in `config/<env>.yaml`.
    pub mode: String,
    /// What that mode means for a job once it has been enqueued.
    pub mode_detail: String,
    /// The provider's own description, or the note that this mode has no provider.
    pub provider: String,
    /// Whether the provider answered `ping`; `null` when there is no provider to ask.
    pub healthy: Option<bool>,
    /// The line under the provider: the ping's failure, or that it answered. Empty when there
    /// is no provider, which is what hides the line.
    pub health_detail: String,
}

impl QueueView {
    /// The card for `mode`, with `provider` as the running provider read it.
    #[must_use]
    pub fn new(mode: &WorkerMode, provider: Option<ProviderStatus>) -> Self {
        let (mode, mode_detail) = match mode {
            WorkerMode::BackgroundQueue => (
                "BackgroundQueue".to_string(),
                "Jobs are stored in the queue and run by the worker process, so they outlive \
                 the request."
                    .to_string(),
            ),
            WorkerMode::ForegroundBlocking => (
                "ForegroundBlocking".to_string(),
                "Jobs run inline, in the process that enqueues them, before the call returns."
                    .to_string(),
            ),
            WorkerMode::BackgroundAsync => (
                "BackgroundAsync".to_string(),
                "Jobs run as detached tasks in the process that enqueues them; nothing is \
                 persisted."
                    .to_string(),
            ),
        };

        let (provider, healthy, health_detail) = match provider {
            Some(ProviderStatus { name, ping }) => match ping {
                Ok(()) => (name, Some(true), "Reachable.".to_string()),
                Err(err) => (name, Some(false), err),
            },
            None => (
                format!("None configured for {mode}; this mode keeps no queue."),
                None,
                String::new(),
            ),
        };

        Self {
            mode,
            mode_detail,
            provider,
            healthy,
            health_detail,
        }
    }
}

/// One worker row in the page's table.
#[derive(Debug, Serialize)]
pub struct WorkerRowView {
    /// The name the queue stores this worker's jobs under.
    pub name: String,
    /// The provider queue the worker's jobs carry; `default` when it names none.
    pub queue: String,
    /// The worker's tags, or `—` when it has none.
    pub tags: String,
    /// Why the worker exists, one line.
    pub detail: String,
}

impl From<&WorkerEntry> for WorkerRowView {
    fn from(entry: &WorkerEntry) -> Self {
        Self {
            name: entry.name.clone(),
            queue: entry.queue.clone().unwrap_or_else(|| "default".to_string()),
            tags: if entry.tags.is_empty() {
                "—".to_string()
            } else {
                entry.tags.join(", ")
            },
            detail: entry.detail.to_string(),
        }
    }
}

/// Everything `assets/views/jobs/index.html` reads beyond the shell's `user`/`active`.
#[derive(Debug, Serialize)]
pub struct JobsView {
    pub queue: QueueView,
    pub workers: Vec<WorkerRowView>,
    /// The scheduler's own entries, from `config/<env>.yaml` — what will be asked for, and
    /// when, as opposed to what is already queued.
    pub scheduled: Vec<ScheduledEntryView>,
    /// The queue's rows, newest first. Empty when this mode keeps no queue, which is what
    /// the template's note is for.
    pub jobs: Vec<JobRowView>,
    /// Whether the row buttons can do anything — false when there is no queue at all.
    pub actionable: bool,
    /// The age at which `processing` counts as stuck, in minutes — the requeue button's
    /// label, so the page and the rule cannot drift apart.
    pub stale_minutes: i64,
    /// One row per process whose liveness is knowable: the scheduler and the worker.
    pub runtime: Vec<RuntimeRowView>,
    /// The warning to show when jobs are waiting and nothing has drained the queue: `None`
    /// when the queue is empty, when a worker has been seen recently, or when there are
    /// stamps at all to judge.
    pub stuck: Option<String>,
}

/// One process's liveness, from the stamp it left behind.
#[derive(Debug, Serialize)]
pub struct RuntimeRowView {
    /// `Scheduler` or `Worker`.
    pub label: &'static str,
    /// Who writes the stamp — what a reader has to start to get it moving.
    pub stamp_writer: &'static str,
    /// `live`, `stale` or `missing`.
    pub state: &'static str,
    /// The badge colour for that state.
    pub state_css: &'static str,
    /// How long ago it last stamped, e.g. `12.0 s`; empty when it never has.
    pub age_label: String,
    /// When it last stamped, ISO, for `local-time.js`; empty when it never has.
    pub seen_utc: String,
    /// The ascii fallback for the same instant.
    pub seen_label: String,
    /// The host and pid of the stamping process, or empty when there is no stamp.
    pub origin: String,
}

impl RuntimeRowView {
    /// The row for `source`, from its newest stamp (if any) and `now`.
    ///
    /// The expected tick is named in the age's tooltip: "3m 05s" is only alarming next to
    /// "expects a stamp every 60 s".
    #[must_use]
    pub fn new(
        source: Source,
        seen: Option<&runtime_heartbeats::Model>,
        now: DateTime<Utc>,
    ) -> Self {
        let (state, state_css) =
            match runtime_heartbeats::liveness(seen.map(|row| row.created_at), now) {
                Liveness::Live => ("live", "bg-emerald-100 text-emerald-800"),
                Liveness::Stale => ("stale", "bg-red-100 text-red-800"),
                Liveness::Missing => ("missing", "bg-slate-100 text-slate-700"),
            };

        // Everything but the age is empty for a source that never stamped, so it reads as a
        // blank row rather than a row full of placeholders.
        let age_label = seen.map_or_else(missing_age_label, |row| {
            duration_label(runtime_heartbeats::age_milliseconds(row.created_at, now))
        });
        let seen_utc = seen
            .map(|row| row.created_at.to_rfc3339())
            .unwrap_or_default();
        let seen_label = seen
            .map(|row| row.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_default();
        let origin = seen
            .map(|row| format!("{} · pid {}", row.host, row.pid))
            .unwrap_or_default();

        Self {
            label: source.label(),
            stamp_writer: source.stamp_writer(),
            state,
            state_css,
            age_label,
            seen_utc,
            seen_label,
            origin,
        }
    }
}

/// The warning to put above the jobs table: `Some` only when rows are waiting *and* no
/// worker has drained the queue recently.
///
/// Both halves are needed — a stale worker with an empty queue is a process that can be
/// started later, not a problem. `worker_state` is the worker's [`Liveness`]; `waiting` is
/// how many rows the queue holds.
#[must_use]
pub fn stuck_warning(
    waiting: usize,
    worker: Option<Liveness>,
    worker_age: Option<String>,
) -> Option<String> {
    if waiting == 0 {
        return None;
    }

    match worker {
        Some(Liveness::Live) => None,
        Some(Liveness::Stale) => Some(format!(
            "{waiting} job(s) are waiting, and no worker has drained the queue for {} — is the \
             worker process still running?",
            worker_age.unwrap_or_else(|| "a while".to_string())
        )),
        // Never seen: the queue may simply be new, but nothing has ever run these jobs.
        _ => Some(format!(
            "{waiting} job(s) are waiting, and no worker has ever drained this queue — is a \
             worker process running? Start one with `cargo loco start --worker`."
        )),
    }
}

/// One entry of the scheduler's `jobs:` map.
#[derive(Debug, Serialize)]
pub struct ScheduledEntryView {
    /// The key in the YAML, which is also what the queue records as the job's name.
    pub name: String,
    /// What a tick actually executes: a task name and its arguments.
    pub run: String,
    /// The cron expression, seconds first.
    pub schedule: String,
    /// The entry's tags, or `—` when it has none.
    pub tags: String,
}

impl From<(&String, &scheduler::Job)> for ScheduledEntryView {
    fn from((name, job): (&String, &scheduler::Job)) -> Self {
        Self {
            name: name.clone(),
            run: job.run.clone(),
            schedule: job.cron.clone(),
            tags: tags_label(job.tags.as_deref()),
        }
    }
}

/// One row of the queue table.
#[derive(Debug, Serialize)]
pub struct JobRowView {
    /// The job's id, as the action URLs carry it.
    pub id: String,
    /// The id shortened to its timestamp prefix — enough to tell two rows apart in a table.
    pub id_short: String,
    /// The worker the row is for, which is also the name the queue stores it under.
    pub name: String,
    /// Loco's own spelling of the status, e.g. `queued`.
    pub status: String,
    /// The status as Loco's own value, for the page to reason about; not serialized, since
    /// `status` is what the template reads.
    #[serde(skip)]
    status_value: JobStatus,
    /// The badge colour for that status.
    pub status_css: &'static str,
    /// The instant the row was written, ISO, for `local-time.js`.
    pub created_utc: String,
    /// The ascii fallback: the same instant, UTC.
    pub created_label: String,
    /// How long it took (finished) or has been taking (queued, running).
    pub elapsed_label: String,
    /// The `run_at` instant, ISO: when the queue became free to run it.
    pub run_at_utc: String,
    /// The row's tags, or `—`.
    pub tags: String,
    /// Whether a cancel would do anything: only a job that has not started.
    pub can_cancel: bool,
    /// Whether a retry would do anything: only a job that failed.
    pub can_retry: bool,
}

impl From<&Job> for JobRowView {
    fn from(job: &Job) -> Self {
        Self {
            status_value: job.status.clone(),
            id: job.id.clone(),
            id_short: job.id.chars().take(8).collect(),
            name: job.name.clone(),
            status: job.status.to_string(),
            status_css: status_css(&job.status),
            created_utc: iso(job.created_at),
            created_label: utc_label(job.created_at),
            elapsed_label: elapsed_label(job, Utc::now()),
            run_at_utc: iso(Some(job.run_at)),
            tags: tags_label(job.tags.as_deref()),
            can_cancel: job.status == JobStatus::Queued,
            can_retry: job.status == JobStatus::Failed,
        }
    }
}

impl JobRowView {
    /// Whether the queue still has work to do for this row — a job nobody has finished or
    /// given up on. This is what the stuck-queue warning counts.
    #[must_use]
    pub const fn is_waiting(&self) -> bool {
        matches!(self.status_value, JobStatus::Queued | JobStatus::Processing)
    }
}

/// What a source that has never stamped says in the age column: not a zero, but the
/// interval a reader should expect.
fn missing_age_label() -> String {
    format!(
        "never (expects one every {} s)",
        runtime_heartbeats::EXPECTED_TICK_SECONDS
    )
}

/// The badge colour for a job's status. One colour per state, so a table scans by colour.
///
/// `JobStatus` is `#[non_exhaustive]`: a state a future Loco adds reads as neutral grey
/// rather than failing to render the page, which is the same colour as a queued job.
#[must_use]
#[allow(clippy::match_same_arms)]
pub const fn status_css(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "bg-slate-100 text-slate-700",
        JobStatus::Processing => "bg-sky-100 text-sky-800",
        JobStatus::Completed => "bg-emerald-100 text-emerald-800",
        JobStatus::Failed => "bg-red-100 text-red-800",
        JobStatus::Cancelled => "bg-amber-100 text-amber-800",
        _ => "bg-slate-100 text-slate-700",
    }
}

/// `at` as an ISO instant for `local-time.js`, empty when the row has no such column set.
fn iso(at: Option<DateTime<Utc>>) -> String {
    at.map(|at| at.to_rfc3339()).unwrap_or_default()
}

/// The fallback text under a `<time>`: the same instant, UTC, in the shape the logs page
/// uses.
fn utc_label(at: Option<DateTime<Utc>>) -> String {
    at.map(|at| at.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_default()
}

/// How long a job took, or has been taking.
///
/// A job the worker has finished carries both ends, so its duration is a fact; one that is
/// queued or running is measured against now, which is why it is recomputed per request.
/// The queue stores no duration of its own.
#[must_use]
pub fn elapsed_label(job: &Job, now: DateTime<Utc>) -> String {
    let started = job.created_at;
    let ended = match job.status {
        JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled => job.updated_at,
        _ => Some(now),
    };

    match (started, ended) {
        (Some(started), Some(ended)) => {
            let elapsed = (ended - started).num_milliseconds();
            // The queue's timestamps are whole seconds, so a job that finished inside one
            // reads as a zero difference; "0 ms" would look like it did nothing at all.
            if elapsed == 0 && !matches!(job.status, JobStatus::Queued | JobStatus::Processing) {
                "<1 s".to_string()
            } else {
                duration_label(elapsed)
            }
        }
        _ => "—".to_string(),
    }
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

/// A list of tags for a cell, or `—` when there are none.
fn tags_label(tags: Option<&[String]>) -> String {
    match tags {
        Some(tags) if !tags.is_empty() => tags.join(", "),
        _ => "—".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A job row as the queue would hand it over: `status` and the timestamps are the only
    /// variation a test needs.
    fn job(status: JobStatus, taken_ms: i64) -> Job {
        let created = DateTime::from_timestamp(1_758_000_000, 0).expect("a valid instant");
        Job {
            id: "01M3JFHDMB2X5YF20FWYGE915A".to_string(),
            name: "CaptureWorker".to_string(),
            data: serde_json::json!({}),
            status,
            run_at: created,
            interval: None,
            created_at: Some(created),
            updated_at: Some(created + chrono::Duration::milliseconds(taken_ms)),
            tags: None,
            priority: 0,
        }
    }

    #[test]
    fn every_job_status_gets_its_own_badge_colour() {
        let css = [
            JobStatus::Queued,
            JobStatus::Processing,
            JobStatus::Completed,
            JobStatus::Failed,
            JobStatus::Cancelled,
        ]
        .map(|status| status_css(&status));

        for (index, colour) in css.iter().enumerate() {
            assert!(
                !css[index + 1..].contains(colour),
                "{colour} is used twice, so the badge stops distinguishing states"
            );
        }
    }

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
    fn a_finished_row_reports_what_it_took_and_a_live_one_what_it_is_taking() {
        let now = DateTime::from_timestamp(1_758_000_000, 0).expect("a valid instant");

        // Finished: the duration is a fact, and `now` does not move it.
        assert_eq!(
            elapsed_label(&job(JobStatus::Completed, 1_540), now),
            "1.5 s"
        );
        assert_eq!(elapsed_label(&job(JobStatus::Failed, 4_000), now), "4.0 s");
        // Whole-second timestamps: a run that fits inside one second is not "0 ms".
        assert_eq!(elapsed_label(&job(JobStatus::Completed, 0), now), "<1 s");
        // ...but a job that has only just been queued is, honestly, zero so far.
        assert_eq!(elapsed_label(&job(JobStatus::Queued, 0), now), "0 ms");

        // Running or waiting: measured against now, so a later request shows more.
        let running = job(JobStatus::Processing, 0);
        assert_eq!(elapsed_label(&running, now), "0 ms");
        assert_eq!(
            elapsed_label(&running, now + chrono::Duration::seconds(12)),
            "12.0 s"
        );
    }

    #[test]
    fn only_the_rows_whose_button_would_work_get_one() {
        let queued = JobRowView::from(&job(JobStatus::Queued, 0));
        assert!(queued.can_cancel && !queued.can_retry);

        let failed = JobRowView::from(&job(JobStatus::Failed, 500));
        assert!(failed.can_retry && !failed.can_cancel);

        for status in [
            JobStatus::Processing,
            JobStatus::Completed,
            JobStatus::Cancelled,
        ] {
            let row = JobRowView::from(&job(status.clone(), 500));
            assert!(
                !row.can_cancel && !row.can_retry,
                "{status} should offer no button: {row:?}"
            );
        }

        // The short id is a prefix of the real one, and the two rows are told apart by it.
        assert_eq!(queued.id_short.len(), 8);
        assert!(queued.id.starts_with(&queued.id_short));
        assert_eq!(queued.tags, "—");
    }

    #[test]
    fn the_scheduled_entry_shows_what_a_tick_runs() {
        let name = "enqueue_capture".to_string();
        let configured = scheduler::Job {
            run: "enqueue_capture".to_string(),
            shell: false,
            run_on_start: false,
            cron: "0 * * * * *".to_string(),
            tags: None,
            output: None,
        };
        let entry = ScheduledEntryView::from((&name, &configured));

        assert_eq!(entry.name, "enqueue_capture");
        assert_eq!(entry.run, "enqueue_capture");
        assert_eq!(entry.schedule, "0 * * * * *");
        assert_eq!(entry.tags, "—");
    }
}
