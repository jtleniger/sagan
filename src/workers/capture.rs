//! `CaptureWorker` — takes one still image and puts it in the file store.
//!
//! Enqueued by the scheduler's `enqueue_capture` heartbeat when the stored Captures
//! interval is due; see `crate::tasks::enqueue_capture`.

use std::path::Path;

use axum::body::Bytes;
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    hardware::{Hardware, HardwareError},
    workers::WorkerEntry,
};

/// The worker's arguments: there are none. Everything it needs — the camera and the file
/// store — comes off the `AppContext`; the *when* is the scheduler heartbeat's decision,
/// not an argument.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CaptureArgs {}

pub struct CaptureWorker {
    pub ctx: AppContext,
}

#[async_trait]
impl BackgroundWorker<CaptureArgs> for CaptureWorker {
    fn build(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }

    /// # Errors
    /// `HardwareError` when this host's camera cannot frame (no camera on a `driver: pi`
    /// build): the queue records a failed job rather than a capture that never happened.
    /// `StorageError` when the store refuses the write.
    async fn perform(&self, _args: CaptureArgs) -> Result<()> {
        let capture = Hardware::of(&self.ctx)?
            .camera
            .capture()
            .await
            .map_err(|err: HardwareError| Error::string(&err.to_string()))?;

        // `<taken_at_ms>.jpg` — the camera's own instant, sortable, and unique at any
        // interval the Captures form accepts (1 minute at the fastest). Every object in
        // this store is a capture, so the key needs no prefix; `crate::tasks::hardware_check`
        // writes `hardware-check-<taken_at_ms>.jpg` for the same reason.
        let key = format!("{}.jpg", capture.taken_at_ms);
        let bytes = Bytes::from(capture.jpeg);
        self.ctx.storage.upload(Path::new(&key), &bytes).await?;

        tracing::info!(
            key,
            bytes = bytes.len(),
            width = capture.width,
            height = capture.height,
            "capture stored"
        );
        Ok(())
    }
}

impl CaptureWorker {
    /// The worker as the `/jobs` page shows it.
    ///
    /// The name, queue and tags are the trait's own answers, so the page cannot claim a queue
    /// or a tag this worker's jobs do not carry. Qualify with `<Self as …>`: the trait is
    /// generic over its argument type and inference has nothing to go on at a bare
    /// `CaptureWorker::class_name()`.
    #[must_use]
    pub fn entry() -> WorkerEntry {
        WorkerEntry {
            name: <Self as BackgroundWorker<CaptureArgs>>::class_name(),
            queue: <Self as BackgroundWorker<CaptureArgs>>::queue(),
            tags: <Self as BackgroundWorker<CaptureArgs>>::tags(),
            detail: "Takes a still image from the camera and stores it in the file store.",
        }
    }
}
