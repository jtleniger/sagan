//! `/jobs` — who the page admits to, and what it says about the periodic jobs.

use chrono::{Duration, Utc};
use loco_rs::{app::AppContext, testing::prelude::*};
use sagan::{
    app::App,
    models::{
        job_runs::{self, Model, Status},
        users::{self, RegisterParams},
    },
};
use sea_orm::{EntityTrait, PaginatorTrait};
use serial_test::serial;

const EMAIL: &str = "jobs@loco.com";
const PASSWORD: &str = "1234";

/// The request harness drops cookies between calls unless asked to keep them.
fn session() -> RequestConfig {
    RequestConfigBuilder::new().save_cookies(true).build()
}

async fn create_user(ctx: &AppContext) -> users::Model {
    users::Model::create_with_password(
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

/// One finished run for `capture`, so the history table has a row to render.
async fn seed_run(ctx: &AppContext, detail: &str) {
    let now = Utc::now();
    let run = Model::claim(&ctx.db, "capture", now, now)
        .await
        .expect("the claim runs")
        .expect("the slot is free");
    Model::finish(&ctx.db, run.id, Status::Succeeded, Some(detail), now)
        .await
        .expect("the run closes");
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
async fn jobs_lists_the_registered_job_and_a_recorded_run() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        seed_run(&ctx, "1790557201099.jpg (350 bytes)").await;

        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let res = request.get("/jobs").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        for expected in [
            // The shell link, and the one job this app registers.
            r#"href="/jobs""#,
            "Periodic jobs",
            "capture",
            "Takes a still image from the camera and stores it in the file store.",
            // A fresh database has no Captures row, so the interval is the default.
            "Every hour",
            ">succeeded<",
            // The run history, with the detail the run recorded.
            "Runs",
            "1790557201099.jpg (350 bytes)",
            "Page 1 of 1",
            // The page loads the timestamp script, so it has to keep the base's own head.
            "https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4",
        ] {
            assert!(
                body.contains(expected),
                "expected {expected:?} on the jobs page, got: {body}"
            );
        }
    })
    .await;
}

/// More runs than fit on a page: the pager links move between them, and each page reports the
/// range it is showing.
#[tokio::test]
#[serial]
async fn pagination_moves_between_pages() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;

        // 26 runs, one per minute, so the history needs two pages of 25.
        let base = Utc::now();
        for minute in 0..26 {
            let slot = base - Duration::minutes(minute);
            let run = Model::claim(&ctx.db, "capture", slot, slot)
                .await
                .expect("the claim runs")
                .expect("each minute is a fresh slot");
            Model::finish(&ctx.db, run.id, Status::Succeeded, Some("ok"), slot)
                .await
                .expect("the run closes");
        }

        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let first = request.get("/jobs").await;
        assert_eq!(first.status_code(), 200);
        let body = first.text();
        assert!(body.contains("Page 1 of 2"), "{body}");
        assert!(body.contains("26 runs"), "{body}");
        assert!(
            body.contains("/jobs?page=2"),
            "a next link is offered: {body}"
        );
        assert!(
            !body.contains("/jobs?page=1"),
            "the first page has no previous link: {body}"
        );

        let second = request.get("/jobs?page=2").await;
        assert_eq!(second.status_code(), 200);
        let body = second.text();
        assert!(body.contains("Page 2 of 2"), "{body}");
        assert!(
            body.contains("/jobs?page=1"),
            "a previous link is offered: {body}"
        );

        assert_eq!(
            job_runs::Entity::find()
                .count(&ctx.db)
                .await
                .expect("the count runs"),
            26
        );
    })
    .await;
}
