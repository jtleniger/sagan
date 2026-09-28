//! `job_runs`: the claim, the reads the due rule makes, and the prune.

use chrono::{DateTime, Duration, Utc};
use loco_rs::testing::prelude::*;
use sagan::{
    app::App,
    models::job_runs::{self, Model, Status, ABANDONED_DETAIL, FAILED, RETENTION_DAYS, RUNNING},
};
use sea_orm::{EntityTrait, PaginatorTrait};
use serial_test::serial;

/// A fixed instant, offset per call: no test here depends on the wall clock.
fn at(minutes: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(1_758_000_000, 0).expect("a valid instant")
        + Duration::minutes(minutes)
}

/// A claim that must succeed, for the setup lines of the tests below.
async fn claim(
    db: &sea_orm::DatabaseConnection,
    job: &str,
    slot: DateTime<Utc>,
) -> job_runs::Model {
    Model::claim(db, job, slot, slot)
        .await
        .expect("the claim runs")
        .expect("the slot is free")
}

/// The same `(job, slot_at)` claimed twice inserts one row: the unique index is the claim, and
/// the loser gets nothing rather than an error.
#[tokio::test]
#[serial]
async fn the_same_slot_is_claimed_once() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;

    let first = Model::claim(db, "capture", at(0), at(1))
        .await
        .expect("the claim runs");
    assert!(first.is_some(), "the first runner takes the slot");

    let second = Model::claim(db, "capture", at(0), at(2))
        .await
        .expect("the second claim runs");
    assert!(second.is_none(), "the second runner must get no row");

    let rows = job_runs::Entity::find()
        .all(db)
        .await
        .expect("the table reads");
    assert_eq!(rows.len(), 1, "one slot, one row");
    assert_eq!(rows[0].status, RUNNING);
    assert_eq!(rows[0].started_at.with_timezone(&Utc), at(1));
}

/// The newest read answers for one job only, and by slot rather than by insert order.
#[tokio::test]
#[serial]
async fn newest_reads_the_latest_slot_for_that_job_only() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;

    // Inserted newest-first, so an implementation ordering by id would pick the wrong row.
    claim(db, "capture", at(10)).await;
    claim(db, "capture", at(0)).await;
    claim(db, "sync", at(5)).await;

    let newest = Model::newest(db, "capture")
        .await
        .expect("the read runs")
        .expect("capture has run");
    assert_eq!(newest.slot_at.with_timezone(&Utc), at(10));

    assert!(
        Model::newest(db, "reclaim")
            .await
            .expect("the read runs")
            .is_none(),
        "a job with no runs has no newest row"
    );
}

/// The running guard finds a run still in `running`, and stops finding it once it is closed.
#[tokio::test]
#[serial]
async fn running_finds_a_live_run_and_its_age() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;

    let claimed = claim(db, "capture", at(0)).await;
    assert!(
        Model::running(db, "sync")
            .await
            .expect("the read runs")
            .is_none(),
        "another job's rows are not this job's guard"
    );

    let running = Model::running(db, "capture")
        .await
        .expect("the read runs")
        .expect("the claim left a running row");
    assert_eq!(running.id, claimed.id);
    assert_eq!(running.age(at(3)), Duration::minutes(3));

    Model::finish(db, running.id, Status::Succeeded, Some("ok"), at(1))
        .await
        .expect("the run closes");
    assert!(
        Model::running(db, "capture")
            .await
            .expect("the read runs")
            .is_none(),
        "a finished run is no longer the guard"
    );
}

/// Abandoning a stale run marks it failed with the recorded reason and a finish time.
#[tokio::test]
#[serial]
async fn abandoning_marks_the_run_failed() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;

    let claimed = claim(db, "capture", at(0)).await;
    Model::abandon(db, claimed.id, at(6))
        .await
        .expect("the run is abandoned");

    let row = job_runs::Entity::find_by_id(claimed.id)
        .one(db)
        .await
        .expect("the row reads")
        .expect("the row exists");
    assert_eq!(row.status, FAILED);
    assert_eq!(row.detail.as_deref(), Some(ABANDONED_DETAIL));
    assert_eq!(
        row.finished_at.map(|at| at.with_timezone(&Utc)),
        Some(at(6))
    );
}

/// The prune drops terminal history past retention, but never the newest row of a job: that
/// row is the due-state, and deleting it would make the job fire on the next tick.
#[tokio::test]
#[serial]
async fn prune_drops_old_history_but_keeps_the_newest_row_per_job() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;
    let now = at(0);
    let old_slot = now - Duration::days(RETENTION_DAYS + 1);

    let old = claim(db, "capture", old_slot).await;
    Model::finish(db, old.id, Status::Succeeded, Some("old"), old_slot)
        .await
        .expect("the old run closes");

    // Also past retention, but the newer of capture's two rows.
    let newest = claim(db, "capture", old_slot + Duration::minutes(5)).await;
    Model::finish(
        db,
        newest.id,
        Status::Failed,
        Some("newest"),
        old_slot + Duration::minutes(5),
    )
    .await
    .expect("the newer run closes");

    // Only one row, and it is that job's newest, so it stays even though it is old.
    let only = claim(db, "sync", now - Duration::days(RETENTION_DAYS + 2)).await;
    Model::finish(
        db,
        only.id,
        Status::Succeeded,
        Some("sync"),
        now - Duration::days(2),
    )
    .await
    .expect("the sync run closes");

    Model::prune(db, now).await.expect("the prune runs");

    let remaining: Vec<i64> = job_runs::Entity::find()
        .all(db)
        .await
        .expect("the table reads")
        .iter()
        .map(|row| row.id)
        .collect();
    assert!(
        remaining.contains(&newest.id),
        "the newest capture row is the due-state and must survive"
    );
    assert!(
        remaining.contains(&only.id),
        "a job's only row is its newest row, old or not"
    );
    assert!(
        !remaining.contains(&old.id),
        "an old terminal row that is not the newest is pruned"
    );
    assert_eq!(remaining.len(), 2, "exactly the two kept rows remain");
}

/// A fresh database has no runs, and the newest read says so rather than erroring.
#[tokio::test]
#[serial]
async fn a_fresh_database_has_no_runs() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;

    assert!(Model::newest(db, "capture")
        .await
        .expect("the read runs")
        .is_none());
    assert!(Model::running(db, "capture")
        .await
        .expect("the read runs")
        .is_none());
    assert_eq!(
        job_runs::Entity::find()
            .count(db)
            .await
            .expect("the count runs"),
        0
    );
}
