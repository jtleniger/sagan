use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

/// The liveness stamps the scheduler and the worker write.
///
/// One row per stamp, not one row per source: the history is what makes a *missed* tick
/// visible later (a gap between consecutive `created_at`s), and the page only ever reads
/// the newest row per source. `created_at` is the stamp time — `create_table` adds it, so
/// there is no second timestamp to keep in step.
///
/// No index: the table holds one row per source per minute, and
/// `crate::models::runtime_heartbeat` prunes anything older than its retention window on
/// every insert, so both the read and the prune scan a few thousand rows at most.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "runtime_heartbeats",
            &[
                ("id", ColType::PkAuto),
                ("source", ColType::String),
                ("host", ColType::String),
                ("pid", ColType::BigInteger),
            ],
            &[],
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "runtime_heartbeats").await
    }
}
