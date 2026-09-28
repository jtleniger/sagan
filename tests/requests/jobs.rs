//! `/jobs` — who the page admits to, and what it says about the workers.

use loco_rs::{app::AppContext, testing::prelude::*};
use sagan::{
    app::App,
    models::users::{self, Model, RegisterParams},
};
use serial_test::serial;

const EMAIL: &str = "jobs@loco.com";
const PASSWORD: &str = "1234";

/// The request harness drops cookies between calls unless asked to keep them.
fn session() -> RequestConfig {
    RequestConfigBuilder::new().save_cookies(true).build()
}

async fn create_user(ctx: &AppContext) -> users::Model {
    Model::create_with_password(
        &ctx.db,
        &RegisterParams {
            email: EMAIL.to_string(),
            password: PASSWORD.to_string(),
            name: "loco".to_string(),
        },
    )
    .await
    .expect("the test user should be created")
}

#[tokio::test]
#[serial]
async fn jobs_redirects_to_login_without_cookie() {
    request_with_config::<App, _, _>(session(), |request, _ctx| async move {
        let res = request.get("/jobs").await;

        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/login");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn jobs_lists_the_configured_workers() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let res = request.get("/jobs").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        for expected in [
            // The shell link, and the one row this app's worker list produces.
            r#"href="/jobs""#,
            "CaptureWorker",
            "—",
            "Takes a still image from the camera and stores it in the file store.",
            // Test mode is ForegroundBlocking, so there is no provider to ping and no
            // queue to list or act on.
            "ForegroundBlocking",
            "None configured for ForegroundBlocking; this mode keeps no queue.",
            "This worker mode keeps no queue, so there are no job rows to show.",
            // Test mode's config has no `scheduler:` block, so the page says so — the
            // entries are read from the deployed file, not from the database.
            "Scheduled",
            "This application schedules no recurring work.",
            // The heartbeat worker is registered, and nothing has stamped in a fresh test
            // database, so both runtime rows read as missing rather than as an error.
            "HeartbeatWorker",
            "Records that a worker process drained the queue, once a minute.",
            "Runtime",
            "never (expects one every 60 s)",
        ] {
            assert!(
                body.contains(expected),
                "expected {expected:?} on the jobs page, got: {body}"
            );
        }

        // No queue means nothing to act on: no recovery button, and no row verbs.
        assert!(
            !body.contains("/jobs/requeue"),
            "a mode with no queue must offer no requeue button: {body}"
        );
        assert!(
            !body.contains(">Cancel<") && !body.contains(">Retry<"),
            "a mode with no queue must offer no row buttons: {body}"
        );
    })
    .await;
}

/// The three actions are POSTs on a page a visitor can reach: each answers the way the
/// pages do (to the form, not a 401 body), and each refuses to act when there is no queue.
#[tokio::test]
#[serial]
async fn job_actions_need_a_session_and_a_queue() {
    for action in [
        "/jobs/01M3JQUEUED000000000000001/cancel",
        "/jobs/01M3JFAILED000000000000002/retry",
        "/jobs/requeue",
    ] {
        request_with_config::<App, _, _>(session(), |request, _ctx| async move {
            let res = request.post(action).await;

            assert_eq!(res.status_code(), 303, "{action} without a cookie");
            assert_eq!(res.header("location"), "/login", "{action}");
        })
        .await;
    }

    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        // Signed in, but test mode keeps no queue: a hand-made POST is a client error, not
        // a 500 and not a silent success.
        for action in [
            "/jobs/01M3JQUEUED000000000000001/cancel",
            "/jobs/01M3JFAILED000000000000002/retry",
            "/jobs/requeue",
        ] {
            let res = request.post(action).await;
            assert_eq!(res.status_code(), 400, "{action} with no queue");
        }
    })
    .await;
}
