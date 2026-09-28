//! Reading and steering the app's job queue — the `/jobs` page's data source.
//!
//! Loco's `Queue` handle exposes mutations but never the rows: `get_jobs` lives on the
//! `QueueProvider` trait, which `Queue` keeps private. The page therefore reads the queue's
//! own SQLite file through Loco's public `bgworker::sqlt` helpers, on a second pool.
//!
//! The one statement Loco does not expose for a *single* job is the cancel:
//! `Queue::cancel_jobs` cancels every job of a worker *name* still in `queued`. That
//! `UPDATE` is the only hand-written SQL here, isolated in [`Inspector::cancel_queued`]
//! with the table name it depends on.
//!
//! Nothing here is a second source of truth: every mutation is Loco's own function, called
//! on the same file the worker process polls, which is what makes a web-only process able
//! to show and steer jobs another process is running.

use loco_rs::{
    app::AppContext,
    bgworker::{sqlt, Job, JobStatus},
    config::{Config, QueueConfig, WorkerMode},
    Error, Result,
};
use sea_orm::sqlx::SqlitePool;
use std::sync::Arc;

/// The age at which a job still in `processing` is assumed to belong to a worker that died
/// mid-job, and so is put back on the queue. A capture takes milliseconds; minutes is
/// unambiguous.
pub const STALE_PROCESSING_MINUTES: i64 = 5;

/// The queue, as the `/jobs` page reads and steers it.
///
/// Built at boot only when `workers.mode` runs a queue backed by SQLite;
/// `ForegroundBlocking` and `BackgroundAsync` keep no queue, so there is no file to open
/// and [`Inspector::of`] answers `None` for them.
pub struct Inspector {
    pool: SqlitePool,
}

impl Inspector {
    /// A pool over `queue.uri`, when this build's mode keeps a queue at all.
    ///
    /// The queue's tables are created by the queue provider during boot (Loco's own setup
    /// step), so this only opens the file the provider is already using.
    ///
    /// # Errors
    /// A SQLite file the filesystem refuses, or a `queue.uri` SQLite will not accept.
    pub async fn from_config(config: &Config) -> Result<Option<Self>> {
        if !matches!(config.workers.mode, WorkerMode::BackgroundQueue) {
            return Ok(None);
        }

        let Some(QueueConfig::Sqlite(queue)) = &config.queue else {
            return Ok(None);
        };

        Ok(Some(Self::with_pool(
            SqlitePool::connect(&queue.uri).await?,
        )))
    }

    /// An inspector over an already-open pool. The boot path builds one from the config;
    /// tests build one over a scratch file.
    #[must_use]
    pub const fn with_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// The inspector `Hooks::after_context` registered, or `None` when this mode keeps no
    /// queue — the page shows a note in that case rather than an empty table.
    #[must_use]
    pub fn of(ctx: &AppContext) -> Option<Arc<Self>> {
        ctx.shared_store.get::<Arc<Self>>()
    }

    /// Every job the queue holds, newest first.
    ///
    /// `created_at` is SQLite's `CURRENT_TIMESTAMP`, so it has whole-second resolution and
    /// two jobs enqueued together tie; the id (`ULID`, ordered by creation) breaks the tie.
    ///
    /// # Errors
    /// A database error — a queue table this build cannot read, or a pool that lost the file.
    pub async fn jobs(&self) -> Result<Vec<Job>> {
        let mut jobs = sqlt::get_jobs(&self.pool, None, None).await?;
        jobs.sort_by(|left, right| (right.created_at, &right.id).cmp(&(left.created_at, &left.id)));
        Ok(jobs)
    }

    /// Cancels a job that has not started, answering whether it did anything.
    ///
    /// A *running* job cannot be cancelled: the process performing it holds the row in
    /// `processing` and never re-reads it, so "cancel" can only mean "do not start this".
    /// Stopping running work means stopping the worker process.
    ///
    /// # Errors
    /// A database error. The statement itself is the only raw SQL in the app; it is
    /// Loco's own cancel, narrowed from worker name to job id.
    pub async fn cancel_queued(&self, id: &str) -> Result<bool> {
        let result = sea_orm::sqlx::query(
            "UPDATE sqlt_loco_queue SET status = $1, updated_at = CURRENT_TIMESTAMP \
             WHERE id = $2 AND status = $3",
        )
        .bind(JobStatus::Cancelled.to_string())
        .bind(id)
        .bind(JobStatus::Queued.to_string())
        .execute(&self.pool)
        .await
        .map_err(|err| Error::string(&format!("cancelling job {id} failed: {err}")))?;

        Ok(result.rows_affected() > 0)
    }

    /// Puts a job that failed back on the queue for another attempt, answering whether one
    /// moved.
    ///
    /// # Errors
    /// A database error.
    pub async fn retry(&self, id: &str) -> Result<bool> {
        Ok(sqlt::retry_failed(&self.pool, Some(id)).await? > 0)
    }

