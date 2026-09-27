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
