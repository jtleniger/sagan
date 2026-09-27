use loco_rs::{app::AppContext, testing::prelude::*};
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
async fn system_redirects_to_login_without_cookie() {
    request_with_config::<App, _, _>(session(), |request, _ctx| async move {
        let res = request.get("/system").await;

        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/login");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn metrics_redirects_the_poller_without_cookie() {
    request_with_config::<App, _, _>(session(), |request, _ctx| async move {
        let res = request.get("/system/metrics").await;

        assert_eq!(res.status_code(), 401);
        // htmx follows this header instead of swapping an error body in.
        assert_eq!(res.header("hx-redirect"), "/login");
        assert_eq!(res.text(), "");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn system_renders_the_metrics_page() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let res = request.get("/system").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        assert!(
            body.contains(r#"id="system-live""#),
            "expected the polled panel, got: {body}"
        );
        assert!(
            body.contains(r#"hx-get="/system/metrics""#),
            "expected the poll target, got: {body}"
        );
        assert!(
            body.contains(r#"href="/system""#),
            "expected the system nav link, got: {body}"
        );
        assert!(
            body.contains(r#"data-sample=""#),
            "expected the chart payload attribute, got: {body}"
        );
    })
    .await;
}

/// The `data-sample="…"` attribute value, HTML-unescaped the way a browser decodes it, parsed
/// back into the JSON the charts and the Disk card see.
fn data_sample(body: &str) -> serde_json::Value {
    let marker = r#"data-sample=""#;
    let start = body
        .find(marker)
        .expect("the panel should carry the chart payload")
        + marker.len();
    let end = start
        + body[start..]
            .find('"')
            .expect("the payload attribute should be closed");

    let unescaped = body[start..end]
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&amp;", "&");

    serde_json::from_str(&unescaped).expect("the payload should be valid json")
}

#[tokio::test]
#[serial]
async fn metrics_returns_a_pollable_fragment() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let res = request.get("/system/metrics").await;

        assert_eq!(res.status_code(), 200);
        let content_type = res
            .header("content-type")
            .to_str()
            .expect("the content type should be ascii")
            .to_string();
        assert!(
            content_type.starts_with("text/html"),
            "expected an html fragment, got: {content_type}"
        );
        let body = res.text();
        assert!(
            body.contains(r#"id="system-live""#),
            "expected the panel to carry the poll trigger again, got: {body}"
        );
        assert!(
            body.contains(r#"hx-trigger="every 2s""#),
            "expected the next poll's trigger, got: {body}"
        );

        // The panel's heading is not the reading: the disk must be in the payload the page
        // carries, measured from this host.
        let payload = data_sample(&body);
        let total = payload["disk"]["total_bytes"]
            .as_u64()
            .expect("the sample should carry the disk it runs on");
        assert!(
            total > 0,
            "the disk reading should be a real one: {payload}"
        );
        assert!(
            payload["disk"]["available_bytes"]
                .as_u64()
                .expect("free space should be a number")
                <= total,
            "free space cannot exceed the volume's size: {payload}"
        );
    })
    .await;
}
