//! `cargo loco task periodic_work` — the scheduler's once-a-minute dispatcher.
//!
//! There is no queue. For each registered job this task decides whether a slot is due, claims
//! it in `job_runs`, runs the work inline, and records the outcome. Running inline is the
//! point: the scheduler's fork-per-tick is a property of Loco, not of a queue, and the claim
//! plus the running-guard is what keeps a slow job from overlapping its next tick.

use chrono::{DateTime, Local, Utc};
use loco_rs::prelude::*;

use crate::{
    jobs::{self, PeriodicJob},
    models::job_runs::{self, Status},
};

pub struct PeriodicWork;

#[async_trait]
impl Task for PeriodicWork {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "periodic_work".to_string(),
            detail: "Run every due job inline, once a minute, recording each attempt in \
                     job_runs. The scheduler runs this; a tick with nothing due does nothing."
                .to_string(),
        }
    }

    /// # Errors
    /// A database error from the dispatch itself. A job's own failure is recorded, not raised.
    async fn run(&self, ctx: &AppContext, _vars: &task::Vars) -> Result<()> {
        dispatch(ctx, &jobs::configured(), Local::now()).await
    }
}

/// Runs every due job in `jobs`, then prunes old history.
///
/// `now` is a parameter so a test can drive the due rule without waiting; [`PeriodicWork`]
/// passes the wall clock. Jobs run sequentially in registration order, and one job's failure
/// is recorded and stepped over — it must not stop the others in the same tick.
///
/// # Errors
/// A database error reading or writing `job_runs`, including the prune.
pub async fn dispatch(
    ctx: &AppContext,
    jobs: &[&dyn PeriodicJob],
    now: DateTime<Local>,
) -> Result<()> {
    for job in jobs {
        if let Err(err) = dispatch_one(ctx, *job, now).await {
            tracing::error!(job = job.name(), error = %err, "periodic job dispatch failed");
        }
    }

    job_runs::Model::prune(&ctx.db, now.with_timezone(&Utc)).await?;
    Ok(())
}

/// One job's tick: is a slot due, is the previous run still working, then claim, run, record.
async fn dispatch_one(ctx: &AppContext, job: &dyn PeriodicJob, now: DateTime<Local>) -> Result<()> {
    let now_utc: DateTime<Utc> = now.with_timezone(&Utc);

    let Some(slot) = job.latest_slot(ctx, now).await? else {
        tracing::trace!(job = job.name(), "no slot; job disabled or unconfigured");
        return Ok(());
    };

    // The overlap guard. A `running` row inside `stale_after` means the previous tick's run is
    // still working. Outside it, the process that wrote the row is gone, so the row is
    // abandoned — and the slot it held stays consumed, which makes the *next* slot the retry.
    if let Some(running) = job_runs::Model::running(&ctx.db, job.name()).await? {
        if running.age(now_utc) < job.stale_after() {
            tracing::trace!(job = job.name(), "previous run still working; skip");
            return Ok(());
        }
        tracing::warn!(job = job.name(), run = running.id, "abandoning a stale run");
        job_runs::Model::abandon(&ctx.db, running.id, now_utc).await?;
    }

    let Some(claimed) = job_runs::Model::claim(&ctx.db, job.name(), slot, now_utc).await? else {
        tracing::trace!(job = job.name(), "slot already claimed by another runner");
        return Ok(());
    };

    let (status, detail) = match job.run(ctx).await {
        Ok(detail) => (Status::Succeeded, detail),
        Err(err) => (Status::Failed, err.to_string()),
    };
    job_runs::Model::finish(&ctx.db, claimed.id, status, Some(&detail), Utc::now()).await?;
    tracing::info!(
        job = job.name(),
        status = status.as_str(),
        detail,
        "periodic run recorded"
    );
    Ok(())
}
