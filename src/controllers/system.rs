use std::sync::Arc;

use crate::{
    controllers::current_user,
    monitor::SystemMonitor,
    views::system::{SystemInfoView, SystemSampleView},
};
use axum::http::StatusCode;
use loco_rs::prelude::*;

/// The process-wide monitor, built at boot in `Hooks::after_context`.
fn monitor(ctx: &AppContext) -> Result<Arc<SystemMonitor>> {
    ctx.shared_store
        .get::<Arc<SystemMonitor>>()
        .ok_or(Error::InternalServerError)
}

/// `GET /system` — live host metrics.
#[debug_handler]
async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let Some(user) = current_user(&ctx, auth).await? else {
        return format::redirect("/login");
    };

    let monitor = monitor(&ctx)?;
    let sample = SystemSampleView::from(&monitor.sample());
    // Drives the charts on the client; see assets/static/js/system.js.
    let sample_json = serde_json::to_string(&sample)?;

    format::render().view(
        &v,
        "system/index.html",
        data!({
            "user": user,
            "active": "system",
            "info": SystemInfoView::from(monitor.info()),
            "sample": sample,
            "sample_json": sample_json,
        }),
    )
}

/// `GET /system/metrics` — the fragment htmx swaps in every 2 s.
///
/// Deliberately does not resolve the user row (that would be a query per poll): a signed
/// token is enough to read host metrics, and nothing user-specific is rendered.
#[debug_handler]
async fn metrics(
    ViewEngine(v): ViewEngine<TeraView>,
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    if auth.is_none() {
        // htmx follows `HX-Redirect` instead of swapping a 401 body, so an expired session
        // lands on the login form instead of a dead panel.
        return format::render()
            .status(StatusCode::UNAUTHORIZED)
            .header("HX-Redirect", "/login")
            .empty();
    }

    let monitor = monitor(&ctx)?;
    let sample = SystemSampleView::from(&monitor.sample());
    let sample_json = serde_json::to_string(&sample)?;

    format::render().view(
        &v,
        "system/_metrics.html",
        data!({"sample": sample, "sample_json": sample_json}),
    )
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/system")
        .add("/", get(index))
        .add("/metrics", get(metrics))
}
