//! `CaptureJob` — takes one still image and puts it in the file store.
//!
//! Its cadence is the Captures interval saved on the Configuration page, so the *stored* interval
//! drives the schedule: a scheduler cron expression is static YAML and cannot.

use std::path::Path;

use async_trait::async_trait;
use axum::body::Bytes;
use chrono::{DateTime, Duration, Local, Utc};
use loco_rs::{app::AppContext, prelude::*};

use crate::{
    captures::CaptureSettings,
    hardware::{Hardware, HardwareError},
    jobs::PeriodicJob,
    models::app_settings,
};

/// The capture job: one frame per due slot, stored as `<taken_at_ms>.jpg`.
pub struct CaptureJob;

impl CaptureJob {
    /// The stored Captures section; a database with no row yet reads as the default interval.
    async fn settings(ctx: &AppContext) -> Result<CaptureSettings> {
        Ok(app_settings::Model::capture_settings(&ctx.db).await?)
    }
}

#[async_trait]
impl PeriodicJob for CaptureJob {
    fn name(&self) -> &'static str {
        "capture"
    }

    fn detail(&self) -> &'static str {
        "Takes a still image from the camera and stores it in the file store."
    }

    fn stale_after(&self) -> Duration {
        // A capture takes milliseconds. Five minutes is unambiguous: a run still `running`
        // after that belongs to a process that is gone, not to a slow camera.
        Duration::minutes(5)
    }

    async fn interval(&self, ctx: &AppContext) -> Result<String> {
        Ok(Self::settings(ctx).await?.interval.describe())
    }

    async fn latest_slot(
        &self,
        ctx: &AppContext,
        now: DateTime<Local>,
    ) -> Result<Option<DateTime<Utc>>> {
        Ok(Self::settings(ctx).await?.interval.latest_slot(now))
    }

    async fn next_slot(
        &self,
        ctx: &AppContext,
        now: DateTime<Local>,
    ) -> Result<Option<DateTime<Utc>>> {
        Ok(Self::settings(ctx).await?.interval.next_slot(now))
    }

    /// # Errors
    /// `HardwareError` when this host's camera cannot frame (no camera on a `driver: pi`
    /// build) — recorded as a failed run rather than a capture that never happened — or a
    /// `StorageError` when the store refuses the write.
    async fn run(&self, ctx: &AppContext) -> Result<String> {
        let capture = Hardware::of(ctx)?
            .camera
            .capture()
            .await
            .map_err(|err: HardwareError| Error::string(&err.to_string()))?;

        // `<taken_at_ms>.jpg` — the camera's own instant, sortable, and unique at any interval
        // the Captures form accepts (1 minute at the fastest). Every object in this store is a
        // capture, so the key needs no prefix; `crate::tasks::hardware_check` writes
        // `hardware-check-<taken_at_ms>.jpg` for the same reason.
        let key = format!("{}.jpg", capture.taken_at_ms);
        let bytes = Bytes::from(capture.jpeg);
        ctx.storage.upload(Path::new(&key), &bytes).await?;

        tracing::info!(
            key,
            bytes = bytes.len(),
            width = capture.width,
            height = capture.height,
            "capture stored"
        );
        Ok(format!("{key} ({} bytes)", bytes.len()))
    }
}
