//! The heartbeat: what it enqueues, proved through the worker it enqueues.

use loco_rs::{boot::run_task, task, testing::prelude::*};
use sagan::{
    app::App,
    captures::{CaptureInterval, CaptureSettings},
    models::app_settings::Model,
};
use serial_test::serial;

/// The directory `config/test.yaml` names for the file store.
const CAPTURE_DIR: &str = "target/test-captures";

#[tokio::test]
#[serial]
async fn a_due_interval_enqueues_one_capture() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let ctx = &boot.app_context;
    let _ = std::fs::remove_dir_all(CAPTURE_DIR);

    // Every minute from midnight: due in whatever minute the test runs in, so the
    // heartbeat's decision needs no clock of its own.
    Model::save_capture_settings(
        &ctx.db,
        &CaptureSettings {
            interval: CaptureInterval::EveryMinutes { minutes: 1 },
        },
    )
    .await
    .expect("the captures section is saved");

    assert!(run_task::<App>(
        ctx,
        Some(&"enqueue_capture".to_string()),
        &task::Vars::default()
    )
    .await
    .is_ok());

    let count = std::fs::read_dir(CAPTURE_DIR)
        .expect("the store's directory exists")
        .count();
    assert_eq!(count, 1, "the due heartbeat enqueued exactly one capture");
}
