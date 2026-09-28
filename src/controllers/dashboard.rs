use axum::{body::Body, http::StatusCode};

use crate::{
    controllers::{current_user, monitor},
    live::{self, LiveConfig},
    views::dashboard::DashboardView,
};
use loco_rs::prelude::*;

/// `GET /` — the signed-in home page.
///
/// HTML, so a missing/expired/forged cookie must land on the login form, not on
/// a 401 JSON body: the resolution in `controllers::current_user` returns `None`
/// for every token failure instead of rejecting, which is exactly this behaviour.
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

    format::render().view(
        &v,
        "dashboard/index.html",
        data!({
            "user": user,
            "active": "dashboard",
            "dashboard": DashboardView::from_sample(&monitor.sample()),
            // The Live card's own payload, a top-level key like the system page's `sample`.
            "live": live::snapshot(&ctx)?,
        }),
    )
}

/// `GET /live` — the Live card's fragment, htmx-swapped every 15 s (`dashboard/_live.html`).
///
/// Deliberately does not resolve the user row (that would be a query per poll), the way
/// `/system/metrics` does not.
#[debug_handler]
async fn live_view(
    ViewEngine(v): ViewEngine<TeraView>,
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    if auth.is_none() {
        // htmx follows `HX-Redirect` instead of swapping a 401 body.
        return format::render()
            .status(StatusCode::UNAUTHORIZED)
            .header("HX-Redirect", "/login")
            .empty();
    }

    let live = live::refresh(&ctx).await?;
    format::render().view(&v, "dashboard/_live.html", data!({ "live": live }))
}

/// `GET /live/image` — the newest live frame's bytes.
///
/// The `at` query the card appends is a cache-buster only; what is served is the newest frame, so
/// a poll and the image it named can never disagree about which file exists.
#[debug_handler]
async fn live_image(auth: Option<auth::JWT>, State(ctx): State<AppContext>) -> Result<Response> {
    if auth.is_none() {
        return format::render().status(StatusCode::UNAUTHORIZED).empty();
    }

    let dir = LiveConfig::from_context(&ctx.config)?.dir;
    let Some(frame) = live::newest(&dir) else {
        return format::render().status(StatusCode::NOT_FOUND).empty();
    };

    format::render()
        .header("content-type", "image/jpeg")
        .header("cache-control", "no-store")
        .response()
        .body(Body::from(std::fs::read(frame.path)?))
        .map_err(Error::from)
}

pub fn routes() -> Routes {
    Routes::new()
        .add("/", get(index))
        .add("/live", get(live_view))
        .add("/live/image", get(live_image))
}
