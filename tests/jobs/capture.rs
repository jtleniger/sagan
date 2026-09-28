//! The capture job against the real file store: what it writes, and what it reports.

use std::path::Path;

use loco_rs::testing::prelude::*;
use sagan::{
    app::App,
    jobs::{self, PeriodicJob},
};
use serial_test::serial;

/// Where `config/test.yaml` points `settings.storage.dir`; the test resolves the same
/// relative path against the same process CWD, so the two cannot disagree.
const CAPTURE_DIR: &str = "target/test-captures";

#[tokio::test]
#[serial]
async fn a_capture_lands_in_the_file_store() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let ctx = &boot.app_context;
    // The store's root survives between runs; start from an empty one so "one capture,
    // one file" means what it says.
    let _ = std::fs::remove_dir_all(CAPTURE_DIR);

    let detail = jobs::CAPTURE
        .run(ctx)
        .await
        .expect("the mock camera frames and the store accepts the write");

    let names: Vec<String> = std::fs::read_dir(CAPTURE_DIR)
        .expect("the store's directory exists")
        .map(|entry| {
            entry
                .expect("a readable entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names.len(), 1, "one capture, one file: {names:?}");

    let name = &names[0];
    assert_eq!(
        Path::new(name)
            .extension()
            .and_then(std::ffi::OsStr::to_str),
        Some("jpg"),
        "the capture is a jpeg: {name}"
    );
    let bytes = std::fs::read(Path::new(CAPTURE_DIR).join(name)).expect("the capture is readable");
    assert_eq!(bytes.len(), 350, "the mock camera's 64x48 frame");
    assert!(bytes.starts_with(&[0xFF, 0xD8]), "a JPEG starts with SOI");
    assert!(bytes.ends_with(&[0xFF, 0xD9]), "and ends with EOI");

    // The store the app holds resolves the same key — not the null driver, which fails
    // every write.
    assert!(
        ctx.storage
            .exists(Path::new(name))
            .await
            .expect("the store answers"),
        "the app's file store should see {name}"
    );

    // The detail the dispatch records names the key and its size.
    assert_eq!(detail, format!("{name} ({} bytes)", bytes.len()));
}
