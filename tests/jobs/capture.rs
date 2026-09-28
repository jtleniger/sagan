//! The capture job against the real filesystem: what it writes, and what it reports.

use std::path::Path;

use loco_rs::testing::prelude::*;
use sagan::{
    app::App,
    jobs::{self, PeriodicJob},
};
use serial_test::serial;

/// Where `config/test.yaml` points `settings.capture.dir`; the test resolves the same
/// relative path against the same process CWD, so the two cannot disagree.
const CAPTURE_DIR: &str = "target/test-captures";

#[tokio::test]
#[serial]
async fn a_capture_writes_one_jpeg_to_the_capture_directory() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    // The directory survives between runs; start from an empty one so "one capture, one file"
    // means what it says.
    let _ = std::fs::remove_dir_all(CAPTURE_DIR);

    let detail = jobs::CAPTURE
        .run(&boot.app_context)
        .await
        .expect("the mock camera writes to the configured directory");

    let names: Vec<String> = std::fs::read_dir(CAPTURE_DIR)
        .expect("the capture directory exists")
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

    // The detail the dispatch records names the file it wrote.
    assert_eq!(detail, *name);
}
