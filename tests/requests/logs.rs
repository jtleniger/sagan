use loco_rs::{app::AppContext, testing::prelude::*};
use sagan::{
    app::App,
    models::users::{self, Model, RegisterParams},
};
use serial_test::serial;

const EMAIL: &str = "test@loco.com";
const PASSWORD: &str = "1234";

/// Where `config/test.yaml` points `logger.file_appender.dir`; the reader resolves the
/// same relative path against the same process CWD, so the two cannot disagree.
const LOG_DIR: &str = "target/test-logs";
const LOG_FILE: &str = "sagan.2026-09-26.log";

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

/// Replaces the log directory with one file holding exactly these lines. Every test that
/// uses it is `#[serial]`, so no two of them share the directory at once.
fn write_fixtures(lines: &[String]) {
    let dir = std::path::Path::new(LOG_DIR);
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("the fixture dir should be creatable");
    let mut body = lines.join("\n");
    body.push('\n');
    std::fs::write(dir.join(LOG_FILE), body).expect("the fixture file should be writable");
}

fn entry(ts: &str, level: &str, message: &str) -> String {
    format!(
        r#"{{"timestamp":"{ts}","level":"{level}","fields":{{"message":"{message}"}},"target":"sagan::test"}}"#
    )
}

/// The three records every filter test works from, oldest first.
fn fixtures() -> Vec<String> {
    vec![
        entry("2026-09-26T10:00:00.000000Z", "INFO", "boot ok"),
        entry("2026-09-26T10:01:00.000000Z", "WARN", "disk almost full"),
        entry("2026-09-26T10:02:00.000000Z", "ERROR", "db gone"),
    ]
}

#[tokio::test]
#[serial]
async fn logs_redirects_to_login_without_cookie() {
    request_with_config::<App, _, _>(session(), |request, _ctx| async move {
        let res = request.get("/logs").await;

        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/login");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn logs_renders_entries_and_filters_by_level() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);
        write_fixtures(&fixtures());

        let res = request.get("/logs").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        for expected in [
            "boot ok",
            "disk almost full",
            "db gone",
            r#"href="/logs""#,
            "Page 1 of 1",
            "2026-09-26 10:02:00.000",
            // What the local-time script reads, and the script itself.
            r#"<time datetime="2026-09-26T10:02:00.000Z" data-local-time>"#,
            r#"src="/static/js/logs.js""#,
        ] {
            assert!(
                body.contains(expected),
                "expected {expected:?} in the rendered page, got: {body}"
            );
        }

        // Newest first: the error's row precedes the boot record's.
        let newest = body
            .find("db gone")
            .expect("the newest entry should be rendered");
        let oldest = body
            .find("boot ok")
            .expect("the oldest entry should be rendered");
        assert!(newest < oldest, "entries were not newest first: {body}");

        let filtered = request.get("/logs?level=error").await;
        assert_eq!(filtered.status_code(), 200);
        let filtered = filtered.text();
        assert!(
            filtered.contains("db gone"),
            "expected the error entry, got: {filtered}"
        );
        assert!(
            !filtered.contains("boot ok"),
            "the info entry should have been filtered out, got: {filtered}"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn logs_filters_by_time_range() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);
        write_fixtures(&fixtures());

        // Both ends name whole seconds, and both include what they name.
        let res = request
            .get("/logs?from=2026-09-26T10:01:00&to=2026-09-26T10:01:00")
            .await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        assert!(
            body.contains("disk almost full"),
            "expected the entry inside the range, got: {body}"
        );
        assert!(
            !body.contains("boot ok") && !body.contains("db gone"),
            "expected the entries outside the range to be filtered out, got: {body}"
        );
        assert!(
            body.contains(r#"value="2026-09-26T10:01:00""#),
            "expected the from input to echo the submitted value, got: {body}"
        );
        // The script reads `data-utc` to show that same instant locally, and writes the
        // UTC back into the hidden twin on submit.
        assert!(
            body.contains(r#"id="logs-from" name="from" step="1""#)
                && body.contains(r#"data-utc="2026-09-26T10:01:00""#)
                && body.contains(
                    r#"id="logs-from-utc" name="from" value="2026-09-26T10:01:00" disabled"#
                ),
            "expected the local-time boundary contract, got: {body}"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn logs_ignores_an_unreadable_filter() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);
        write_fixtures(&fixtures());

        // A hand-edited link with a level and a date the reader cannot use still lists the
        // logs, rather than erroring or showing an empty page.
        let res = request.get("/logs?level=bogus&from=yesterday").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        assert!(
            body.contains("boot ok") && body.contains("db gone"),
            "expected every entry, got: {body}"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn logs_empty_state_without_entries() {
    request_with_config::<App, _, _>(session(), |request, ctx| async move {
        create_user(&ctx).await;
        let login = request
            .post("/login")
            .form(&[("email", EMAIL), ("password", PASSWORD)])
            .await;
        assert_eq!(login.status_code(), 303);
        let _ = std::fs::remove_dir_all(LOG_DIR);

        let res = request.get("/logs").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        assert!(
            body.contains("No log entries match these filters."),
            "expected the empty state, got: {body}"
        );
        assert!(
            body.contains(LOG_DIR),
            "expected the empty state to name the directory, got: {body}"
        );
    })
    .await;
}
