//! The periodic jobs this app runs, and the registry the dispatcher iterates.
//!
//! A job answers three kinds of question: what it is (`name`, `detail`, `stale_after`), when a
//! tick should run it (`interval`, `latest_slot`, `next_slot` — all read from stored settings,
//! so the page and the dispatcher cannot disagree), and what it does (`run`). The dispatcher
//! (`crate::tasks::periodic_work`) and the `/jobs` page both work through this trait, so a job
//! added here cannot be declared in one and be invisible in the other.

pub mod capture;

use chrono::{DateTime, Duration, Local, Utc};
use loco_rs::{app::AppContext, Result};

use crate::jobs::capture::CaptureJob;

/// One periodic job.
///
/// `latest_slot`/`next_slot` take `now` as a parameter rather than reading the clock, so the
/// due rule can be driven from a test. They are `async` because a job's cadence is a stored
/// setting: `capture`'s comes from the Captures section of `app_settings`.
#[async_trait::async_trait]
pub trait PeriodicJob: Send + Sync {
    /// The stable name runs are recorded under (`job_runs.job`) and the page shows.
    fn name(&self) -> &'static str;

    /// Why the job exists, one line, for the page.
    fn detail(&self) -> &'static str;

    /// How long a `running` row may stay unexplained before a later tick abandons it.
    ///
    /// It bounds the one real hazard of running inline: the scheduler forks a fresh child per
    /// tick with no dedup, so a slow job can overlap the next tick's. A job whose work can take
    /// longer than this must say so.
    fn stale_after(&self) -> Duration;

    /// The cadence as configured right now, one line, for the page — e.g. `Every 5 minutes`.
    async fn interval(&self, ctx: &AppContext) -> Result<String>;

    /// The most recent schedule slot at or before `now`, or `None` when the job is disabled.
    async fn latest_slot(
        &self,
        ctx: &AppContext,
        now: DateTime<Local>,
    ) -> Result<Option<DateTime<Utc>>>;

    /// The next slot strictly after `now`, for the page's "next due" column.
    async fn next_slot(
        &self,
        ctx: &AppContext,
        now: DateTime<Local>,
    ) -> Result<Option<DateTime<Utc>>>;

    /// Do the work, returning the detail recorded on success.
    async fn run(&self, ctx: &AppContext) -> Result<String>;
}

/// The one job this app runs today. `ReclaimJob` and `SyncJob` are what a later change adds.
pub static CAPTURE: CaptureJob = CaptureJob;

/// Every registered job, in the order the dispatcher runs them and the page lists them.
#[must_use]
pub fn configured() -> Vec<&'static dyn PeriodicJob> {
    vec![&CAPTURE]
}
