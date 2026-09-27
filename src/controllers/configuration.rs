use crate::{
    captures::{CaptureParams, CaptureSettings},
    controllers::current_user,
    models::app_settings,
    views::{configuration::CapturesView, user::UserView},
};
use axum::http::StatusCode;
use loco_rs::prelude::*;
use serde::Deserialize;

/// The redirect marker `save_captures` sets, so a reload after a save re-reads instead of
/// re-posting the form.
#[derive(Debug, Deserialize)]
pub struct PageParams {
    pub saved: Option<String>,
}

/// `GET /configuration` — the runtime settings, section by section.
#[debug_handler]
async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
    Query(params): Query<PageParams>,
) -> Result<Response> {
    let Some(user) = current_user(&ctx, auth).await? else {
        return format::redirect("/login");
    };

    let settings = app_settings::Model::capture_settings(&ctx.db).await?;

    render_page(
        &v,
        &user,
        &CapturesView::from_interval(&settings.interval),
        None,
        params.saved.is_some(),
    )
}

/// `POST /configuration/captures` — save the Captures interval.
///
/// A rejected submission re-renders the page (422) with the message and the values as typed; a
/// saved one redirects, so a reload re-reads.
#[debug_handler]
async fn save_captures(
    ViewEngine(v): ViewEngine<TeraView>,
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
    Form(params): Form<CaptureParams>,
) -> Result<Response> {
    let Some(user) = current_user(&ctx, auth).await? else {
        return format::redirect("/login");
    };

    let settings = app_settings::Model::capture_settings(&ctx.db).await?;

    let interval = match params.interval() {
        Ok(interval) => interval,
        Err(message) => {
            return render_page(
                &v,
                &user,
                &CapturesView::rejected(&settings.interval, &params),
                Some(&message),
                false,
            );
        }
    };

    app_settings::Model::save_capture_settings(&ctx.db, &CaptureSettings { interval }).await?;

    format::redirect("/configuration?saved=1")
}

/// Renders the page; a message means the submission was rejected, which is a 422 rather than a
/// successful 200 with an error banner.
fn render_page(
    v: &TeraView,
    user: &UserView,
    captures: &CapturesView,
    error: Option<&str>,
    saved: bool,
) -> Result<Response> {
    let status = if error.is_some() {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };

    format::render().status(status).view(
        v,
        "configuration/index.html",
        data!({
            "user": user,
            "active": "configuration",
            "captures": captures,
            "error": error,
            "saved": saved,
        }),
    )
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/configuration")
        .add("/", get(index))
        .add("/captures", post(save_captures))
}
