use loco_rs::{app::AppContext, prelude::cookie, testing::prelude::*};
use sagan::{
    app::App,
    camera_lock::CameraLock,
    models::users::{self, Model, RegisterParams},
};
use serial_test::serial;

const EMAIL: &str = "test@loco.com";
const PASSWORD: &str = "1234";
/// Where `config/test.yaml` points `settings.live.dir`; the test resolves the same relative
/// path against the same process CWD, so the two cannot disagree.
const LIVE_DIR: &str = "target/test-live";

/// The request harness drops cookies between calls unless asked to keep them.
fn session() -> RequestConfig {
    RequestConfigBuilder::new().save_cookies(true).build()
}

/// The frame files in [`LIVE_DIR`]: `<digits>.jpg` and nothing else — no `<ms>.partial.jpg`, no
/// lock file, no scratch.
fn live_frames() -> Vec<String> {
    std::fs::read_dir(LIVE_DIR)
        .expect("the live directory exists")
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.strip_suffix(".jpg")
                .is_some_and(|stem| stem.chars().all(|c| c.is_ascii_digit()))
        })
        .collect()
}

/// The `at=` cache-buster the Live card's `<img>` names.
fn live_at(body: &str) -> String {
    let marker = r#"src="/live/image?at="#;
    let start = body
        .find(marker)
        .expect("the card should name the frame it shows")
        + marker.len();
    let end = start
        + body[start..]
            .find('"')
            .expect("the src attribute should be closed");
    body[start..end].to_string()
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
        create_user(&ctx).await;
        // Start from no live frame so the card is deterministic: the placeholder, not whatever a
        // previous test or a running dev server left behind.
        let _ = std::fs::remove_dir_all(LIVE_DIR);

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
            body.contains(r#"href="/""#),
            "expected the dashboard nav link, got: {body}"
        );
        for expected in [
            // Live: the poll target, and the placeholder with no frame yet.
            r#"id="live-view""#,
            r#"hx-get="/live""#,
            r#"hx-trigger="every 15s""#,
            r#"src="/static/img/no-capture-available.svg""#,
            "Waiting for the first frame from the camera.",
            // Status: name and badge text for the tiles the controller supplies.
            "14 captures today",
            // Disk: the live reading from the shared monitor. The volume the test runner sits
            // on is not fixed, so only the shape of the line is asserted here.
            "free of ",
            "Last backed up captures at 2026-09-26 03:00:00 UTC",
            "Last reclaimed space at 2026-09-27 04:30:00 UTC; 1.2 GB freed",
        ] {
            assert!(
                body.contains(expected),
                "expected {expected:?} on the dashboard, got: {body}"
            );
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn live_redirects_the_poller_without_cookie() {
    request_with_config::<App, _, _>(session(), |request, _ctx| async move {
        let res = request.get("/live").await;

        assert_eq!(res.status_code(), 401);
        // htmx follows this header instead of swapping an error body in.
        assert_eq!(res.header("hx-redirect"), "/login");
        assert_eq!(res.text(), "");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn live_image_is_unauthorized_without_cookie() {
    request_with_config::<App, _, _>(session(), |request, _ctx| async move {
        let res = request.get("/live/image").await;

        assert_eq!(res.status_code(), 401);
        assert_eq!(res.text(), "");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn live_captures_a_frame_and_serves_it() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let _ = std::fs::remove_dir_all(LIVE_DIR);

        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let res = request.get("/live").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        for expected in [
            r#"id="live-view""#,
            r#"hx-get="/live""#,
            r#"hx-trigger="every 15s""#,
            r#"src="/live/image?at="#,
        ] {
            assert!(
                body.contains(expected),
                "expected {expected:?} in the polled fragment, got: {body}"
            );
        }
        assert_eq!(live_frames().len(), 1, "one poll, one frame");

        // The bytes the card's `<img>` fetches are the mock bundle's frame: the capture went
        // through the `Camera` trait, was written to disk and is served back.
        let res = request.get("/live/image").await;
        assert_eq!(res.status_code(), 200);
        let content_type = res
            .header("content-type")
            .to_str()
            .expect("the content type should be ascii")
            .to_string();
        assert!(
            content_type.starts_with("image/jpeg"),
            "expected a jpeg, got: {content_type}"
        );
        let bytes = res.as_bytes();
        assert_eq!(bytes.len(), 350, "the mock camera's 64x48 frame");
        assert!(bytes.starts_with(&[0xFF, 0xD8]), "a JPEG starts with SOI");
        assert!(bytes.ends_with(&[0xFF, 0xD9]), "and ends with EOI");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn live_reuses_a_frame_younger_than_the_refresh_interval() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let _ = std::fs::remove_dir_all(LIVE_DIR);

        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        let first = request.get("/live").await;
        assert_eq!(first.status_code(), 200);
        let at = live_at(&first.text());

        let second = request.get("/live").await;
        assert_eq!(second.status_code(), 200);
        assert_eq!(
            live_at(&second.text()),
            at,
            "a frame younger than REFRESH_MS is reused, not captured again"
        );
        assert_eq!(live_frames().len(), 1, "no second frame was written");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn live_waits_for_the_camera_lock() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let _ = std::fs::remove_dir_all(LIVE_DIR);

        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);

        // Another camera user — the scheduler's child, in production — holds the lock: a poll
        // must wait rather than frame over it.
        let held = CameraLock::acquire()
            .await
            .expect("the test takes the camera lock");
        let blocked =
            tokio::time::timeout(std::time::Duration::from_millis(300), request.get("/live")).await;
        assert!(
            blocked.is_err(),
            "a poll must not answer while another process frames"
        );

        drop(held);
        let res = request.get("/live").await;
        assert_eq!(res.status_code(), 200);
        assert_eq!(
            live_frames().len(),
            1,
            "with the lock released, the poll frames"
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
