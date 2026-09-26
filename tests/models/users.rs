use insta::assert_debug_snapshot;
use loco_rs::testing::prelude::*;
use sagan::{
    app::App,
    models::users::{self, Model, RegisterParams},
};
use sea_orm::{ActiveModelTrait, ActiveValue};
use serial_test::serial;

macro_rules! configure_insta {
    ($($expr:expr),*) => {
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_suffix("users");
        let _guard = settings.bind_to_scope();
    };
}

/// Every test builds the users it needs: `config/test.yaml` recreates and
/// truncates the database per boot, so no fixture row survives into a test.
fn params(email: &str) -> RegisterParams {
    RegisterParams {
        email: email.to_string(),
        password: "1234".to_string(),
        name: "framework".to_string(),
    }
}

#[tokio::test]
#[serial]
async fn test_can_validate_model() {
    configure_insta!();

    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");

    let invalid_user = users::ActiveModel {
        name: ActiveValue::set("1".to_string()),
        email: ActiveValue::set("invalid-email".to_string()),
        ..Default::default()
    };

    let res = invalid_user.insert(&boot.app_context.db).await;

    assert_debug_snapshot!(res);
}

#[tokio::test]
#[serial]
async fn can_create_with_password() {
    configure_insta!();

    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");

    let params = params("test@framework.com");

    let user = Model::create_with_password(&boot.app_context.db, &params)
        .await
        .expect("a user should be created");

    // Snapshot only the fields this test is about, never the whole `Model`.
    // A whole-model snapshot encodes every column, so adding one field to
    // `users` — the first thing most apps do — fails every such test at once
    // and buries the one real change in a pile of mechanical re-blessing.
    // Anything the snapshot does not cover, assert directly:
    assert_ne!(
        user.password, params.password,
        "the password must be stored hashed, never in the clear"
    );
    assert_debug_snapshot!((user.email, user.name));
}
#[tokio::test]
#[serial]
async fn handle_create_with_password_with_duplicate() {
    configure_insta!();

    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");

    Model::create_with_password(&boot.app_context.db, &params("user1@example.com"))
        .await
        .expect("the first user should be created");

    let new_user =
        Model::create_with_password(&boot.app_context.db, &params("user1@example.com")).await;

    assert_debug_snapshot!(new_user);
}

#[tokio::test]
#[serial]
async fn can_find_by_email() {
    configure_insta!();

    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");

    Model::create_with_password(&boot.app_context.db, &params("user1@example.com"))
        .await
        .expect("a user should be created");

    let existing_user = Model::find_by_email(&boot.app_context.db, "user1@example.com").await;
    let non_existing_user_results =
        Model::find_by_email(&boot.app_context.db, "un@existing-email.com").await;

    // Narrowed on purpose — see `can_create_with_password` above.
    assert_debug_snapshot!(existing_user.map(|user| (user.email, user.name)));
    assert_debug_snapshot!(non_existing_user_results.map(|user| (user.email, user.name)));
}

#[tokio::test]
#[serial]
async fn can_find_by_pid() {
    configure_insta!();

    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");

    let created = Model::create_with_password(&boot.app_context.db, &params("user1@example.com"))
        .await
        .expect("a user should be created");

    let existing_user = Model::find_by_pid(&boot.app_context.db, &created.pid.to_string())
        .await
        .expect("the created user should be found by its pid");
    let non_existing_user_results =
        Model::find_by_pid(&boot.app_context.db, "23232323-2323-2323-2323-232323232323").await;

    // A generated pid is random, so it cannot go in a snapshot: assert the
    // finder round-trips *this* row instead.
    assert_eq!(existing_user.pid, created.pid);
    assert_eq!(existing_user.email, "user1@example.com");

    // Only the failure case is snapshot-able — see `can_create_with_password`.
    assert_debug_snapshot!(non_existing_user_results.map(|user| (user.pid, user.email)));
}
