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
                "active": "dashboard"
            }),
        )
        .expect("dashboard view should render");

    assert!(
        rendered.contains(r#"href="/""#),
        "expected the dashboard nav link from the shell, got: {rendered}"
    );
    assert!(
        rendered.contains(r#"action="/logout""#),
        "expected the sign-out form from the shell, got: {rendered}"
    );
    assert!(
        rendered.contains("t@example.com"),
        "expected the user's email, got: {rendered}"
    );
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

#[test]
fn renders_system_page_with_metrics_and_chart_payload() {
    let view = engine();

    // The same object the controller builds; every field the templates read.
    let sample = data!({
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
        "temps": [{"label": "coretemp Package id 0", "celsius": 45.0, "celsius_label": "45.0 °C"}]
    });
    let sample_json = serde_json::to_string(&sample).expect("the sample should serialize");

    let rendered = view
        .render(
            "system/index.html",
            data!({
                "user": {"pid": "p", "name": "Test User", "email": "t@example.com"},
                "active": "system",
                "info": {"hostname": "host-1", "os": "Debian GNU/Linux 13", "arch": "x86_64"},
                "sample": sample,
                "sample_json": sample_json
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
}
