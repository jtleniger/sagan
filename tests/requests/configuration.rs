use loco_rs::{app::AppContext, testing::prelude::*};
use sagan::{
    app::App,
    models::users::{self, Model, RegisterParams},
};
use serial_test::serial;

const EMAIL: &str = "config@loco.com";
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
async fn configuration_redirects_to_login_without_cookie() {
    request_with_config::<App, _, _>(session(), |request, _ctx| async move {
        let res = request.get("/configuration").await;

        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/login");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn configuration_renders_the_captures_section() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let res = request.get("/configuration").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        for expected in [
            "Captures",
            // Nothing has been saved, so the page opens on the default interval.
            "Currently: Every hour",
            r#"name="kind""#,
            r#"value="hourly""#,
            r#"href="/configuration""#,
        ] {
            assert!(
                body.contains(expected),
                "expected {expected:?} on the configuration page, got: {body}"
            );
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn saving_an_interval_redirects_and_persists() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let saved = request
            .post("/configuration/captures")
            .form(&[("kind", "every_minutes"), ("minutes", "5")])
            .await;
        assert_eq!(saved.status_code(), 303);
        assert_eq!(saved.header("location"), "/configuration?saved=1");

        let body = request.get("/configuration").await.text();
        assert!(
            body.contains("Currently: Every 5 minutes"),
            "expected the saved interval, got: {body}"
        );
        assert!(
            body.contains(r#"value="5""#),
            "expected the saved minutes in the form, got: {body}"
        );

        let saved = request
            .post("/configuration/captures")
            .form(&[("kind", "daily_at"), ("at", "03:30")])
            .await;
        assert_eq!(saved.status_code(), 303);

        let body = request.get("/configuration").await.text();
        assert!(
            body.contains("Currently: Every day at 03:30"),
            "expected the second saved interval, got: {body}"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_rejected_submission_keeps_what_was_typed() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let rejected = request
            .post("/configuration/captures")
            .form(&[("kind", "every_minutes"), ("minutes", "0")])
            .await;
        assert_eq!(rejected.status_code(), 422);
        let body = rejected.text();
        assert!(
            body.contains("Minutes must be between 1 and 1440."),
            "expected the validation message, got: {body}"
        );
        assert!(
            body.contains(r#"value="0""#),
            "expected the typed value to be kept, got: {body}"
        );

        let body = request.get("/configuration").await.text();
        assert!(
            body.contains("Currently: Every hour"),
            "a rejected submission must not be written, got: {body}"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn an_empty_submission_is_rejected() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let rejected = request
            .post("/configuration/captures")
            .form(&[("kind", ""), ("minutes", "")])
            .await;
        assert_eq!(rejected.status_code(), 422);
        assert!(
            rejected
                .text()
                .contains("Choose how often the camera should capture."),
            "expected the no-radio message"
        );
    })
    .await;
}
