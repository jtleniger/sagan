use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

/// One row per attempt at one schedule slot of one job.
///
/// The table is the single source of truth for three readers: the run history the `/jobs`
/// page paginates, the "what ran last" state the due rule compares its latest slot against,
/// and the overlap guard (a `running` row with a fresh `started_at`).
///
/// The unique `(job, slot_at)` index is the claim: two runners attempting the same slot
/// conflict and the loser skips. The same index serves "newest row for a job"
/// (`ORDER BY slot_at DESC LIMIT 1`).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "job_runs",
            &[
                ("id", ColType::PkAuto),
                ("job", ColType::String),
                ("status", ColType::String),
                ("slot_at", ColType::TimestampWithTimeZone),
                ("started_at", ColType::TimestampWithTimeZone),
                ("finished_at", ColType::TimestampWithTimeZoneNull),
                ("detail", ColType::TextNull),
            ],
            &[],
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx-job_runs-job-slot")
                .table(Alias::new("job_runs"))
                .col(Alias::new("job"))
                .col(Alias::new("slot_at"))
                .unique()
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "job_runs").await
    }
}
