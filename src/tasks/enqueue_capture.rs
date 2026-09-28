//! `cargo loco task enqueue_capture` — the scheduler's once-a-minute heartbeat.
//!
//! It enqueues a [`crate::workers::capture::CaptureWorker`] job only when the interval
//! stored by the Configuration page says one is due, which is the only way a *stored*
//! interval can drive a schedule: a scheduler cron expression is static YAML.

use loco_rs::prelude::*;

use crate::{
    models::app_settings,
    workers::capture::{CaptureArgs, CaptureWorker},
};

pub struct EnqueueCapture;

#[async_trait]
impl Task for EnqueueCapture {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "enqueue_capture".to_string(),
            detail: "Enqueue a camera capture when the Captures interval is due. The scheduler \
                     runs this once a minute; a run outside the minute the interval names does \
                     nothing."
                .to_string(),
        }
    }

    /// # Errors
    /// A database error reading the Captures section, or the queue refusing the enqueue.
    async fn run(&self, ctx: &AppContext, _vars: &task::Vars) -> Result<()> {
        let interval = app_settings::Model::capture_settings(&ctx.db)
            .await?
            .interval;

        if !interval.due_at(chrono::Local::now().time()) {
            tracing::trace!(interval = interval.describe(), "no capture due");
            return Ok(());
        }

        let job = CaptureWorker::perform_later(ctx, CaptureArgs {}).await?;
        tracing::info!(job, interval = interval.describe(), "capture enqueued");
        Ok(())
    }
}
