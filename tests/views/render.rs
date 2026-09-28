use fluent_templates::{ArcLoader, FluentLoader};
use loco_rs::{
    controller::views::{engines, ViewRenderer},
    prelude::data,
};

/// Builds the view engine exactly as `ViewEngineInitializer` does at boot:
/// same template directory, same i18n `t()` function registration.
///
/// Tera resolves `{% extends %}` and function references when a template is
/// added, so this also proves the `base.html → auth/login.html` chain loads.
fn engine() -> engines::TeraView {
    let loader = std::sync::Arc::new(
        ArcLoader::builder("assets/i18n", unic_langid::langid!("en-US"))
            .shared_resources(Some(&["assets/shared.ftl".into()]))
            .customize(|bundle| bundle.set_use_isolating(false))
            .build()
            .expect("locales should load"),
    );

    engines::TeraView::build_with_post_process(move |tera| {
        tera.register_function("t", FluentLoader::new(loader.clone()));
        Ok(())
    })
    .expect("view engine should build")
}

#[test]
fn renders_login_form_with_error() {
    let view = engine();

    let rendered = view
        .render(
            "auth/login.html",
            data!({"email": "", "error": Some("nope")}),
        )
        .expect("login view should render");

    assert!(
        rendered.contains(r#"name="email""#),
        "expected the email field, got: {rendered}"
    );
    assert!(
        rendered.contains(r#"name="password""#),
        "expected the password field, got: {rendered}"
    );
    assert!(
        rendered.contains("nope"),
        "expected the error message, got: {rendered}"
    );
}

#[test]
fn renders_dashboard_inside_the_app_shell() {
    let view = engine();

    let rendered = view
        .render(
            "dashboard/index.html",
            data!({
                "user": {"pid": "p", "name": "Test User", "email": "t@example.com"},
                "active": "dashboard",
                // The same object `DashboardView` serializes; every key the template reads.
                "dashboard": {
                    "live": {
                        "captured_at_utc": "2026-09-27T12:00:00Z",
                        "captured_at": "2026-09-27 12:00:00 UTC"
                    },
                    "status": [
                        {
                            "name": "Captures",
                            "status": "Good",
                            "status_css": "bg-emerald-100 text-emerald-800",
                            "text": "14 captures today"
                        },
                        {
                            "name": "Disk",
                            "status": "OK",
                            "status_css": "bg-amber-100 text-amber-800",
                            "text": "3.0 GiB free of 24.0 GiB"
                        }
                    ]
                }
            }),
        )
        .expect("dashboard view should render");

    assert!(
        rendered.contains(r#"href="/""#),
        "expected the dashboard nav link from the shell, got: {rendered}"
    );
    // The shell's nav is what every page renders, so a page added to the sidebar
    // must be reachable from here, not only from itself.
    assert!(
        rendered.contains(r#"href="/logs""#),
        "expected the logs nav link from the shell, got: {rendered}"
    );
    assert!(
        rendered.contains(r#"href="/system""#),
        "expected the system nav link from the shell, got: {rendered}"
    );
    assert!(
        rendered.contains(r#"action="/logout""#),
        "expected the sign-out form from the shell, got: {rendered}"
    );
    assert!(
        rendered.contains("t@example.com"),
        "expected the user's email, got: {rendered}"
    );

    for expected in [
        // Live: the placeholder capture and its timestamp, converted by local-time.js.
        r#"src="/static/img/no-capture-available.svg""#,
        r#"<time datetime="2026-09-27T12:00:00Z" data-local-time>"#,
        "2026-09-27 12:00:00 UTC",
        r#"src="/static/js/local-time.js""#,
        // Status: the name, the text and the badge colour of a tile.
        "Captures",
        "14 captures today",
        "bg-emerald-100 text-emerald-800",
        "Disk",
        "3.0 GiB free of 24.0 GiB",
        "bg-amber-100 text-amber-800",
    ] {
        assert!(
            rendered.contains(expected),
            "expected {expected:?} in the rendered page, got: {rendered}"
        );
    }
}

/// The `data-sample="…"` attribute value, HTML-unescaped the way a browser decodes it,
/// parsed back into the JSON the charts see.
fn parse_data_sample(rendered: &str) -> serde_json::Value {
    let marker = r#"data-sample=""#;
    let start = rendered
        .find(marker)
        .expect("the panel should carry the chart payload")
        + marker.len();
    let end = start
        + rendered[start..]
            .find('"')
            .expect("the payload attribute should be closed");
    let escaped = &rendered[start..end];

    let unescaped = escaped
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&amp;", "&");

    serde_json::from_str(&unescaped).expect("the payload should be valid json")
}

/// The `sample` object the system page reads, with `disk` as the only variation.
fn system_sample(disk: &serde_json::Value) -> serde_json::Value {
    data!({
        "taken_at_ms": 1_758_000_000_000_i64,
        "cpu_total": 42.0,
        "cpu_total_label": "42.0%",
        "cores": [
            {"name": "cpu0", "usage": 42.0, "usage_label": "42.0%"},
            {"name": "cpu1", "usage": 7.5, "usage_label": "7.5%"}
        ],
        "memory": {
            "total_bytes": 8_589_934_592_u64,
            "used_bytes": 3_221_225_472_u64,
            "available_bytes": 5_368_709_120_u64,
            "used_percent": 37.5,
            "used_percent_label": "37.5%",
            "used_label": "3.0 GiB",
            "total_label": "8.0 GiB",
            "available_label": "5.0 GiB",
            "swap_total_bytes": 0_u64,
            "swap_used_bytes": 0_u64,
            "swap_label": "0 B / 0 B"
        },
        "temps": [{"label": "coretemp Package id 0", "celsius": 45.0, "celsius_label": "45.0 °C"}],
        "disk": disk
    })
}

#[test]
fn renders_system_page_with_metrics_and_chart_payload() {
    let view = engine();

    // The same object the controller builds; every field the templates read.
    let sample = system_sample(&data!({
        "mount_point": "/",
        "file_system": "ext4",
        "total_bytes": 25_769_803_776_u64,
        "used_bytes": 18_361_767_936_u64,
        "available_bytes": 7_408_035_840_u64,
        "used_percent": 71.3,
        "used_percent_label": "71.3%",
        "used_label": "17.1 GiB",
        "total_label": "24.0 GiB",
        "available_label": "6.9 GiB"
    }));
    let sample_json = serde_json::to_string(&sample).expect("the sample should serialize");

    let rendered = view
        .render(
            "system/index.html",
            data!({
                "user": {"pid": "p", "name": "Test User", "email": "t@example.com"},
                "active": "system",
                "info": {"hostname": "host-1", "os": "Debian GNU/Linux 13", "arch": "x86_64"},
                "sample": sample,
                "sample_json": sample_json,
                "hardware": {
                    "fan": {"speed_label": "60%"},
                    "environment": {
                        "humidity_label": "45.0%",
                        "pressure_label": "1013.0 hPa"
                    }
                }
            }),
        )
        .expect("system view should render");

    for expected in [
        "host-1",
        "Debian GNU/Linux 13",
        "x86_64",
        r#"hx-get="/system/metrics""#,
        r#"id="cpu-chart""#,
        r#"id="temp-chart""#,
        "cpu1",
        "3.0 GiB",
        "coretemp Package id 0",
        "6.9 GiB available · / (ext4)",
        "24.0 GiB",
        // The hardware card: the fan's commanded duty and the ambient reading.
        "Fan speed",
        "60%",
        "Humidity",
        "45.0%",
        "Pressure",
        "1013.0 hPa",
    ] {
        assert!(
            rendered.contains(expected),
            "expected {expected:?} in the rendered page, got: {rendered}"
        );
    }

    // The payload the charts read is the sample itself, not a re-encoding of it.
    let payload = parse_data_sample(&rendered);
    assert_eq!(payload["cpu_total"], data!(42.0));
    assert_eq!(payload["memory"]["used_percent"], data!(37.5));
    assert_eq!(payload["temps"][0]["celsius"], data!(45.0));
    assert_eq!(payload["disk"]["mount_point"], data!("/"));
}

#[test]
fn renders_system_page_without_a_disk() {
    let view = engine();
    let sample = system_sample(&data!(null));
    let sample_json = serde_json::to_string(&sample).expect("the sample should serialize");

    let rendered = view
        .render(
            "system/index.html",
            data!({
                "user": {"pid": "p", "name": "Test User", "email": "t@example.com"},
                "active": "system",
                "info": {"hostname": "host-1", "os": "Debian GNU/Linux 13", "arch": "x86_64"},
                "sample": sample,
                "sample_json": sample_json,
                "hardware": {"fan": null, "environment": null}
            }),
        )
        .expect("the system view should render without a disk");

    assert!(
        rendered.contains("No disks reported by this host."),
        "expected the no-disk text, got: {rendered}"
    );
    assert!(
        rendered.contains("No fan or environment sensor reported by this host."),
        "expected the no-hardware note, got: {rendered}"
    );
}

#[test]
fn renders_logs_page_with_entries_and_pager() {
    let view = engine();

    // The same object `LogsPageView` serializes; every key the template reads.
    let rendered = view
        .render(
            "logs/index.html",
            data!({
                "user": {"pid": "p", "name": "Test User", "email": "t@example.com"},
                "active": "logs",
                "logs": {
                    "entries": [{
                        "timestamp": "2026-09-26 10:02:00.000",
                        "timestamp_utc": "2026-09-26T10:02:00.000Z",
                        "level": "WARN",
                        "level_css": "bg-amber-100 text-amber-800",
                        "target": "sagan::test",
                        "message": "disk almost full",
                        "extra": r#"{"k":"v"}"#
                    }],
                    "page": 2,
                    "total_pages": 3,
                    "total_items": 120,
                    "truncated": false,
                    "dir": "logs",
                    "enabled": true,
                    "level_value": "warn",
                    "from": {
                        "date": "2026-09-26",
                        "time": "10:00:00",
                        "utc": "2026-09-26T10:00:00"
                    },
                    "to": {"date": "", "time": "", "utc": ""},
                    "prev_url": "/logs?page=1&level=warn",
                    "next_url": "/logs?page=3&level=warn",
                    "first_url": "/logs?page=1&level=warn",
                    "last_url": "/logs?page=3&level=warn"
                }
            }),
        )
        .expect("logs view should render");

    for expected in [
        "bg-amber-100 text-amber-800",
        "disk almost full",
        "&quot;k&quot;:&quot;v&quot;",
        "2026-09-26 10:02:00.000",
        // The row's timestamp: the instant for the browser, the UTC text as the fallback.
        r#"<time datetime="2026-09-26T10:02:00.000Z" data-local-time>"#,
        // The filter boundary the local-time script converts: the UTC instant on the group,
        // the split UTC values in the two controls the reader edits, and the hidden input
        // that is the only part of the boundary submitted.
        r#"id="logs-from" data-utc="2026-09-26T10:00:00""#,
        r#"id="logs-from-date" aria-label="From date" value="2026-09-26""#,
        r#"id="logs-from-time" aria-label="From time" step="1" value="10:00:00""#,
        r#"id="logs-from-utc" name="from" value="2026-09-26T10:00:00">"#,
        r#"src="/static/js/logs.js""#,
        // The pager keeps the active filters, HTML-escaped in the attribute.
        r#"href="/logs?page=3&amp;level=warn""#,
        r#"href="/logs?page=1&amp;level=warn""#,
        "Page 2 of 3",
        "120 entries",
        // The sidebar link, added by this page's `nav` block override.
        r#"href="/logs""#,
    ] {
        assert!(
            rendered.contains(expected),
            "expected {expected:?} in the rendered page, got: {rendered}"
        );
    }

    assert!(
        rendered.contains(r#"href="/system""#),
        "the overridden nav block should keep the shell's other links, got: {rendered}"
    );
    assert!(
        rendered.contains(r#"<option value="warn"  selected>"#),
        "the level select should echo the active filter, got: {rendered}"
    );
}

#[test]
fn renders_logs_empty_state() {
    let view = engine();

    let rendered = view
        .render(
            "logs/index.html",
            data!({
                "user": {"pid": "p", "name": "Test User", "email": "t@example.com"},
                "active": "logs",
                "logs": {
                    "entries": [],
                    "page": 1,
                    "total_pages": 1,
                    "total_items": 0,
                    "truncated": false,
                    "dir": "logs",
                    "enabled": false,
                    "level_value": "all",
                    "from": {"date": "", "time": "", "utc": ""},
                    "to": {"date": "", "time": "", "utc": ""},
                    "prev_url": null,
                    "next_url": null,
                    "first_url": null,
                    "last_url": null
                }
            }),
        )
        .expect("the empty logs view should render");

    assert!(
        rendered.contains("No log entries match these filters."),
        "expected the empty state, got: {rendered}"
    );
    assert!(
        rendered.contains("File logging is disabled"),
        "expected the disabled-logger note, got: {rendered}"
    );
}

#[test]
fn renders_jobs_page_with_workers_and_queue() {
    let view = engine();

    let rendered = view
        .render(
            "jobs/index.html",
            data!({
                "user": {"pid": "p", "name": "Test User", "email": "t@example.com"},
                "active": "jobs",
                "jobs": jobs_object(
                    &queue_object(
                        "BackgroundQueue",
                        "Jobs are stored in the queue and run by the worker process, so they outlive the request.",
                        "sqlite queue",
                        Some(true),
                        "Reachable."
                    ),
                    true,
                    &[],
                    &runtime_rows("live"),
                    None
                )
            }),
        )
        .expect("jobs view should render");

    for expected in [
        // The sidebar link this page's shell adds.
        r#"href="/jobs""#,
        "BackgroundQueue",
        "sqlite queue",
        "Reachable.",
        "CaptureWorker",
        "default",
        "—",
        "Takes a still image from the camera and stores it in the file store.",
        // The healthy line's colour, on the line that carries the ping's answer.
        "text-emerald-700",
        // The scheduler's own entries: what will be asked for, and when.
        "Scheduled",
        "0 * * * * *",
        // An empty queue in a mode that has one: the note, and the recovery button, since
        // the queue exists to be acted on.
        "Nothing in the queue.",
        r#"action="/jobs/requeue""#,
        // The button names the window the requeue rule uses, from the payload rather than a
        // second copy of the number in the template.
        "Requeue jobs stuck over 5 minutes",
        // This page overrides `head` to load the timestamp script, so it has to keep the
        // base's own head — without `super()` the Tailwind build is dropped and the page
        // renders unstyled.
        "https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4",
        // The runtime rows: what a live stamp looks like.
        "Runtime",
        "Scheduler",
        "Worker",
        ">live<",
        "(12.0 s ago)",
        "(3.0 s ago)",
        "host-1 · pid 42",
    ] {
        assert!(
            rendered.contains(expected),
            "expected {expected:?} in the rendered page, got: {rendered}"
        );
    }
    assert!(
        !rendered.contains("text-red-700"),
        "a provider that answered is not a failure: {rendered}"
    );
    assert!(
        !rendered.contains("Jobs are waiting"),
        "nothing is stuck while a worker is live: {rendered}"
    );
}

#[test]
fn renders_jobs_page_without_a_queue() {
    let view = engine();

    // A mode with no provider to ping and no queue to list: the notes replace the tables,
    // and no button is offered that could not work.
    let without_provider = view
        .render(
            "jobs/index.html",
            data!({
                "user": {"pid": "p", "name": "Test User", "email": "t@example.com"},
                "active": "jobs",
                "jobs": jobs_object(
                    &queue_object(
                        "ForegroundBlocking",
                        "Jobs run inline, in the process that enqueues them, before the call returns.",
                        "None configured for ForegroundBlocking; this mode keeps no queue.",
                        None,
                        ""
                    ),
                    false,
                    &[],
                    &runtime_rows("missing"),
                    None
                )
            }),
        )
        .expect("jobs view should render without a provider");

    for expected in [
        "None configured for ForegroundBlocking; this mode keeps no queue.",
        "This worker mode keeps no queue, so there are no job rows to show.",
        "This application schedules no recurring work.",
        // Never stamped: the age column says what to expect instead of showing a date.
        ">missing<",
        "never (expects one every 60 s)",
    ] {
        assert!(
            without_provider.contains(expected),
            "expected {expected:?}, got: {without_provider}"
        );
    }
    assert!(
        !without_provider.contains("/jobs/requeue") && !without_provider.contains("text-red-700"),
        "no provider is neither a failure nor something to act on: {without_provider}"
    );
    assert!(
        !without_provider.contains("Jobs are waiting"),
        "an empty queue is not stuck, whatever the stamps say: {without_provider}"
    );
}

#[test]
fn renders_jobs_page_with_job_rows_and_actions() {
    let view = engine();

    let rendered = view
        .render(
            "jobs/index.html",
            data!({
                "user": {"pid": "p", "name": "Test User", "email": "t@example.com"},
                "active": "jobs",
                "jobs": jobs_object(
                    &queue_object(
                        "BackgroundQueue",
                        "Jobs are stored in the queue and run by the worker process, so they outlive the request.",
                        "sqlite queue",
                        Some(true),
                        "Reachable."
                    ),
                    true,
                    &[
                        job_row("01M3JQUEUED000000000000001", "queued", true, false, "12.0 s"),
                        job_row("01M3JFAILED000000000000002", "failed", false, true, "320 ms"),
                        job_row("01M3JRUNNING00000000000003", "processing", false, false, "2m 05s"),
                        job_row("01M3JDONE00000000000000004", "completed", false, false, "1.5 s"),
                    ],
                    &runtime_rows("stale"),
                    Some("2 job(s) are waiting, and no worker has drained the queue for 3m 05s — is the worker process still running?")
                )
            }),
        )
        .expect("jobs view should render rows");

    for expected in [
        "4 jobs in the queue.",
        // Every status keeps its own badge, and the label is Loco's spelling.
        "bg-slate-100 text-slate-700",
        "bg-red-100 text-red-800",
        "bg-sky-100 text-sky-800",
        "bg-emerald-100 text-emerald-800",
        ">queued<",
        ">failed<",
        ">processing<",
        ">completed<",
        // The row's timestamp: the instant for the browser, the UTC text as the fallback.
        r#"<time datetime="2026-09-27T22:27:00+00:00" data-local-time>"#,
        "2026-09-27 22:27:00 UTC",
        r#"src="/static/js/local-time.js""#,
        "12.0 s",
        "320 ms",
        "2m 05s",
        "camera, nightly",
        // The queue-wide recovery button, and the two per-row ones.
        r#"action="/jobs/requeue""#,
        r#"action="/jobs/01M3JQUEUED000000000000001/cancel""#,
        r#"action="/jobs/01M3JFAILED000000000000002/retry""#,
        // A worker that stopped, and the waiting work that makes it worth saying so.
        ">stale<",
        "(3.0 s ago)",
    ] {
        assert!(
            rendered.contains(expected),
            "expected {expected:?} in the rendered page, got: {rendered}"
        );
    }

    // The warning is the page's own: rows waiting with a worker that stopped draining.
    assert!(
        rendered.contains("Jobs are waiting")
            && rendered.contains("2 job(s) are waiting, and no worker has drained the queue for 3m 05s — is the worker process still running?"),
        "expected the stuck warning, got: {rendered}"
    );
    assert!(
        rendered.contains("border-red-200 bg-red-50"),
        "the warning should stand out like one, got: {rendered}"
    );

    // A row offers a button only where the queue would act on it: one cancel (the queued
    // job), one retry (the failed one), and nothing for the running or finished rows.
    assert_eq!(rendered.matches(">Cancel<").count(), 1, "{rendered}");
    assert_eq!(rendered.matches(">Retry<").count(), 1, "{rendered}");
    assert!(
        !rendered.contains("/01M3JRUNNING00000000000003/"),
        "a running job cannot be cancelled or retried: {rendered}"
    );
    assert!(
        !rendered.contains("/01M3JDONE00000000000000004/"),
        "a completed job cannot be cancelled or retried: {rendered}"
    );
}

/// The `queue` object the jobs page reads, as `QueueView` serializes it.
fn queue_object(
    mode: &str,
    mode_detail: &str,
    provider: &str,
    healthy: Option<bool>,
    health_detail: &str,
) -> serde_json::Value {
    data!({
        "mode": mode,
        "mode_detail": mode_detail,
        "provider": provider,
        "healthy": healthy,
        "health_detail": health_detail
    })
}

/// One `Runtime` row as `RuntimeRowView` serializes it.
fn runtime_row(label: &str, state: &str, age_label: &str) -> serde_json::Value {
    let state_css = match state {
        "live" => "bg-emerald-100 text-emerald-800",
        "stale" => "bg-red-100 text-red-800",
        _ => "bg-slate-100 text-slate-700",
    };
    let seen = state != "missing";
    data!({
        "label": label,
        "stamp_writer": "whoever runs it",
        "state": state,
        "state_css": state_css,
        "age_label": age_label,
        "seen_utc": if seen { "2026-09-27T23:05:00+00:00" } else { "" },
        "seen_label": if seen { "2026-09-27 23:05:00 UTC" } else { "" },
        "origin": if seen { "host-1 · pid 42" } else { "" }
    })
}

/// The `jobs` object the jobs page reads: the fixed worker and scheduler halves, with the
/// queue's actionable flag, its rows, the runtime rows and the stuck warning as the only
/// variation.
fn jobs_object(
    queue: &serde_json::Value,
    actionable: bool,
    jobs: &[serde_json::Value],
    runtime: &[serde_json::Value],
    stuck: Option<&str>,
) -> serde_json::Value {
    data!({
        "queue": queue,
        "workers": [{
            "name": "CaptureWorker",
            "queue": "default",
            "tags": "—",
            "detail": "Takes a still image from the camera and stores it in the file store."
        }],
        "scheduled": if actionable {
            vec![data!({
                "name": "enqueue_capture",
                "run": "enqueue_capture",
                "schedule": "0 * * * * *",
                "tags": "—"
            })]
        } else {
            vec![]
        },
        "jobs": jobs,
        "actionable": actionable,
        "stale_minutes": 5,
        "runtime": runtime,
        "stuck": stuck
    })
}

/// Both `Runtime` rows, with the worker's state as the variation: the scheduler is live
/// whenever the worker is, since the same tick writes both.
fn runtime_rows(worker_state: &str) -> Vec<serde_json::Value> {
    let scheduler = if worker_state == "missing" {
        "missing"
    } else {
        "live"
    };
    vec![
        runtime_row(
            "Scheduler",
            scheduler,
            if scheduler == "missing" {
                "never (expects one every 60 s)"
            } else {
                "12.0 s"
            },
        ),
        runtime_row(
            "Worker",
            worker_state,
            if worker_state == "missing" {
                "never (expects one every 60 s)"
            } else {
                "3.0 s"
            },
        ),
    ]
}

/// One row as `JobRowView` serializes it: `status` picks the badge and the verbs, and the
/// timestamps are fixed so the fallback text is predictable.
fn job_row(
    id: &str,
    status: &str,
    can_cancel: bool,
    can_retry: bool,
    elapsed: &str,
) -> serde_json::Value {
    let status_css = match status {
        "queued" => "bg-slate-100 text-slate-700",
        "processing" => "bg-sky-100 text-sky-800",
        "completed" => "bg-emerald-100 text-emerald-800",
        "failed" => "bg-red-100 text-red-800",
        _ => "bg-amber-100 text-amber-800",
    };
    data!({
        "id": id,
        "id_short": &id[..8],
        "name": "CaptureWorker",
        "status": status,
        "status_css": status_css,
        "created_utc": "2026-09-27T22:27:00+00:00",
        "created_label": "2026-09-27 22:27:00 UTC",
        "elapsed_label": elapsed,
        "run_at_utc": "2026-09-27T22:27:00+00:00",
        "tags": if status == "failed" { "camera, nightly" } else { "—" },
        "can_cancel": can_cancel,
        "can_retry": can_retry
    })
}
