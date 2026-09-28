//! The liveness stamps: what the scheduler and the worker write, and what a reader sees.

use chrono::Utc;
use loco_rs::testing::prelude::*;
use sagan::{
    app::App,
    models::runtime_heartbeats::{self, Source},
};
use sea_orm::{DatabaseConnection, EntityTrait};
use serial_test::serial;

/// A row aged `hours` back, so retention can be tested without waiting a day.
async fn backdate(db: &DatabaseConnection, id: i64, hours: i64) {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    let mut row: runtime_heartbeats::ActiveModel = runtime_heartbeats::Entity::find_by_id(id)
        .one(db)
        .await
        .expect("the row reads")
        .expect("the row exists")
        .into();
    row.created_at = ActiveValue::Set((Utc::now() - chrono::Duration::hours(hours)).into());
    row.update(db).await.expect("the row is backdated");
}

/// Nothing has stamped in a fresh database, so every source reads as missing rather than
/// as an error.
#[tokio::test]
#[serial]
async fn a_fresh_database_has_no_stamps() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;

    assert!(runtime_heartbeats::Model::latest(db, Source::Scheduler)
        .await
        .expect("an empty table is not an error")
        .is_none());
    assert_eq!(
        runtime_heartbeats::Model::seen(db)
            .await
            .expect("every source reads")
            .len(),
        Source::ALL.len()
    );
}

/// A stamp says which source, which host, which pid — and the newest one is what a reader
/// gets, even when two land in the same second.
#[tokio::test]
#[serial]
async fn a_stamp_records_the_process_that_wrote_it() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;

    let first = runtime_heartbeats::Model::stamp(db, Source::Scheduler)
        .await
        .expect("the scheduler stamps");
    assert_eq!(first.source, "scheduler");
    assert_eq!(first.pid, i64::from(std::process::id()));
    assert!(!first.host.is_empty(), "a stamp says where it came from");

    let second = runtime_heartbeats::Model::stamp(db, Source::Scheduler)
        .await
        .expect("the scheduler stamps again");
    assert!(second.id > first.id, "a stamp is a new row, not an update");

    let latest = runtime_heartbeats::Model::latest(db, Source::Scheduler)
        .await
        .expect("the newest stamp reads")
        .expect("a stamp exists");
    assert_eq!(latest.id, second.id, "the newest stamp is the one returned");

    // The worker's row is its own: stamping the scheduler must not answer for the worker.
    assert!(runtime_heartbeats::Model::latest(db, Source::Worker)
        .await
        .expect("the worker's row reads")
        .is_none());
}

/// History is bounded: a stamp older than the retention window is dropped by the next
/// stamp, so the table cannot grow without limit.
#[tokio::test]
#[serial]
async fn stamps_older_than_the_retention_window_are_pruned() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;

    let old = runtime_heartbeats::Model::stamp(db, Source::Worker)
        .await
        .expect("the worker stamps");
    backdate(db, old.id, runtime_heartbeats::RETENTION_HOURS + 1).await;

    let recent = runtime_heartbeats::Model::stamp(db, Source::Worker)
        .await
        .expect("the worker stamps again");
    backdate(db, recent.id, runtime_heartbeats::RETENTION_HOURS - 1).await;

    // The next stamp is what prunes, so write one more.
    let newest = runtime_heartbeats::Model::stamp(db, Source::Worker)
        .await
        .expect("the worker stamps a third time");

    let ids: Vec<i64> = runtime_heartbeats::Entity::find()
        .all(db)
        .await
        .expect("the history reads")
        .iter()
        .map(|row| row.id)
        .collect();
    assert!(
        !ids.contains(&old.id),
        "the stamp past the window should be gone, left: {ids:?}"
    );
    assert!(ids.contains(&recent.id), "history inside the window stays");
    assert!(ids.contains(&newest.id));
}
