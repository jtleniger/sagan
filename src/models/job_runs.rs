//! The record of every periodic-job attempt, and the helpers the dispatcher needs.
//!
//! One row per `(job, slot_at)`: the schedule slot the run belongs to, whether it is still
//! running, and what it produced. Three readers depend on the table:
//!
//! - the `/jobs` page, which paginates it newest first;
//! - the due rule, which compares the newest slot for a job with the current one;
//! - the overlap guard, which is the newest `running` row and its `started_at`.
//!
//! The unique `(job, slot_at)` index in `m20260927_231500_job_runs` is what makes [`Model::claim`]
//! atomic: two runners attempting the same slot conflict, and the one that loses inserts
//! nothing.

use chrono::{DateTime, Duration, Utc};
use loco_rs::prelude::*;
use sea_orm::{sea_query::OnConflict, TryInsertResult};

pub use super::_entities::job_runs::{self, ActiveModel, Column, Entity, Model};

/// The `status` of a run that has not finished.
pub const RUNNING: &str = "running";
/// The `status` of a run that returned `Ok`.
pub const SUCCEEDED: &str = "succeeded";
/// The `status` of a run that returned `Err`, or was abandoned.
pub const FAILED: &str = "failed";

/// The `detail` recorded when a `running` row is abandoned because a later tick found it past
/// its job's `stale_after`.
pub const ABANDONED_DETAIL: &str = "abandoned";

/// How much history the table keeps. Terminal rows older than this are dropped by the next
/// dispatch — always keeping the newest row per job, which is the due-state.
pub const RETENTION_DAYS: i64 = 30;

/// A run's lifecycle, as the `status` column stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Succeeded,
    Failed,
}

impl Status {
    /// The value stored in the `status` column.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => RUNNING,
            Self::Succeeded => SUCCEEDED,
            Self::Failed => FAILED,
        }
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    /// `create_table` gives `updated_at` a database default, but no trigger bumps it on
    /// update, so a finished run would keep the insert time. Set it here for the same reason
    /// `app_settings` does.
    async fn before_save<C>(self, _db: &C, insert: bool) -> std::result::Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if !insert && self.updated_at.is_unchanged() {
            let mut this = self;
            this.updated_at = ActiveValue::Set(Utc::now().into());
            Ok(this)
        } else {
            Ok(self)
        }
    }
}

