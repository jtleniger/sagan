//! `HeartbeatWorker` — records that a worker process is draining the queue.
//!
//! Enqueued by the scheduler's `heartbeat` task, and performed by whichever process polls the
//! queue; see `crate::tasks::heartbeat` and `crate::models::runtime_heartbeats`.

use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    models::runtime_heartbeats::{self, Source},
    workers::WorkerEntry,
};

/// The worker's arguments: there are none. The stamp's content is a fact about the process
/// performing it, not something a producer could supply.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct HeartbeatArgs {}

pub struct HeartbeatWorker {
    pub ctx: AppContext,
}

#[async_trait]
impl BackgroundWorker<HeartbeatArgs> for HeartbeatWorker {
    fn build(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }

    /// # Errors
    /// A database error writing the stamp — which, on a queue that is draining jobs at all,
    /// means the database the queue itself lives in is unreachable.
    async fn perform(&self, _args: HeartbeatArgs) -> Result<()> {
        runtime_heartbeats::Model::stamp(&self.ctx.db, Source::Worker).await?;
        Ok(())
    }
}

impl HeartbeatWorker {
    /// The worker as the `/jobs` page shows it.
    #[must_use]
    pub fn entry() -> WorkerEntry {
        WorkerEntry {
            name: <Self as BackgroundWorker<HeartbeatArgs>>::class_name(),
            queue: <Self as BackgroundWorker<HeartbeatArgs>>::queue(),
            tags: <Self as BackgroundWorker<HeartbeatArgs>>::tags(),
            detail: "Records that a worker process drained the queue, once a minute.",
        }
    }
}
