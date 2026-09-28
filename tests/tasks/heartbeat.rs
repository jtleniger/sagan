//! The heartbeat tick: the two stamps one `heartbeat` run leaves behind.

use loco_rs::{boot::run_task, task, testing::prelude::*};
use sagan::{
    app::App,
    models::runtime_heartbeats::{self, Source},
};
use serial_test::serial;

/// Test mode runs workers inline, so one tick stamps both sources: the task records the
/// scheduler, and the job it enqueues records the worker. In a deployed split, the same two
/// rows are written by two different processes.
#[tokio::test]
#[serial]
async fn one_tick_stamps_the_scheduler_and_the_worker() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let ctx = &boot.app_context;

    assert!(
        run_task::<App>(ctx, Some(&"heartbeat".to_string()), &task::Vars::default())
            .await
            .is_ok()
    );

    for source in Source::ALL {
        let stamp = runtime_heartbeats::Model::latest(&ctx.db, source)
            .await
            .expect("the stamps read")
            .unwrap_or_else(|| panic!("{} should have stamped", source.as_str()));
        assert_eq!(stamp.source, source.as_str());
        assert_eq!(stamp.pid, i64::from(std::process::id()));
    }
}
