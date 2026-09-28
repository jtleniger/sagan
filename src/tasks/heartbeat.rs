//! `cargo loco task heartbeat` — the scheduler's once-a-minute liveness tick.
//!
//! Two stamps come out of one tick: this task records that the scheduler ran, and the job it
//! enqueues records that a worker drained the queue. That is the whole reason the task
//! exists — `/jobs` needs to tell "the clock is ticking" apart from "someone is working".

use loco_rs::prelude::*;

use crate::{
    models::runtime_heartbeats::{self, Source},
    workers::heartbeat::{HeartbeatArgs, HeartbeatWorker},
};

pub struct Heartbeat;

#[async_trait]
impl Task for Heartbeat {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "heartbeat".to_string(),
            detail: "Record that the scheduler ticked, and enqueue a job whose completion \
                     records that a worker is draining the queue. Run once a minute by the \
                     scheduler; the /jobs page reads the stamps."
                .to_string(),
        }
    }

    /// # Errors
    /// A database error writing the stamp, or the queue refusing the enqueue.
    async fn run(&self, ctx: &AppContext, _vars: &task::Vars) -> Result<()> {
        // Stamped before anything else, so a tick that cannot enqueue still proves the
        // scheduler is alive — which is exactly the failure a reader needs to see.
        runtime_heartbeats::Model::stamp(&ctx.db, Source::Scheduler).await?;

        let job = HeartbeatWorker::perform_later(ctx, HeartbeatArgs {}).await?;
        tracing::trace!(job, "heartbeat");
        Ok(())
    }
}
