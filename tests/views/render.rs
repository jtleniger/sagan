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
