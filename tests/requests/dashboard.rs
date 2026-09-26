use loco_rs::{app::AppContext, prelude::cookie, testing::prelude::*};
use sagan::{
    app::App,
    models::users::{self, Model, RegisterParams},
};
use serial_test::serial;

const EMAIL: &str = "test@loco.com";
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
async fn dashboard_redirects_to_login_without_cookie() {
    request_with_config::<App, _, _>(session(), |request, _ctx| async move {
        let res = request.get("/").await;

        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/login");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn dashboard_renders_the_signed_in_user() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        let user = create_user(&ctx).await;

        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let res = request.get("/").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        assert!(
            body.contains(EMAIL),
            "expected the signed-in user's email, got: {body}"
        );
        assert!(
            body.contains(&user.pid.to_string()),
            "expected the signed-in user's pid, got: {body}"
        );
        assert!(
            body.contains(r#"href="/""#),
            "expected the dashboard nav link, got: {body}"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn dashboard_rejects_a_garbage_cookie() {
    request_with_config::<App, _, _>(session(), |request, _ctx| async move {
        let res = request
            .get("/")
            .add_cookie(cookie::Cookie::new("auth_token", "garbage"))
            .await;

        // Loco's optional JWT extraction never rejects: a forged token is
        // indistinguishable from no token here.
        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/login");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn dashboard_redirects_to_login_after_logout() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;

        request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        let logout = request.post("/logout").await;
        assert_eq!(logout.status_code(), 303);

        let res = request.get("/").await;

        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/login");
    })
    .await;
}
