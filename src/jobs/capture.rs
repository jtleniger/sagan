//! `CaptureJob` — takes one still image from the camera and writes it to a file.
//!
//! The destination is `settings.capture.dir`; the camera driver only writes the file, the way the
//! Pi's `libcamera-*` command-line tools do. Its cadence is the Captures interval saved on the
//! Configuration page, so the *stored* interval drives the schedule: a scheduler cron expression
//! is static YAML and cannot.

use std::path::PathBuf;

use chrono::{DateTime, Duration, Local, Utc};
use loco_rs::{app::AppContext, config::Config, prelude::*};
use serde::Deserialize;

use crate::{
    captures::CaptureSettings,
    hardware::{Hardware, HardwareError},
    jobs::PeriodicJob,
    models::app_settings,
};

/// The directory captures land in when `settings.capture.dir` is absent, and the one
/// `.gitignore` excludes.
pub const DEFAULT_DIR: &str = "captures";

/// The `settings.capture` block.
///
/// Every field defaults, so an absent `settings:` block (or an absent `capture:` key) writes to
/// [`DEFAULT_DIR`] rather than failing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureConfig {
    /// Where the camera writes its stills. A relative path resolves against the process working
    /// directory.
    pub dir: PathBuf,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            dir: DEFAULT_DIR.into(),
        }
    }
}

impl CaptureConfig {
    /// # Errors
    /// When the `settings.capture` block exists but does not match this schema — a typo must fail
    /// the boot, not silently fall back to `captures`.
    pub fn from_context(config: &Config) -> Result<Self> {
        /// The `settings:` block, as far as this module reads it.
        ///
        /// `Config::settings` deserializes the *whole* block, so the `capture:` key has to be
        /// named here; `deny_unknown_fields` on [`CaptureConfig`] then rejects a typo inside
        /// `capture:` rather than defaulting it.
        #[derive(Debug, Default, Deserialize)]
        #[serde(default)]
        struct Settings {
            capture: CaptureConfig,
        }

        Ok(config.settings::<Settings>()?.capture)
    }
}

/// The capture job: one frame per due slot, written as `<taken_at_ms>.jpg`.
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
        "Takes a still image from the camera and writes it to the capture directory."
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
    /// `HardwareError` when this host's camera cannot frame (no camera on a `driver: pi` build) or
    /// the destination cannot be written — recorded as a failed run rather than a capture that
    /// never happened.
    async fn run(&self, ctx: &AppContext) -> Result<String> {
        let dir = CaptureConfig::from_context(&ctx.config)?.dir;

        // `<taken_at_ms>.jpg` — the capture instant, sortable, and unique at any interval the
        // Captures form accepts (1 minute at the fastest); `crate::tasks::hardware_check` writes
        // `hardware-check-<taken_at_ms>.jpg` for the same reason.
        let filename = format!("{}.jpg", Utc::now().timestamp_millis());

        Hardware::of(ctx)?
            .camera
            .capture(&dir, &filename)
            .await
            .map_err(|err: HardwareError| Error::string(&err.to_string()))?;

        let path = dir.join(&filename);
        tracing::info!(path = %path.display(), "capture written");
        Ok(filename)
    }
}