impl Model {
    /// Claims `(job, slot_at)` for this runner, or `None` when another runner already has it.
    ///
    /// The insert carries the unique index; `ON CONFLICT DO NOTHING` turns the second
    /// runner's attempt into zero rows rather than an error, which is what makes the claim
    /// the overlap guard for two processes pointed at one database.
    ///
    /// # Errors
    /// A database error.
    pub async fn claim(
        db: &DatabaseConnection,
        job: &str,
        slot_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> ModelResult<Option<Self>> {
        let slot_at: DateTimeWithTimeZone = slot_at.into();

        let mut conflict = OnConflict::columns([job_runs::Column::Job, job_runs::Column::SlotAt]);
        conflict.do_nothing();

        let inserted = job_runs::Entity::insert(job_runs::ActiveModel {
            job: Set(job.to_string()),
            status: Set(Status::Running.as_str().to_string()),
            slot_at: Set(slot_at),
            started_at: Set(now.into()),
            ..Default::default()
        })
        .on_conflict(conflict)
        .try_insert()
        .exec(db)
        .await?;

        match inserted {
            TryInsertResult::Inserted(_) => Ok(Some(Self::find_by_slot(db, job, slot_at).await?)),
            TryInsertResult::Conflicted | TryInsertResult::Empty => Ok(None),
        }
    }

    /// The row for `(job, slot_at)`, which [`Model::claim`] has just inserted.
    ///
    /// # Errors
    /// A database error, or a missing row (the claim was made and the table changed under it).
    pub async fn find_by_slot(
        db: &DatabaseConnection,
        job: &str,
        slot_at: DateTimeWithTimeZone,
    ) -> ModelResult<Self> {
        job_runs::Entity::find()
            .filter(job_runs::Column::Job.eq(job))
            .filter(job_runs::Column::SlotAt.eq(slot_at))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// The newest run for `job`, whatever its status — the due-state the page and the rule read.
    ///
    /// # Errors
    /// A database error.
    pub async fn newest(db: &DatabaseConnection, job: &str) -> ModelResult<Option<Self>> {
        Ok(job_runs::Entity::find()
            .filter(job_runs::Column::Job.eq(job))
            .order_by_desc(job_runs::Column::SlotAt)
            .order_by_desc(job_runs::Column::Id)
            .one(db)
            .await?)
    }

    /// The newest row for `job` still in [`RUNNING`], if any — the overlap guard.
    ///
    /// # Errors
    /// A database error.
    pub async fn running(db: &DatabaseConnection, job: &str) -> ModelResult<Option<Self>> {
        Ok(job_runs::Entity::find()
            .filter(job_runs::Column::Job.eq(job))
            .filter(job_runs::Column::Status.eq(RUNNING))
            .order_by_desc(job_runs::Column::StartedAt)
            .order_by_desc(job_runs::Column::Id)
            .one(db)
            .await?)
    }

    /// How long this run has been going, as of `now` — negative ages clamp to zero.
    #[must_use]
    pub fn age(&self, now: DateTime<Utc>) -> Duration {
        let started: DateTime<Utc> = self.started_at.with_timezone(&Utc);
        (now - started).max(Duration::zero())
    }

    /// Closes the run: its terminal status, the detail to show, and the finish instant.
    ///
    /// # Errors
    /// A database error, or a row that is gone.
    pub async fn finish(
        db: &DatabaseConnection,
        id: i64,
        status: Status,
        detail: Option<&str>,
        now: DateTime<Utc>,
    ) -> ModelResult<()> {
        let mut row: ActiveModel = job_runs::Entity::find_by_id(id)
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)?
            .into();
        row.status = Set(status.as_str().to_string());
        row.detail = Set(detail.map(ToString::to_string));
        row.finished_at = Set(Some(now.into()));
        row.update(db).await?;
        Ok(())
    }

    /// Marks a `running` row as failed because no tick can account for it any more.
    ///
    /// The slot the run occupied stays consumed: the *next* slot is what retries. Two runners
    /// racing the same work is the failure this avoids, and it costs one interval.
    ///
    /// # Errors
    /// A database error, or a row that is gone.
    pub async fn abandon(db: &DatabaseConnection, id: i64, now: DateTime<Utc>) -> ModelResult<()> {
        Self::finish(db, id, Status::Failed, Some(ABANDONED_DETAIL), now).await
    }

    /// Drops terminal rows older than [`RETENTION_DAYS`], always keeping the newest row of
    /// each job.
    ///
    /// The newest row is what the due rule reads: deleting it would make the job look like it
    /// had never run, so it would fire on the very next tick.
    ///
    /// # Errors
    /// A database error.
    pub async fn prune(db: &DatabaseConnection, now: DateTime<Utc>) -> ModelResult<()> {
        let cutoff: DateTimeWithTimeZone = (now - Duration::days(RETENTION_DAYS)).into();

        let jobs: Vec<String> = job_runs::Entity::find()
            .select_only()
            .column(job_runs::Column::Job)
            .distinct()
            .into_tuple::<String>()
            .all(db)
            .await?;

        let mut keep = Vec::with_capacity(jobs.len());
        for job in &jobs {
            if let Some(newest) = Self::newest(db, job).await? {
                keep.push(newest.id);
            }
        }

        let mut delete = job_runs::Entity::delete_many()
            .filter(job_runs::Column::Status.ne(RUNNING))
            .filter(job_runs::Column::SlotAt.lt(cutoff));
        if !keep.is_empty() {
            delete = delete.filter(job_runs::Column::Id.is_not_in(keep));
        }
        delete.exec(db).await?;
        Ok(())
    }
}
