use loco_rs::testing::prelude::*;
use sagan::{
    app::App,
    captures::{CaptureInterval, CaptureSettings, SECTION},
    models::app_settings,
};
use sea_orm::EntityTrait;
use serial_test::serial;

/// A freshly migrated database has no Captures row: the section reads as its default, and nothing
/// has been written under it yet.
#[tokio::test]
#[serial]
async fn an_unsaved_section_reads_as_the_default_interval() {
    let boot = boot_test::<App>().await.expect("the app should boot");

    assert!(
        app_settings::Model::find_section(&boot.app_context.db, SECTION)
            .await
            .expect("a missing section is not an error")
            .is_none()
    );
    assert_eq!(
        app_settings::Model::capture_settings(&boot.app_context.db)
            .await
            .expect("an unsaved section reads as the default"),
        CaptureSettings::default()
    );
}

/// The second save updates the row the first one inserted — one row per section.
#[tokio::test]
#[serial]
async fn a_saved_section_round_trips() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;

    app_settings::Model::save_capture_settings(
        db,
        &CaptureSettings {
            interval: CaptureInterval::EveryMinutes { minutes: 5 },
        },
    )
    .await
    .expect("the section should save");

    assert_eq!(
        app_settings::Model::capture_settings(db)
            .await
            .expect("the section should read back"),
        CaptureSettings {
            interval: CaptureInterval::EveryMinutes { minutes: 5 },
        }
    );

    app_settings::Model::save_capture_settings(
        db,
        &CaptureSettings {
            interval: CaptureInterval::Hourly,
        },
    )
    .await
    .expect("the section should save again");

    assert_eq!(
        app_settings::Model::capture_settings(db)
            .await
            .expect("the section should read back"),
        CaptureSettings {
            interval: CaptureInterval::Hourly,
        }
    );
    assert_eq!(
        app_settings::Entity::find()
            .all(db)
            .await
            .expect("the table should be queryable")
            .len(),
        1,
        "the second save must update the row, not insert another"
    );
}

/// A payload this build cannot read is an error, not a silent reset.
#[tokio::test]
#[serial]
async fn a_payload_that_cannot_be_read_is_an_error() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let db = &boot.app_context.db;

    app_settings::Model::save_section(db, SECTION, &"not an object")
        .await
        .expect("any JSON value can be stored");

    assert!(
        app_settings::Model::capture_settings(db).await.is_err(),
        "a stored payload that is not CaptureSettings must fail loudly"
    );
}
