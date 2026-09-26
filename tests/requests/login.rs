use loco_rs::{app::AppContext, testing::prelude::*};
use sagan::{
    app::App,
    models::users::{Model, RegisterParams},
};
use serial_test::serial;

const EMAIL: &str = "test@loco.com";
const PASSWORD: &str = "1234";

/// The request harness drops cookies between calls unless asked to keep them.
fn session() -> RequestConfig {
    RequestConfigBuilder::new().save_cookies(true).build()
}

/// The login form reads real rows, and `config/test.yaml` truncates `users`
/// per boot, so every test creates the account it signs in with.
async fn create_user(ctx: &AppContext) {
    Model::create_with_password(
        &ctx.db,
        &RegisterParams {
            email: EMAIL.to_string(),
            password: PASSWORD.to_string(),
            name: "loco".to_string(),
        },
    )
    .await
    .expect("the test user should be created");
}

#[tokio::test]
#[serial]
async fn can_get_login_form() {
    request_with_config::<App, _, _>(session(), |request, _ctx| async move {
        let res = request.get("/login").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        assert!(
            body.contains(r#"name="email""#),
            "expected the email field, got: {body}"
        );
        assert!(
            body.contains(r#"name="password""#),
            "expected the password field, got: {body}"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn login_with_wrong_password_rerenders_the_form() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;

        let res = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", "wrong")])
            .await;

        assert_eq!(res.status_code(), 401);
        assert!(
            res.text().contains("Invalid email or password"),
            "expected the error message, got: {}",
            res.text()
        );
        assert!(
            res.headers().get("set-cookie").is_none(),
            "a failed login must not hand out a token cookie"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn login_with_valid_password_sets_cookie_and_redirects() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;

        let res = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;

        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/");

        let set_cookie = res.header("set-cookie").to_str().unwrap().to_owned();
        assert!(
            set_cookie.contains("auth_token="),
            "expected the token cookie, got: {set_cookie}"
        );
        assert!(
            set_cookie.to_lowercase().contains("httponly"),
            "the token cookie must not be readable from JS, got: {set_cookie}"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn logout_clears_cookie_and_redirects_to_login() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;

        request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;

        let res = request.post("/logout").await;

        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/login");

        let set_cookie = res.header("set-cookie").to_str().unwrap().to_owned();
        assert!(
            set_cookie.contains("auth_token="),
            "expected the cookie to be removed, got: {set_cookie}"
        );
    })
    .await;
}
