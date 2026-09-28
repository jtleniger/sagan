//! The dispatcher: which ticks run a job, and what each outcome records.
//!
//! `dispatch` is driven directly with a fake job and an injected `now`, so the due rule and
//! the overlap guard are tested without waiting for a clock or a camera.

use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, Duration, Local, Utc};
use loco_rs::prelude::*;
use sagan::{
    app::App,
    jobs::PeriodicJob,
    models::job_runs::{self, Model, Status},
    tasks::periodic_work::dispatch,
};
use sea_orm::EntityTrait;
use serial_test::serial;

/// A job whose name, slot and outcome the test sets.
struct FakeJob {
    name: &'static str,
    /// What `latest_slot` answers: `None` is a job that is disabled or unconfigured.
    slot: Option<DateTime<Utc>>,
    stale_after: Duration,
    failing: bool,
    runs: AtomicUsize,
}

impl FakeJob {
    const fn due(slot: DateTime<Utc>) -> Self {
        Self {
            name: "fake",
            slot: Some(slot),
            stale_after: Duration::minutes(5),
            failing: false,
            runs: AtomicUsize::new(0),
        }
    }

    fn disabled() -> Self {
        Self {
            slot: None,
            ..Self::due(Utc::now())
        }
    }

    const fn failing(slot: DateTime<Utc>) -> Self {
        Self {
            failing: true,
            ..Self::due(slot)
        }
    }

    const fn named(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }

    const fn with_stale_after(mut self, stale_after: Duration) -> Self {
        self.stale_after = stale_after;
        self
    }

    fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl PeriodicJob for FakeJob {
    fn name(&self) -> &'static str {
        self.name
    }

    fn detail(&self) -> &'static str {
        "a job that exists only in this test"
    }

    fn stale_after(&self) -> Duration {
        self.stale_after
    }

    async fn interval(&self, _ctx: &AppContext) -> Result<String> {
        Ok("Every minute".to_string())
    }

    async fn latest_slot(
        &self,
        _ctx: &AppContext,
        _now: DateTime<Local>,
    ) -> Result<Option<DateTime<Utc>>> {
        Ok(self.slot)
    }

    async fn next_slot(
        &self,
        _ctx: &AppContext,
        _now: DateTime<Local>,
    ) -> Result<Option<DateTime<Utc>>> {
        Ok(self.slot.map(|slot| slot + Duration::minutes(1)))
    }

    async fn run(&self, _ctx: &AppContext) -> Result<String> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        if self.failing {
            Err(Error::string("boom"))
        } else {
            Ok("did the thing".to_string())
        }
    }
}

/// A due job runs once, and the attempt is recorded as succeeded with the detail it returned.
#[tokio::test]
#[serial]
async fn a_due_job_runs_once_and_records_succeeded() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let ctx = &boot.app_context;
    let now = Local::now();
    let slot = now.with_timezone(&Utc);
    let job = FakeJob::due(slot);

    dispatch(ctx, &[&job], now).await.expect("dispatch runs");

    assert_eq!(job.runs(), 1);
    let row = Model::newest(&ctx.db, "fake")
        .await
        .expect("the read runs")
        .expect("a run was recorded");
    assert_eq!(row.status, job_runs::SUCCEEDED);
    assert_eq!(row.slot_at.with_timezone(&Utc), slot);
    assert!(
        row.finished_at.is_some(),
        "a finished run has a finish time"
    );
    assert_eq!(row.detail.as_deref(), Some("did the thing"));
}

/// A job with no slot runs nothing and writes nothing.
#[tokio::test]
#[serial]
async fn a_job_with_no_slot_does_nothing() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let ctx = &boot.app_context;
    let job = FakeJob::disabled();

    dispatch(ctx, &[&job], Local::now())
        .await
        .expect("dispatch runs");

    assert_eq!(job.runs(), 0);
    assert!(
        Model::newest(&ctx.db, "fake")
            .await
            .expect("the read runs")
            .is_none(),
        "a skipped job writes no row"
    );
}