    /// Puts jobs stranded in `processing` for longer than [`STALE_PROCESSING_MINUTES`]
    /// back on the queue — the recovery path for a worker that died mid-job.
    ///
    /// # Errors
    /// A database error.
    pub async fn requeue_stale(&self) -> Result<()> {
        sqlt::requeue(&self.pool, &STALE_PROCESSING_MINUTES).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    /// A scratch queue file, unique per test so a parallel run cannot share it.
    async fn scratch_pool(name: &str) -> SqlitePool {
        let path = format!("target/test-queue-{name}.sqlite");
        let _ = std::fs::remove_file(&path);
        let pool = SqlitePool::connect(&format!("sqlite://{path}?mode=rwc"))
            .await
            .expect("the scratch queue opens");
        sqlt::initialize_database(&pool)
            .await
            .expect("the queue tables are created");
        pool
    }

    async fn enqueue(pool: &SqlitePool, name: &str) -> String {
        sqlt::enqueue(
            pool,
            name,
            serde_json::json!({}),
            Utc::now(),
            None,
            None,
            None,
        )
        .await
        .expect("the job is enqueued")
    }

    /// Move a row into a state only the worker loop would normally produce, so the
    /// transitions under test can start from it. Raw SQL because that is the point: the
    /// rows are the queue's, not the app's.
    async fn set_state(pool: &SqlitePool, id: &str, status: JobStatus, age_minutes: i64) {
        sea_orm::sqlx::query(
            "UPDATE sqlt_loco_queue SET status = $1, updated_at = DATETIME('now', $2) WHERE id = \
             $3",
        )
        .bind(status.to_string())
        .bind(format!("-{age_minutes} minute"))
        .bind(id)
        .execute(pool)
        .await
        .expect("the row's state is set");
    }

    async fn status_of(inspector: &Inspector, id: &str) -> JobStatus {
        inspector
            .jobs()
            .await
            .expect("the queue reads")
            .into_iter()
            .find(|job| job.id == id)
            .expect("the job is listed")
            .status
    }

    #[tokio::test]
    async fn the_page_sees_the_rows_and_can_move_them() {
        let pool = scratch_pool("inspector").await;
        let inspector = Inspector::with_pool(pool.clone());

        // An empty queue is a valid, readable answer.
        assert!(inspector
            .jobs()
            .await
            .expect("an empty queue reads")
            .is_empty());

        // Newest first, whatever order the rows were written in.
        let older = enqueue(&pool, "CaptureWorker").await;
        let newer = enqueue(&pool, "MailerWorker").await;
        let listed: Vec<String> = inspector
            .jobs()
            .await
            .expect("the queue reads")
            .into_iter()
            .map(|job| job.id)
            .collect();
        assert_eq!(listed, vec![newer.clone(), older.clone()], "newest first");

        // A queued job is cancelled by id, and cancelling is not idempotent: the second
        // call reports that nothing moved.
        assert!(
            inspector
                .cancel_queued(&older)
                .await
                .expect("the update runs"),
            "a queued job is cancelled"
        );
        assert_eq!(status_of(&inspector, &older).await, JobStatus::Cancelled);
        assert!(
            !inspector
                .cancel_queued(&older)
                .await
                .expect("the update runs"),
            "a cancelled job is not queued any more"
        );

        // A job that is running is left alone — cancel means "do not start", and the
        // worker's own row must not be rewritten under it.
        set_state(&pool, &newer, JobStatus::Processing, 0).await;
        assert!(
            !inspector
                .cancel_queued(&newer)
                .await
                .expect("the update runs"),
            "a processing job is not cancelled"
        );
        assert_eq!(status_of(&inspector, &newer).await, JobStatus::Processing);

        // Failed → queued, once.
        set_state(&pool, &newer, JobStatus::Failed, 0).await;
        assert!(
            inspector.retry(&newer).await.expect("the update runs"),
            "a failed job is retried"
        );
        assert_eq!(status_of(&inspector, &newer).await, JobStatus::Queued);
        assert!(
            !inspector.retry(&newer).await.expect("the update runs"),
            "a queued job is not retried"
        );

        // A job stuck in processing past the staleness window is put back; a recent one
        // is not, because it may be a worker that is simply still working.
        let fresh = enqueue(&pool, "CaptureWorker").await;
        set_state(&pool, &fresh, JobStatus::Processing, 0).await;
        set_state(
            &pool,
            &newer,
            JobStatus::Processing,
            STALE_PROCESSING_MINUTES + 1,
        )
        .await;
        inspector.requeue_stale().await.expect("the update runs");
        assert_eq!(
            status_of(&inspector, &newer).await,
            JobStatus::Queued,
            "stale"
        );
        assert_eq!(
            status_of(&inspector, &fresh).await,
            JobStatus::Processing,
            "still running, or at least still recent"
        );
    }
}