/// A run whose slot has been claimed already is not retried: the failed attempt consumed the
/// slot, and only the next slot tries again.
#[tokio::test]
#[serial]
async fn a_failed_run_is_recorded_and_its_slot_is_not_retried() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let ctx = &boot.app_context;
    let now = Local::now();
    let job = FakeJob::failing(now.with_timezone(&Utc));

    dispatch(ctx, &[&job], now).await.expect("dispatch runs");

    let row = Model::newest(&ctx.db, "fake")
        .await
        .expect("the read runs")
        .expect("a run was recorded");
    assert_eq!(row.status, job_runs::FAILED);
    assert_eq!(row.detail.as_deref(), Some("boom"));
    assert!(row.finished_at.is_some());

    // Same slot, another tick: the claim conflicts, so the run count cannot move.
    dispatch(ctx, &[&job], now)
        .await
        .expect("dispatch runs again");
    assert_eq!(job.runs(), 1, "a failed slot is not retried");
}

/// A `running` row inside `stale_after` is the previous tick still working: the job is
/// skipped, and no second row is written.
#[tokio::test]
#[serial]
async fn a_run_still_working_blocks_the_next_tick() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let ctx = &boot.app_context;
    let now = Local::now();
    let now_utc = now.with_timezone(&Utc);

    // A previous tick claimed an earlier slot and has not finished; its age is near zero.
    let running = Model::claim(&ctx.db, "fake", now_utc - Duration::minutes(1), now_utc)
        .await
        .expect("the claim runs")
        .expect("the slot is free");

    let job = FakeJob::due(now_utc);
    dispatch(ctx, &[&job], now).await.expect("dispatch runs");

    assert_eq!(job.runs(), 0, "the guard skips the job");
    let newest = Model::newest(&ctx.db, "fake")
        .await
        .expect("the read runs")
        .expect("the running row is still there");
    assert_eq!(newest.id, running.id);
    assert_eq!(newest.status, job_runs::RUNNING);
}

/// A `running` row past `stale_after` belongs to a process that is gone: it is abandoned, and
/// the current slot runs as a fresh attempt.
#[tokio::test]
#[serial]
async fn a_stale_run_is_abandoned_and_the_current_slot_runs() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let ctx = &boot.app_context;
    let now = Local::now();
    let now_utc = now.with_timezone(&Utc);
    let started = now_utc - Duration::minutes(30);

    let stale = Model::claim(&ctx.db, "fake", started, started)
        .await
        .expect("the claim runs")
        .expect("the slot is free");

    let job = FakeJob::due(now_utc).with_stale_after(Duration::minutes(5));
    dispatch(ctx, &[&job], now).await.expect("dispatch runs");

    assert_eq!(job.runs(), 1, "the current slot runs");
    let abandoned = job_runs::Entity::find_by_id(stale.id)
        .one(&ctx.db)
        .await
        .expect("the row reads")
        .expect("the row exists");
    assert_eq!(abandoned.status, job_runs::FAILED);
    assert_eq!(
        abandoned.detail.as_deref(),
        Some(job_runs::ABANDONED_DETAIL)
    );

    let newest = Model::newest(&ctx.db, "fake")
        .await
        .expect("the read runs")
        .expect("the new run was recorded");
    assert_eq!(newest.status, job_runs::SUCCEEDED);
    assert_ne!(newest.id, stale.id);
}

/// A job that fails to run does not stop the jobs after it in the same tick.
#[tokio::test]
#[serial]
async fn one_jobs_failure_does_not_stop_the_next() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let ctx = &boot.app_context;
    let now = Local::now();
    let slot = now.with_timezone(&Utc);

    let broken = FakeJob::failing(slot).named("broken");
    let working = FakeJob::due(slot).named("working");
    dispatch(ctx, &[&broken, &working], now)
        .await
        .expect("dispatch itself succeeds");

    assert_eq!(broken.runs(), 1);
    assert_eq!(working.runs(), 1, "the second job still ran");
    assert_eq!(
        Model::newest(&ctx.db, "broken")
            .await
            .expect("the read runs")
            .expect("the failing run was recorded")
            .status,
        Status::Failed.as_str()
    );
    assert_eq!(
        Model::newest(&ctx.db, "working")
            .await
            .expect("the read runs")
            .expect("the working run was recorded")
            .status,
        Status::Succeeded.as_str()
    );
}
